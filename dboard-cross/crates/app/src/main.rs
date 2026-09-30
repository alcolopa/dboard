// Hide the console window on Windows release builds.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

slint::include_modules!();

use dboard_core::config;
use dboard_core::model::{Cell, ConnectionConfig, Environment, Page, TableKind};
use dboard_core::postgres::PgDriver;
use dboard_core::safety;
use slint::{ModelRc, SharedString, VecModel, Weak};
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

const PAGE_SIZE: i64 = 100;

enum Cmd {
    Connect(ConnectionConfig, String),
    Disconnect,
    OpenTable(usize),
    Edit { row: usize, col: usize, text: String, null: bool },
    ClearFlash,
    Undo,
    NextPage,
    PrevPage,
    SortBy(usize),
    ApplyFilter(String),
    Refresh,
    Query(String),
    ConfirmRun,
    ConfirmCancel,
}

/// (value, state) where state: 0 idle, 1 saving, 2 saved, 3 error.
type GridRows = Vec<Vec<(Cell, i32)>>;

fn ui(w: &Weak<App>, f: impl FnOnce(&App) + Send + 'static) {
    let w = w.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = w.upgrade() {
            f(&app);
        }
    });
}

fn to_model(rows: GridRows) -> ModelRc<ModelRc<GridCell>> {
    let rows: Vec<ModelRc<GridCell>> = rows
        .into_iter()
        .map(|r| {
            let cells: Vec<GridCell> = r
                .into_iter()
                .map(|(c, state)| GridCell {
                    is_null: c.is_none(),
                    text: c.unwrap_or_default().into(),
                    state,
                })
                .collect();
            ModelRc::new(VecModel::from(cells))
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn to_strs(v: Vec<String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(v.into_iter().map(SharedString::from).collect::<Vec<_>>()))
}

struct Worker {
    w: Weak<App>,
    tx: UnboundedSender<Cmd>,
    driver: Option<PgDriver>,
    env: Environment,
    /// Sidebar rows: (schema, name, is_header)
    tree: Vec<(String, String, bool)>,
    current: Option<(String, String)>,
    grid_is_table: bool,
    page: Page,
    rows: Vec<Vec<Cell>>,
    pending_sql: Option<String>,
    log: Vec<String>,
}

impl Worker {
    fn log(&mut self, msg: String) {
        self.log.insert(0, msg);
        self.log.truncate(200);
        let l = self.log.clone();
        ui(&self.w, move |a| a.set_log(to_strs(l)));
    }

    fn push_grid(&self, columns: Vec<String>, flash: Option<(usize, usize, i32)>) {
        let grid: GridRows = self
            .rows
            .iter()
            .enumerate()
            .map(|(r, row)| {
                row.iter()
                    .enumerate()
                    .map(|(c, v)| (v.clone(), flash.filter(|f| f.0 == r && f.1 == c).map_or(0, |f| f.2)))
                    .collect()
            })
            .collect();
        ui(&self.w, move |a| {
            a.set_columns(to_strs(columns));
            a.set_rows(to_model(grid));
        });
    }

    fn columns(&self) -> Vec<String> {
        match (&self.driver, &self.current) {
            (Some(d), Some((s, n))) if self.grid_is_table => d
                .table(s, n)
                .map(|t| t.columns.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn set_error(&self, msg: String) {
        ui(&self.w, move |a| a.set_error(msg.into()));
    }

    fn rebuild_tree(&mut self) {
        let Some(d) = &self.driver else { return };
        let mut tree = Vec::new();
        let mut last = String::new();
        for t in &d.metadata.tables {
            if t.schema != last {
                tree.push((t.schema.clone(), String::new(), true));
                last = t.schema.clone();
            }
            tree.push((t.schema.clone(), t.name.clone(), false));
        }
        let items: Vec<TreeItem> = tree
            .iter()
            .map(|(s, n, h)| {
                let kind = d.table(s, n).map(|t| t.kind);
                let suffix = match kind {
                    Some(TableKind::View) => "  (view)",
                    Some(TableKind::MaterializedView) => "  (mat. view)",
                    _ => "",
                };
                TreeItem {
                    header: *h,
                    label: if *h { s.to_uppercase().into() } else { format!("{n}{suffix}").into() },
                    schema: s.as_str().into(),
                    name: n.as_str().into(),
                }
            })
            .collect();
        self.tree = tree;
        ui(&self.w, move |a| a.set_tree(ModelRc::new(VecModel::from(items))));
    }

    async fn load_page(&mut self) {
        let Some((s, n)) = self.current.clone() else { return };
        let Some(d) = &self.driver else { return };
        match d.fetch_page(&s, &n, &self.page).await {
            Ok(r) => {
                let editable = d.table(&s, &n).is_some_and(|t| t.kind == TableKind::Table && t.has_primary_key());
                let is_table = d.table(&s, &n).is_some_and(|t| t.kind == TableKind::Table);
                let banner = if is_table && !editable {
                    "No primary key: inline editing is disabled to protect your data."
                } else {
                    ""
                };
                let from = self.page.offset + 1;
                let to = self.page.offset + r.rows.len() as i64;
                let info = match r.total_estimate {
                    Some(e) if e > 0 => format!("Rows {from}–{to} of ~{e}"),
                    _ => format!("Rows {from}–{to}"),
                };
                let timing = format!("{:.1} ms", r.duration_ms);
                let sort = self.page.sort_column.clone().unwrap_or_default();
                self.grid_is_table = true;
                self.rows = r.rows;
                let cols = r.columns;
                self.push_grid(cols, None);
                ui(&self.w, move |a| {
                    a.set_editable(editable);
                    a.set_banner(banner.into());
                    a.set_page_info(info.into());
                    a.set_timing(timing.into());
                    a.set_sort_label(sort.into());
                    a.set_table_title(format!("{s}.{n}").into());
                    a.set_error("".into());
                });
                self.log(format!("{:.1} ms · SELECT from {}.{}", r.duration_ms, self.current.as_ref().unwrap().0, self.current.as_ref().unwrap().1));
            }
            Err(e) => self.set_error(e.to_string()),
        }
    }

    async fn run_query(&mut self, sql: String) {
        let Some(d) = &self.driver else { return };
        match d.execute_query(&sql).await {
            Ok(r) => {
                self.grid_is_table = false;
                self.rows = r.rows;
                let n = self.rows.len();
                let timing = format!("{n} rows · {:.1} ms", r.duration_ms);
                self.push_grid(r.columns, None);
                ui(&self.w, move |a| {
                    a.set_editable(false);
                    a.set_banner("".into());
                    a.set_timing(timing.clone().into());
                    a.set_page_info(timing.into());
                    a.set_error("".into());
                });
                let first = sql.lines().next().unwrap_or("").to_string();
                self.log(format!("{:.1} ms · {first}", r.duration_ms));
            }
            Err(e) => {
                self.log(format!("ERROR · {e}"));
                self.set_error(e.to_string());
                let msg = e.to_string();
                ui(&self.w, move |a| a.set_banner(msg.into()));
            }
        }
    }

    fn sync_undo(&self) {
        let n = self.driver.as_ref().map_or(0, |d| d.history.len()) as i32;
        ui(&self.w, move |a| a.set_undo_count(n));
    }

    async fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Connect(cfg, pw) => {
                match PgDriver::connect(cfg.clone(), &pw).await {
                    Ok(d) => {
                        let ver = d.server_version().await.unwrap_or_default();
                        let status = format!("{}@{}:{} / {} · PostgreSQL {ver}", cfg.username, cfg.host, cfg.port, cfg.database);
                        self.env = cfg.environment;
                        self.driver = Some(d);
                        config::save_password(&cfg, &pw);
                        let _ = config::save_connections(&[cfg]);
                        self.rebuild_tree();
                        ui(&self.w, move |a| {
                            a.set_status(status.into());
                            a.set_error("".into());
                            a.set_busy(false);
                            a.set_connected(true);
                        });
                    }
                    Err(e) => {
                        let m = e.to_string();
                        ui(&self.w, move |a| {
                            a.set_error(m.into());
                            a.set_busy(false);
                        });
                    }
                }
            }
            Cmd::Disconnect => {
                self.driver = None;
                self.current = None;
                self.rows.clear();
                ui(&self.w, |a| {
                    a.set_connected(false);
                    a.set_rows(to_model(Vec::new()));
                    a.set_columns(to_strs(Vec::new()));
                    a.set_tree(ModelRc::new(VecModel::from(Vec::<TreeItem>::new())));
                });
            }
            Cmd::OpenTable(i) => {
                let Some((s, n, false)) = self.tree.get(i).cloned() else { return };
                self.current = Some((s, n));
                self.page = Page { limit: PAGE_SIZE, offset: 0, sort_column: None, sort_ascending: true, filter: None };
                ui(&self.w, |a| a.set_filter_text("".into()));
                self.load_page().await;
            }
            Cmd::NextPage => {
                if self.grid_is_table && self.rows.len() as i64 == self.page.limit {
                    self.page.offset += self.page.limit;
                    self.load_page().await;
                }
            }
            Cmd::PrevPage => {
                if self.grid_is_table && self.page.offset > 0 {
                    self.page.offset = (self.page.offset - self.page.limit).max(0);
                    self.load_page().await;
                }
            }
            Cmd::SortBy(c) => {
                if !self.grid_is_table {
                    return;
                }
                let Some(name) = self.columns().get(c).cloned() else { return };
                if self.page.sort_column.as_deref() == Some(&name) {
                    self.page.sort_ascending = !self.page.sort_ascending;
                } else {
                    self.page.sort_column = Some(name);
                    self.page.sort_ascending = true;
                }
                self.page.offset = 0;
                self.load_page().await;
            }
            Cmd::ApplyFilter(f) => {
                self.page.filter = Some(f).filter(|f| !f.trim().is_empty());
                self.page.offset = 0;
                self.load_page().await;
            }
            Cmd::Refresh => {
                if let Some(d) = &mut self.driver {
                    if let Err(e) = d.refresh_metadata().await {
                        self.set_error(e.to_string());
                        return;
                    }
                }
                self.rebuild_tree();
                if self.grid_is_table {
                    self.load_page().await;
                }
            }
            Cmd::Edit { row, col, text, null } => {
                if !self.grid_is_table {
                    return;
                }
                let Some((s, n)) = self.current.clone() else { return };
                let cols = self.columns();
                let (Some(colname), Some(current)) = (cols.get(col).cloned(), self.rows.get(row).cloned()) else { return };
                let new: Cell = if null { None } else { Some(text) };
                if current.get(col).cloned().flatten() == new && (null == current[col].is_none()) {
                    return; // unchanged
                }
                self.flash(cols.clone(), row, col, 1, &new);
                let Some(d) = &mut self.driver else { return };
                match d.edit_cell(&s, &n, &current, &colname, new.clone()).await {
                    Ok(()) => {
                        self.rows[row][col] = new;
                        self.push_grid(cols, Some((row, col, 2)));
                        self.log(format!("UPDATE {s}.{n} SET {colname}"));
                        self.sync_undo();
                        self.schedule_clear();
                    }
                    Err(e) => {
                        // Revert the displayed value and surface the error.
                        self.push_grid(cols, Some((row, col, 3)));
                        self.log(format!("ERROR · {e}"));
                        let m = e.to_string();
                        ui(&self.w, move |a| a.set_banner(m.into()));
                        self.schedule_clear();
                    }
                }
            }
            Cmd::ClearFlash => {
                let cols = self.columns();
                if self.grid_is_table {
                    self.push_grid(cols, None);
                }
            }
            Cmd::Undo => {
                let Some(d) = &mut self.driver else { return };
                match d.undo().await {
                    Ok(Some(r)) => {
                        self.log(format!("UNDO {}.{} SET {}", r.schema, r.table, r.column));
                        if self.grid_is_table {
                            self.load_page().await;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => self.set_error(e.to_string()),
                }
                self.sync_undo();
            }
            Cmd::Query(sql) => {
                if sql.trim().is_empty() {
                    return;
                }
                if safety::requires_confirmation(self.env, safety::is_destructive(&sql)) {
                    self.pending_sql = Some(sql.clone());
                    ui(&self.w, move |a| {
                        a.set_confirm_sql(sql.into());
                        a.set_confirm_typed("".into());
                        a.set_confirm_open(true);
                    });
                } else {
                    self.run_query(sql).await;
                }
            }
            Cmd::ConfirmRun => {
                ui(&self.w, |a| a.set_confirm_open(false));
                if let Some(sql) = self.pending_sql.take() {
                    self.run_query(sql).await;
                }
            }
            Cmd::ConfirmCancel => {
                self.pending_sql = None;
                ui(&self.w, |a| a.set_confirm_open(false));
            }
        }
    }

    fn flash(&self, cols: Vec<String>, r: usize, c: usize, state: i32, new: &Cell) {
        // Show the new value immediately with a "saving" spinner (optimistic).
        let mut rows = self.rows.clone();
        rows[r][c] = new.clone();
        let grid: GridRows = rows
            .into_iter()
            .enumerate()
            .map(|(ri, row)| {
                row.into_iter().enumerate().map(|(ci, v)| (v, if ri == r && ci == c { state } else { 0 })).collect()
            })
            .collect();
        ui(&self.w, move |a| {
            a.set_columns(to_strs(cols));
            a.set_rows(to_model(grid));
        });
    }

    fn schedule_clear(&self) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let _ = tx.send(Cmd::ClearFlash);
        });
    }
}

fn main() {
    let app = App::new().unwrap();
    let (tx, mut rx) = unbounded_channel::<Cmd>();

    // Prefill the last-used connection.
    if let Some(c) = config::load_connections().into_iter().next() {
        app.set_conn_host(c.host.clone().into());
        app.set_conn_port(c.port.to_string().into());
        app.set_conn_db(c.database.clone().into());
        app.set_conn_user(c.username.clone().into());
        app.set_conn_env(Environment::ALL.iter().position(|e| *e == c.environment).unwrap_or(3) as i32);
        if let Some(pw) = config::load_password(&c) {
            app.set_conn_password(pw.into());
        }
    }

    macro_rules! send {
        ($($body:tt)*) => {{ let tx = tx.clone(); move |$($body)*| }};
    }

    let a = app.as_weak();
    app.on_connect(send!() => {
        let Some(app) = a.upgrade() else { return };
        let cfg = ConnectionConfig {
            name: "Postgres".into(),
            host: app.get_conn_host().to_string(),
            port: app.get_conn_port().parse().unwrap_or(5432),
            database: app.get_conn_db().to_string(),
            username: app.get_conn_user().to_string(),
            environment: Environment::ALL[(app.get_conn_env() as usize).min(3)],
        };
        app.set_busy(true);
        app.set_error("".into());
        let _ = tx.send(Cmd::Connect(cfg, app.get_conn_password().to_string()));
    });
    app.on_disconnect(send!() => { let _ = tx.send(Cmd::Disconnect); });
    app.on_open_table(send!(i) => { let _ = tx.send(Cmd::OpenTable(i as usize)); });
    app.on_edit_cell(send!(r, c, t, n) => {
        let _ = tx.send(Cmd::Edit { row: r as usize, col: c as usize, text: t.to_string(), null: n });
    });
    app.on_undo(send!() => { let _ = tx.send(Cmd::Undo); });
    app.on_next_page(send!() => { let _ = tx.send(Cmd::NextPage); });
    app.on_prev_page(send!() => { let _ = tx.send(Cmd::PrevPage); });
    app.on_sort_by(send!(c) => { let _ = tx.send(Cmd::SortBy(c as usize)); });
    app.on_apply_filter(send!(f) => { let _ = tx.send(Cmd::ApplyFilter(f.to_string())); });
    app.on_refresh(send!() => { let _ = tx.send(Cmd::Refresh); });
    app.on_run_query(send!(s) => { let _ = tx.send(Cmd::Query(s.to_string())); });
    app.on_confirm_run(send!() => { let _ = tx.send(Cmd::ConfirmRun); });
    app.on_confirm_cancel(send!() => { let _ = tx.send(Cmd::ConfirmCancel); });

    let weak = app.as_weak();
    let worker_tx = tx.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let mut w = Worker {
                w: weak,
                tx: worker_tx,
                driver: None,
                env: Environment::Local,
                tree: Vec::new(),
                current: None,
                grid_is_table: false,
                page: Page::default(),
                rows: Vec::new(),
                pending_sql: None,
                log: Vec::new(),
            };
            while let Some(cmd) = rx.recv().await {
                w.handle(cmd).await;
            }
        });
    });

    app.run().unwrap();
}
