//! The worker thread: owns the database connection and all persisted state, and talks to the
//! UI thread only through `ui()` (updates) and `Cmd` (requests).

use crate::export::{self, ExportCol};
use crate::suggest;
use crate::{App, AppState, ColInfo, ConnForm, ConnItem, FieldItem, GridCell, HistoryItem, PaletteItem, SavedItem, TabInfo, TreeItem};
use dboard_core::config::{self, secrets, HistoryEntry, SavedQuery, Settings, Store, Theme as ThemePref, FOLDERS};
use dboard_core::model::*;
use dboard_core::sql::Dialect;
use dboard_core::{mongo, safety, Conn};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel, Weak};
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

const PAGE_SIZES: [i64; 6] = [25, 50, 100, 250, 500, 1000];
const RESULT_CAP: usize = 5000;

pub enum Cmd {
    // connection manager
    NewConn,
    SelectConn(String),
    SaveConn(ConnForm),
    TestConn(ConnForm),
    ConnectConn(ConnForm),
    DeleteConn(String),
    DuplicateConn(String),
    // session
    Disconnect,
    Refresh,
    Undo,
    // sidebar
    FilterTree(String),
    TreeClick(usize),
    TreeAction(usize, String),
    // tabs
    NewQueryTab,
    ActivateTab(usize),
    CloseTab(i32),
    TabAction(usize, String),
    ReopenTab,
    // grid
    EditCell(usize, usize, String, bool),
    ToggleBool(usize, usize),
    OpenJsonCell(usize, usize),
    SortBy(usize),
    ColResized(usize, f32),
    ApplyFilter(String),
    NextPage,
    PrevPage,
    SetPageSize(usize),
    CopyText(String),
    CellCopy(usize, usize),
    DeleteRow(usize),
    OpenInsert,
    OpenDoc(usize),
    // query
    QueryEdited(String),
    ApplySuggestion(String),
    RunQuery(String),
    ExplainQuery(String, bool),
    InsertTemplate(String),
    OpenSaveQuery,
    // dialogs
    ConfirmRun,
    ConfirmCancel,
    InsertFieldEdited(usize, String),
    InsertSubmit,
    InsertCancel,
    JsonSave(String),
    JsonFormat(String),
    JsonCancel,
    SaveQuerySubmit(String, usize),
    SaveQueryCancel,
    LoadHistory(usize),
    LoadSaved(usize),
    DeleteSaved(usize),
    ClearHistory,
    ClearActivity,
    OpenPalette(bool),
    PaletteChanged(String),
    PaletteRun(usize),
    PaletteClose,
    OpenExport,
    ExportCopy(usize, bool),
    ExportSave(usize, bool),
    ExportCancel,
    OpenSettings,
    SettingsChanged { dark: bool, compact: bool, page_idx: usize, confirm: bool, autocomplete: bool, font: i32 },
    SettingsClose,
    ClearCredentials,
    // internal
    ClearFlash,
    ClearToast(u64),
}

// -------------------------------------------------------------------------------------------
// Plain (Send) view-models pushed to the UI
// -------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
struct ColMeta {
    name: String,
    type_name: String,
    pk: bool,
    fk: String,
    is_bool: bool,
    is_json: bool,
}

impl ColMeta {
    fn from_column(c: &Column) -> Self {
        Self {
            name: c.name.clone(),
            type_name: c.type_name.clone(),
            pk: c.is_primary_key,
            fk: c.fk.clone().unwrap_or_default(),
            is_bool: c.is_bool(),
            is_json: c.is_json(),
        }
    }
    fn plain(name: &str) -> Self {
        Self { name: name.to_string(), ..Default::default() }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Table = 0,
    Query = 1,
    Structure = 2,
    Routine = 3,
}

#[derive(Clone)]
struct Tab {
    kind: Kind,
    title: String,
    pinned: bool,
    schema: String,
    name: String,
    obj: Option<DbObject>,
    page: Page,
    filter_text: String,
    query_text: String,
    cols: Vec<ColMeta>,
    widths: Vec<f32>,
    rows: Vec<Vec<Cell>>,
    editable: bool,
    banner: String,
    banner_err: bool,
    page_info: String,
    timing: String,
    ddl: String,
}

impl Tab {
    fn new(kind: Kind, title: impl Into<String>, page_size: i64) -> Self {
        Self {
            kind,
            title: title.into(),
            pinned: false,
            schema: String::new(),
            name: String::new(),
            obj: None,
            page: Page { limit: page_size, offset: 0, sort_column: None, sort_ascending: true, filter: None },
            filter_text: String::new(),
            query_text: String::new(),
            cols: Vec::new(),
            widths: Vec::new(),
            rows: Vec::new(),
            editable: false,
            banner: String::new(),
            banner_err: false,
            page_info: String::new(),
            timing: String::new(),
            ddl: String::new(),
        }
    }
}

fn auto_widths(cols: &[ColMeta], rows: &[Vec<Cell>]) -> Vec<f32> {
    cols.iter()
        .enumerate()
        .map(|(i, c)| {
            let mut n = c.name.chars().count().max(c.type_name.chars().count().min(24) / 2);
            for r in rows.iter().take(50) {
                n = n.max(r.get(i).and_then(|v| v.as_ref()).map_or(4, |v| v.chars().take(40).count()));
            }
            (n as f32 * 7.6 + 30.0).clamp(80.0, 340.0)
        })
        .collect()
}

fn ui(w: &Weak<App>, f: impl FnOnce(AppState) + Send + 'static) {
    let w = w.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = w.upgrade() {
            f(app.global::<AppState>());
        }
    });
}

fn strs(v: Vec<String>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(v.into_iter().map(SharedString::from).collect::<Vec<_>>()))
}

fn grid_model(rows: Vec<Vec<(Cell, i32)>>) -> ModelRc<ModelRc<GridCell>> {
    let rows: Vec<ModelRc<GridCell>> = rows
        .into_iter()
        .map(|r| {
            let cells: Vec<GridCell> = r
                .into_iter()
                .map(|(c, state)| GridCell { is_null: c.is_none(), text: c.unwrap_or_default().into(), state })
                .collect();
            ModelRc::new(VecModel::from(cells))
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

fn hms() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn when(secs: u64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_opt(secs as i64, 0).single().map(|d| d.format("%b %d %H:%M").to_string()).unwrap_or_default()
}

fn one_line(s: &str) -> String {
    let l: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if l.chars().count() > 120 { format!("{}…", l.chars().take(120).collect::<String>()) } else { l }
}

fn downloads_dir() -> std::path::PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(std::path::PathBuf::from);
    let d = home.clone().map(|h| h.join("Downloads"));
    match d {
        Some(d) if d.is_dir() || std::fs::create_dir_all(&d).is_ok() => d,
        _ => home.unwrap_or_else(std::env::temp_dir),
    }
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

// -------------------------------------------------------------------------------------------

enum Pending {
    Query(String),
    Explain(String, bool),
    DeleteRow(usize),
    Truncate(String, String),
    Drop(String, String),
}

enum JsonTarget {
    Cell(usize, usize),
    Doc(Vec<Cell>),
    NewDoc,
}

enum PaletteAction {
    Command(&'static str),
    OpenTable(String, String),
    Column(String, String),
    Object(DbObject),
    Saved(usize),
    Connection(String),
}

#[derive(Clone)]
struct TreeEntry {
    kind: i32,
    schema: String,
    name: String,
    key: String,
}

pub struct Worker {
    w: Weak<App>,
    tx: UnboundedSender<Cmd>,
    store: Store,
    connections: Vec<ConnectionConfig>,
    settings: Settings,
    history: Vec<HistoryEntry>,
    saved: Vec<SavedQuery>,
    conn: Option<Conn>,
    env: Environment,
    tabs: Vec<Tab>,
    active: Option<usize>,
    closed: Vec<Tab>,
    query_counter: usize,
    tree_filter: String,
    collapsed: HashSet<String>,
    tree: Vec<TreeEntry>,
    activity: Vec<String>,
    pending: Option<Pending>,
    insert_vals: Vec<String>,
    json_target: Option<JsonTarget>,
    palette: Vec<PaletteAction>,
    palette_search: bool,
    toast_id: u64,
    export_dialect: Dialect,
    form_id: String,
}

impl Worker {
    pub fn new(w: Weak<App>, tx: UnboundedSender<Cmd>, store: Store) -> Self {
        let connections = store.load_connections();
        let settings = store.load_settings();
        let history = store.load_history();
        let saved = store.load_saved_queries();
        Self {
            w,
            tx,
            store,
            connections,
            settings,
            history,
            saved,
            conn: None,
            env: Environment::Local,
            tabs: Vec::new(),
            active: None,
            closed: Vec::new(),
            query_counter: 0,
            tree_filter: String::new(),
            collapsed: HashSet::new(),
            tree: Vec::new(),
            activity: Vec::new(),
            pending: None,
            insert_vals: Vec::new(),
            json_target: None,
            palette: Vec::new(),
            palette_search: false,
            toast_id: 0,
            export_dialect: Dialect::Pg,
            form_id: String::new(),
        }
    }

    /// Push everything persisted (settings, connections, history) to a freshly opened window.
    pub fn init(&mut self) {
        self.push_settings();
        self.push_saved_and_history();
        self.push_connections();
        let last = self.connections.iter().find(|c| c.id == self.settings.last_connection_id).or_else(|| self.connections.iter().max_by_key(|c| c.last_used)).map(|c| c.id.clone());
        match last {
            Some(id) => self.select_conn(&id),
            None => self.new_conn(),
        }
    }

    fn persist_settings(&self) {
        let _ = self.store.save_settings(&self.settings);
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast_id += 1;
        let (id, msg) = (self.toast_id, msg.into());
        ui(&self.w, move |st| st.set_toast(msg.into()));
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let _ = tx.send(Cmd::ClearToast(id));
        });
    }

    fn log_activity(&mut self, ms: Option<f64>, what: &str) {
        let line = match ms {
            Some(ms) => format!("{}  {:.1} ms  {}", hms(), ms, one_line(what)),
            None => format!("{}  {}", hms(), one_line(what)),
        };
        self.activity.insert(0, line);
        self.activity.truncate(500);
        let a = self.activity.clone();
        ui(&self.w, move |st| st.set_activity(strs(a)));
    }

    // ---------------------------------------------------------------------------------------
    // Settings
    // ---------------------------------------------------------------------------------------

    fn push_settings(&self) {
        let s = self.settings.clone();
        let idx = PAGE_SIZES.iter().position(|p| *p == s.default_page_size).unwrap_or(2) as i32;
        let w = self.w.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = w.upgrade() {
                let dark = s.theme == ThemePref::Dark;
                let t = app.global::<crate::Theme>();
                t.set_dark(dark);
                t.set_row_h(if s.compact_density { 24.0 } else { 30.0 });
                app.global::<crate::Palette>()
                    .set_color_scheme(if dark { slint::language::ColorScheme::Dark } else { slint::language::ColorScheme::Light });
                let st = app.global::<AppState>();
                st.set_set_dark(dark);
                st.set_set_compact(s.compact_density);
                st.set_set_page_size_index(idx);
                st.set_set_confirm(s.confirm_destructive);
                st.set_set_autocomplete(s.autocomplete);
                st.set_set_font_size(s.editor_font_size as i32);
                st.set_inspector_open(s.inspector_open);
            }
        });
    }

    fn protected(&self) -> bool {
        self.settings.confirm_destructive && self.env.requires_destructive_confirmation()
    }

    // ---------------------------------------------------------------------------------------
    // Connection manager
    // ---------------------------------------------------------------------------------------

    fn push_connections(&self) {
        let sel = self.form_id_hint();
        let mut list = self.connections.clone();
        list.sort_by(|a, b| b.last_used.cmp(&a.last_used).then(a.display_name().to_lowercase().cmp(&b.display_name().to_lowercase())));
        let items: Vec<(String, String, String, u32, bool)> = list
            .iter()
            .map(|c| {
                let who = if c.username.is_empty() { c.host.clone() } else { format!("{}@{}", c.username, c.host) };
                (c.id.clone(), c.display_name(), format!("{} • {} • {}", c.db_type.label(), c.environment.label(), who), c.environment.color(), c.id == sel)
            })
            .collect();
        ui(&self.w, move |st| {
            let v: Vec<ConnItem> = items
                .into_iter()
                .map(|(id, name, sub, col, active)| ConnItem {
                    id: id.into(),
                    name: name.into(),
                    subtitle: sub.into(),
                    color: slint::Color::from_rgb_u8((col >> 16) as u8, (col >> 8) as u8, col as u8),
                    active,
                })
                .collect();
            st.set_connections(ModelRc::new(VecModel::from(v)));
        });
    }

    /// The id currently shown in the form (kept in `settings.last_connection_id` while browsing).
    fn form_id_hint(&self) -> String {
        self.form_id.clone()
    }
}

// ---------------------------------------------------------------------------------------------
// Connection manager
// ---------------------------------------------------------------------------------------------

impl Worker {
    fn cfg_from_form(&self, f: &ConnForm) -> (ConnectionConfig, String) {
        let db_type = DbType::ALL[(f.type_index.max(0) as usize).min(2)];
        let mut uri = f.uri.trim().to_string();
        let mut password = f.password.to_string();
        if db_type == DbType::Mongo && !uri.is_empty() {
            // A password typed into the URI belongs in the keyring, not in the config file.
            let (clean, pw) = mongo::split_uri_password(&uri);
            if let Some(p) = pw {
                uri = clean;
                if password.is_empty() {
                    password = p;
                }
            }
        }
        let port_text = f.port.trim();
        let port = if port_text.is_empty() { db_type.default_port() } else { port_text.parse().unwrap_or(0) };
        let existing_last_used = self.connections.iter().find(|c| c.id == f.id.as_str()).map_or(0, |c| c.last_used);
        let cfg = ConnectionConfig {
            id: if f.id.is_empty() { config::new_id() } else { f.id.to_string() },
            name: f.name.trim().to_string(),
            db_type,
            host: f.host.trim().to_string(),
            port,
            database: f.database.trim().to_string(),
            username: f.user.trim().to_string(),
            environment: Environment::ALL[(f.env_index.max(0) as usize).min(3)],
            ssl: SslMode::ALL[(f.ssl_index.max(0) as usize).min(3)],
            mongo_uri: uri,
            remember_password: f.remember,
            last_used: existing_last_used,
        };
        (cfg, password)
    }

    fn show_form(&mut self, c: &ConnectionConfig, password: String, is_new: bool, info: String, error: String) {
        self.form_id = c.id.clone();
        let c = c.clone();
        ui(&self.w, move |st| {
            st.set_form(ConnForm {
                id: c.id.into(),
                name: c.name.into(),
                type_index: c.db_type.index() as i32,
                host: c.host.into(),
                port: c.port.to_string().into(),
                database: c.database.into(),
                user: c.username.into(),
                password: password.into(),
                env_index: c.environment.index() as i32,
                ssl_index: c.ssl.index() as i32,
                uri: c.mongo_uri.into(),
                remember: c.remember_password,
            });
            st.set_form_is_new(is_new);
            st.set_form_info(info.into());
            st.set_form_error(error.into());
            st.set_form_busy(false);
        });
        self.push_connections();
    }

    pub fn new_conn(&mut self) {
        let c = ConnectionConfig::new_blank();
        self.show_form(&c, String::new(), true, String::new(), String::new());
    }

    fn select_conn(&mut self, id: &str) {
        let Some(c) = self.connections.iter().find(|c| c.id == id).cloned() else { return self.new_conn() };
        let pw = if c.remember_password { secrets::get(&c).unwrap_or_default() } else { String::new() };
        self.show_form(&c, pw, false, String::new(), String::new());
    }

    fn form_error(&self, msg: impl Into<String>) {
        let m = msg.into();
        ui(&self.w, move |st| {
            st.set_form_error(m.into());
            st.set_form_info("".into());
            st.set_form_busy(false);
            st.set_busy(false);
        });
    }

    /// Validate + persist the form. Returns the saved config and the password typed for this session.
    fn save_conn(&mut self, f: &ConnForm, quiet: bool) -> Option<(ConnectionConfig, String)> {
        let (cfg, pw) = self.cfg_from_form(f);
        if let Some(msg) = cfg.validate() {
            self.form_error(msg);
            return None;
        }
        match self.connections.iter_mut().find(|c| c.id == cfg.id) {
            Some(slot) => *slot = cfg.clone(),
            None => self.connections.push(cfg.clone()),
        }
        if let Err(e) = self.store.save_connections(&self.connections) {
            self.form_error(format!("Could not save connections: {e}"));
            return None;
        }
        let mut info = if quiet { String::new() } else { "Saved.".to_string() };
        if cfg.remember_password && !pw.is_empty() {
            if let Err(e) = secrets::set(&cfg, &pw) {
                info = format!("Saved, but the password could not be stored ({e}). You'll be asked for it next time.");
            }
        } else if !cfg.remember_password || pw.is_empty() {
            secrets::delete(&cfg);
        }
        self.show_form(&cfg, pw.clone(), false, info, String::new());
        Some((cfg, pw))
    }

    async fn test_conn(&mut self, f: &ConnForm) {
        let (cfg, pw) = self.cfg_from_form(f);
        if let Some(msg) = cfg.validate() {
            return self.form_error(msg);
        }
        ui(&self.w, |st| {
            st.set_form_busy(true);
            st.set_form_error("".into());
            st.set_form_info("Connecting…".into());
        });
        match dboard_core::driver::test_connection(cfg, &pw).await {
            Ok(v) => ui(&self.w, move |st| {
                st.set_form_busy(false);
                st.set_form_info(format!("Connection successful · server {v}").into());
            }),
            Err(e) => self.form_error(e.to_string()),
        }
    }

    async fn connect_conn(&mut self, f: &ConnForm) {
        let Some((cfg, pw)) = self.save_conn(f, true) else { return };
        ui(&self.w, |st| {
            st.set_busy(true);
            st.set_form_error("".into());
        });
        match Conn::connect(cfg.clone(), &pw).await {
            Ok(conn) => self.on_connected(cfg, conn),
            Err(e) => self.form_error(e.to_string()),
        }
    }

    fn on_connected(&mut self, cfg: ConnectionConfig, conn: Conn) {
        self.env = cfg.environment;
        self.tabs.clear();
        self.active = None;
        self.closed.clear();
        self.tree_filter.clear();
        self.collapsed.clear();
        self.activity.clear();
        if let Some(c) = self.connections.iter_mut().find(|c| c.id == cfg.id) {
            c.last_used = config::now_secs();
        }
        let _ = self.store.save_connections(&self.connections);
        self.settings.last_connection_id = cfg.id.clone();
        self.persist_settings();
        let who = if cfg.db_type == DbType::Mongo && !cfg.mongo_uri.is_empty() {
            cfg.mongo_uri.clone()
        } else {
            format!("{}@{}:{}{}", cfg.username, cfg.host, cfg.port, if cfg.database.is_empty() { String::new() } else { format!(" / {}", cfg.database) })
        };
        let status = format!("{} {} · {who}", cfg.db_type.label(), conn.server_version);
        let (name, env, protected, db) = (cfg.display_name(), cfg.environment, self.protected(), cfg.db_type.index() as i32);
        self.conn = Some(conn);
        self.rebuild_tree();
        self.push_saved_and_history();
        self.show_active();
        self.push_inspector();
        ui(&self.w, move |st| {
            st.set_conn_name(name.into());
            st.set_env_label(env.label().into());
            let c = env.color();
            st.set_env_color(slint::Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8));
            st.set_status(status.into());
            st.set_db_type(db);
            st.set_is_protected(protected);
            st.set_busy(false);
            st.set_connected(true);
        });
    }

    fn disconnect(&mut self) {
        self.conn = None;
        self.tabs.clear();
        self.active = None;
        self.pending = None;
        self.push_connections();
        ui(&self.w, |st| {
            st.set_connected(false);
            st.set_busy(false);
            st.set_confirm_open(false);
            st.set_insert_open(false);
            st.set_json_open(false);
            st.set_palette_open(false);
            st.set_export_open(false);
        });
    }

    fn delete_conn(&mut self, id: &str) {
        if let Some(c) = self.connections.iter().find(|c| c.id == id).cloned() {
            secrets::delete(&c);
        }
        self.connections.retain(|c| c.id != id);
        let _ = self.store.save_connections(&self.connections);
        if self.settings.last_connection_id == id {
            self.settings.last_connection_id.clear();
            self.persist_settings();
        }
        match self.connections.first().map(|c| c.id.clone()) {
            Some(next) => self.select_conn(&next),
            None => self.new_conn(),
        }
    }

    fn duplicate_conn(&mut self, id: &str) {
        let Some(orig) = self.connections.iter().find(|c| c.id == id).cloned() else { return };
        let mut copy = orig.clone();
        copy.id = config::new_id();
        copy.name = format!("{} (copy)", orig.display_name());
        copy.last_used = 0;
        let pw = secrets::get(&orig).unwrap_or_default();
        if copy.remember_password && !pw.is_empty() {
            let _ = secrets::set(&copy, &pw);
        }
        self.connections.push(copy.clone());
        let _ = self.store.save_connections(&self.connections);
        self.show_form(&copy, pw, false, "Duplicated. Edit the copy and Save.".into(), String::new());
    }
}

// ---------------------------------------------------------------------------------------------
// Sidebar tree
// ---------------------------------------------------------------------------------------------

impl Worker {
    fn rebuild_tree(&mut self) {
        let Some(conn) = &self.conn else { return };
        let filter = self.tree_filter.to_lowercase();
        let filtering = !filter.is_empty();
        let matches = |n: &str| !filtering || n.to_lowercase().contains(&filter);
        let mut entries: Vec<(TreeEntry, String, i32, bool)> = Vec::new(); // (entry, label, level, expanded)

        let mut schemas: Vec<String> = conn.metadata.tables.iter().map(|t| t.schema.clone()).chain(conn.metadata.objects.iter().map(|o| o.schema.clone())).collect();
        schemas.sort();
        schemas.dedup();
        for s in schemas {
            let tables: Vec<&Table> = conn.metadata.tables.iter().filter(|t| t.schema == s && matches(&t.name)).collect();
            let objects: Vec<&DbObject> = conn.metadata.objects.iter().filter(|o| o.schema == s && matches(&o.name)).collect();
            if tables.is_empty() && objects.is_empty() {
                continue;
            }
            let skey = format!("s:{s}");
            let open = filtering || !self.collapsed.contains(&skey);
            let label = if s.is_empty() { "database".to_string() } else { s.clone() };
            entries.push((TreeEntry { kind: 0, schema: s.clone(), name: String::new(), key: skey }, label, 0, open));
            if !open {
                continue;
            }
            let sections: [(&str, i32, Vec<(String, i32)>); 5] = [
                ("Tables", 1, tables.iter().filter(|t| t.kind == TableKind::Table).map(|t| (t.name.clone(), 2)).collect()),
                ("Views", 1, tables.iter().filter(|t| matches!(t.kind, TableKind::View | TableKind::MaterializedView)).map(|t| (t.name.clone(), if t.kind == TableKind::View { 3 } else { 4 })).collect()),
                ("Collections", 1, tables.iter().filter(|t| t.kind == TableKind::Collection).map(|t| (t.name.clone(), 5)).collect()),
                ("Routines", 1, objects.iter().filter(|o| o.kind != ObjectKind::Sequence).map(|o| (o.name.clone(), if o.kind == ObjectKind::Procedure { 7 } else { 6 })).collect()),
                ("Sequences", 1, objects.iter().filter(|o| o.kind == ObjectKind::Sequence).map(|o| (o.name.clone(), 8)).collect()),
            ];
            for (title, _, items) in sections {
                if items.is_empty() {
                    continue;
                }
                let key = format!("s:{s}/{title}");
                let sopen = filtering || !self.collapsed.contains(&key);
                entries.push((TreeEntry { kind: 1, schema: s.clone(), name: title.into(), key: key.clone() }, format!("{title} ({})", items.len()), 1, sopen));
                if sopen {
                    for (name, kind) in items {
                        entries.push((TreeEntry { kind, schema: s.clone(), name: name.clone(), key: format!("{key}/{name}") }, name, 2, false));
                    }
                }
            }
        }
        self.tree = entries.iter().map(|e| e.0.clone()).collect();
        let items: Vec<(String, i32, i32, bool, String)> = entries.into_iter().map(|(e, label, level, open)| (label, level, e.kind, open, e.key)).collect();
        ui(&self.w, move |st| {
            let v: Vec<TreeItem> = items
                .into_iter()
                .map(|(label, level, kind, expanded, key)| TreeItem { label: label.into(), level, kind, expanded, key: key.into() })
                .collect();
            st.set_tree(ModelRc::new(VecModel::from(v)));
        });
    }

    fn find_object(&self, schema: &str, name: &str, kind: i32) -> Option<DbObject> {
        let want = |o: &DbObject| match kind {
            6 => o.kind == ObjectKind::Function,
            7 => o.kind == ObjectKind::Procedure,
            _ => o.kind == ObjectKind::Sequence,
        };
        self.conn.as_ref()?.metadata.objects.iter().find(|o| o.schema == schema && o.name == name && want(o)).cloned()
    }
}

// ---------------------------------------------------------------------------------------------
// Tabs & data
// ---------------------------------------------------------------------------------------------

impl Worker {
    fn dialect(&self) -> Dialect {
        match self.conn.as_ref().map(|c| c.db_type()) {
            Some(DbType::MySql) => Dialect::My,
            _ => Dialect::Pg,
        }
    }

    fn is_mongo(&self) -> bool {
        self.conn.as_ref().is_some_and(|c| c.db_type() == DbType::Mongo)
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.active.and_then(|i| self.tabs.get(i))
    }

    fn active_mut(&mut self) -> Option<&mut Tab> {
        let i = self.active?;
        self.tabs.get_mut(i)
    }

    fn push_tabs(&self) {
        let tabs: Vec<(String, i32, bool, bool)> =
            self.tabs.iter().enumerate().map(|(i, t)| (t.title.clone(), t.kind as i32, t.pinned, Some(i) == self.active)).collect();
        ui(&self.w, move |st| {
            let v: Vec<TabInfo> = tabs.into_iter().map(|(title, kind, pinned, active)| TabInfo { title: title.into(), kind, pinned, active }).collect();
            st.set_tabs(ModelRc::new(VecModel::from(v)));
        });
    }

    /// Push the whole active tab (or the empty state) to the UI.
    fn show_active(&self) {
        self.push_tabs();
        let tab = self.active_tab().cloned();
        ui(&self.w, move |st| match tab {
            None => {
                st.set_tab_kind(-1);
                st.set_cols(ModelRc::new(VecModel::from(Vec::<ColInfo>::new())));
                st.set_rows(grid_model(Vec::new()));
                st.set_banner("".into());
                st.set_page_info("".into());
                st.set_timing("".into());
                st.set_table_title("".into());
                st.set_ddl_text("".into());
                st.set_selected_row(-1);
            }
            Some(t) => {
                let cols: Vec<ColInfo> = t
                    .cols
                    .iter()
                    .map(|c| ColInfo { name: c.name.clone().into(), type_name: c.type_name.clone().into(), pk: c.pk, fk: c.fk.clone().into(), is_bool: c.is_bool, is_json: c.is_json })
                    .collect();
                st.set_tab_kind(t.kind as i32);
                st.set_cols(ModelRc::new(VecModel::from(cols)));
                st.set_col_widths(ModelRc::new(VecModel::from(t.widths.clone())));
                st.set_grid_width(t.widths.iter().sum());
                st.set_rows(grid_model(t.rows.iter().map(|r| r.iter().map(|c| (c.clone(), 0)).collect()).collect()));
                st.set_row_offset(t.page.offset as i32);
                st.set_selected_row(-1);
                st.set_table_title(if t.schema.is_empty() { t.name.clone() } else { format!("{}.{}", t.schema, t.name) }.into());
                st.set_page_info(t.page_info.into());
                st.set_timing(t.timing.into());
                st.set_sort_column(t.page.sort_column.clone().unwrap_or_default().into());
                st.set_sort_asc(t.page.sort_ascending);
                st.set_editable(t.editable);
                st.set_banner(t.banner.into());
                st.set_banner_error(t.banner_err);
                st.set_filter_text(t.filter_text.into());
                st.set_query_text(t.query_text.into());
                st.set_ddl_text(t.ddl.into());
                st.set_page_size_index(PAGE_SIZES.iter().position(|p| *p == t.page.limit).unwrap_or(2) as i32);
                st.set_suggestions(strs(Vec::new()));
            }
        });
    }

    /// Re-push only the row data, optionally flashing one cell (1 saving, 2 saved, 3 error).
    fn push_rows(&self, flash: Option<(usize, usize, i32)>) {
        let Some(t) = self.active_tab() else { return };
        let grid: Vec<Vec<(Cell, i32)>> = t
            .rows
            .iter()
            .enumerate()
            .map(|(r, row)| row.iter().enumerate().map(|(c, v)| (v.clone(), flash.filter(|f| f.0 == r && f.1 == c).map_or(0, |f| f.2))).collect())
            .collect();
        ui(&self.w, move |st| st.set_rows(grid_model(grid)));
    }

    fn set_banner(&mut self, msg: &str, is_err: bool) {
        if let Some(t) = self.active_mut() {
            t.banner = msg.to_string();
            t.banner_err = is_err;
        }
        let m = msg.to_string();
        ui(&self.w, move |st| {
            st.set_banner(m.into());
            st.set_banner_error(is_err);
        });
    }

    fn add_tab(&mut self, tab: Tab) {
        self.tabs.push(tab);
        self.active = Some(self.tabs.len() - 1);
    }

    fn default_page_size(&self) -> i64 {
        self.settings.default_page_size
    }

    async fn open_table(&mut self, schema: &str, name: &str) {
        if let Some(i) = self.tabs.iter().position(|t| t.kind == Kind::Table && t.schema == schema && t.name == name) {
            self.active = Some(i);
            return self.show_active();
        }
        let mut t = Tab::new(Kind::Table, name, self.default_page_size());
        t.schema = schema.into();
        t.name = name.into();
        self.add_tab(t);
        self.load_active().await;
    }

    async fn open_structure(&mut self, schema: &str, name: &str) {
        if let Some(i) = self.tabs.iter().position(|t| t.kind == Kind::Structure && t.schema == schema && t.name == name) {
            self.active = Some(i);
        } else {
            let mut t = Tab::new(Kind::Structure, format!("Structure: {name}"), self.default_page_size());
            t.schema = schema.into();
            t.name = name.into();
            self.add_tab(t);
        }
        self.load_active().await;
    }

    async fn open_routine(&mut self, o: DbObject) {
        if let Some(i) = self.tabs.iter().position(|t| t.kind == Kind::Routine && t.obj.as_ref() == Some(&o)) {
            self.active = Some(i);
        } else {
            let mut t = Tab::new(Kind::Routine, o.name.clone(), self.default_page_size());
            t.schema = o.schema.clone();
            t.name = o.name.clone();
            t.obj = Some(o);
            self.add_tab(t);
        }
        self.load_active().await;
    }

    fn select_star(&self, schema: &str, name: &str) -> String {
        if self.is_mongo() {
            return format!("db.{name}.find({{}}).limit(100)");
        }
        let d = self.dialect();
        let q = if schema.is_empty() { d.quote(name) } else { format!("{}.{}", d.quote(schema), d.quote(name)) };
        format!("SELECT * FROM {q} LIMIT 100;")
    }

    fn new_query_tab(&mut self, text: String) {
        self.query_counter += 1;
        let mut t = Tab::new(Kind::Query, format!("Query {}", self.query_counter), self.default_page_size());
        t.query_text = text;
        self.add_tab(t);
        self.show_active();
        let hint = if self.is_mongo() {
            "Ctrl+Enter to run. Syntax: db.collection.find({…}).sort({…}).limit(n)  or  db.collection.aggregate([…])"
        } else {
            "Ctrl+Enter to run."
        };
        ui(&self.w, move |st| st.set_query_hint(hint.into()));
    }

    /// (Re)load whatever the active tab shows, then repaint.
    async fn load_active(&mut self) {
        let Some(i) = self.active else { return };
        let tab = self.tabs[i].clone();
        match tab.kind {
            Kind::Query => {}
            Kind::Table => self.load_table(i, tab).await,
            Kind::Structure => self.load_structure(i, tab).await,
            Kind::Routine => {
                let text = match (&mut self.conn, &tab.obj) {
                    (Some(c), Some(o)) => c.object_def(o).await.unwrap_or_else(|e| format!("-- {e}")),
                    _ => String::new(),
                };
                self.tabs[i].ddl = text;
            }
        }
        self.show_active();
    }

    async fn load_table(&mut self, i: usize, tab: Tab) {
        let Some(conn) = self.conn.as_mut() else { return };
        let res = conn.fetch_page(&tab.schema, &tab.name, &tab.page).await;
        let table = conn.table(&tab.schema, &tab.name).cloned();
        match res {
            Ok(r) => {
                let meta: Vec<ColMeta> = r
                    .columns
                    .iter()
                    .map(|n| table.as_ref().and_then(|t| t.column(n)).map(ColMeta::from_column).unwrap_or_else(|| ColMeta::plain(n)))
                    .collect();
                let editable = table.as_ref().is_some_and(|t| t.is_editable());
                let is_view = table.as_ref().is_some_and(|t| matches!(t.kind, TableKind::View | TableKind::MaterializedView));
                let banner = if is_view {
                    "View: read-only."
                } else if !editable {
                    "No primary key: inline editing is disabled to protect your data."
                } else {
                    ""
                };
                let from = tab.page.offset + 1;
                let to = tab.page.offset + r.rows.len() as i64;
                let info = match (r.rows.is_empty(), r.total_estimate) {
                    (true, _) => "No rows".to_string(),
                    (_, Some(e)) if e > 0 => format!("Rows {from}–{to} of ~{e}"),
                    _ => format!("Rows {from}–{to}"),
                };
                let t = &mut self.tabs[i];
                t.widths = if t.widths.len() == meta.len() && !t.widths.is_empty() { t.widths.clone() } else { auto_widths(&meta, &r.rows) };
                t.cols = meta;
                t.rows = r.rows;
                t.editable = editable;
                t.banner = banner.into();
                t.banner_err = false;
                t.page_info = info;
                t.timing = format!("{:.1} ms", r.duration_ms);
                self.log_activity(Some(r.duration_ms), &format!("SELECT {}.{}", tab.schema, tab.name));
            }
            Err(e) => {
                let t = &mut self.tabs[i];
                t.banner = e.to_string();
                t.banner_err = true;
                self.log_activity(None, &format!("ERROR {e}"));
            }
        }
    }

    async fn load_structure(&mut self, i: usize, tab: Tab) {
        let Some(conn) = self.conn.as_mut() else { return };
        let Some(table) = conn.table(&tab.schema, &tab.name).cloned() else { return };
        let ddl = conn.ddl(&tab.schema, &tab.name).await.unwrap_or_else(|e| format!("-- {e}"));
        let cols: Vec<ColMeta> = ["Column", "Type", "Nullable", "Default", "Key", "References"].iter().map(|n| ColMeta::plain(n)).collect();
        let rows: Vec<Vec<Cell>> = table
            .columns
            .iter()
            .map(|c| {
                vec![
                    Some(c.name.clone()),
                    Some(c.type_name.clone()),
                    Some(if c.nullable { "YES" } else { "NO" }.into()),
                    c.default.clone(),
                    Some(if c.is_primary_key { "PRIMARY" } else { "" }.into()),
                    c.fk.clone(),
                ]
            })
            .collect();
        let t = &mut self.tabs[i];
        t.widths = auto_widths(&cols, &rows);
        t.cols = cols;
        t.page_info = format!(
            "{} column(s){}{}",
            rows.len(),
            table.estimated_rows.map(|n| format!(" · ~{n} rows")).unwrap_or_default(),
            table.size_bytes.map(|b| format!(" · {:.1} KB", b as f64 / 1024.0)).unwrap_or_default()
        );
        t.rows = rows;
        t.ddl = ddl;
        t.editable = false;
    }

    fn close_tab(&mut self, idx: i32) {
        let i = if idx < 0 { self.active } else { Some(idx as usize) };
        let Some(i) = i.filter(|i| *i < self.tabs.len()) else { return };
        if self.tabs[i].pinned && idx < 0 {
            return;
        }
        let t = self.tabs.remove(i);
        self.closed.push(t);
        if self.closed.len() > 20 {
            self.closed.remove(0);
        }
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(a) if a > i => Some(a - 1),
            Some(a) if a == i => Some(i.min(self.tabs.len() - 1)),
            other => other,
        };
        self.show_active();
    }

    async fn tab_action(&mut self, i: usize, action: &str) {
        if i >= self.tabs.len() {
            return;
        }
        match action {
            "pin" => {
                self.tabs[i].pinned = !self.tabs[i].pinned;
                self.push_tabs();
            }
            "duplicate" => {
                let mut t = self.tabs[i].clone();
                t.pinned = false;
                t.title = format!("{} (copy)", t.title);
                self.tabs.insert(i + 1, t);
                self.active = Some(i + 1);
                self.show_active();
            }
            "close-others" => {
                let keep = self.tabs[i].clone();
                let old = std::mem::take(&mut self.tabs);
                for t in old.into_iter() {
                    if t.pinned {
                        self.tabs.push(t);
                    } else {
                        self.closed.push(t);
                    }
                }
                if !self.tabs.iter().any(|t| t.title == keep.title && t.kind == keep.kind) {
                    self.tabs.push(keep);
                }
                self.active = Some(self.tabs.len() - 1);
                self.show_active();
            }
            _ => {}
        }
    }

    async fn reopen_tab(&mut self) {
        let Some(t) = self.closed.pop() else { return self.toast("No recently closed tabs") };
        self.add_tab(t);
        self.load_active().await;
    }

    async fn tree_click(&mut self, i: usize) {
        let Some(e) = self.tree.get(i).cloned() else { return };
        match e.kind {
            0 | 1 => {
                if !self.collapsed.remove(&e.key) {
                    self.collapsed.insert(e.key);
                }
                self.rebuild_tree();
            }
            2..=5 => self.open_table(&e.schema, &e.name).await,
            k => {
                if let Some(o) = self.find_object(&e.schema, &e.name, k) {
                    self.open_routine(o).await;
                }
            }
        }
    }

    async fn tree_action(&mut self, i: usize, action: &str) {
        let Some(e) = self.tree.get(i).cloned() else { return };
        let is_data = (2..=5).contains(&e.kind);
        match action {
            "open" if is_data => self.open_table(&e.schema, &e.name).await,
            "structure" if is_data => self.open_structure(&e.schema, &e.name).await,
            "structure" | "definition" => {
                if let Some(o) = self.find_object(&e.schema, &e.name, e.kind) {
                    self.open_routine(o).await;
                }
            }
            "query" if is_data => {
                let text = self.select_star(&e.schema, &e.name);
                self.new_query_tab(text);
            }
            "insert" if is_data => {
                self.open_table(&e.schema, &e.name).await;
                self.open_insert();
            }
            "export" if is_data => {
                self.open_table(&e.schema, &e.name).await;
                self.open_export();
            }
            "truncate" if is_data => self.confirm_object(Pending::Truncate(e.schema, e.name), "Truncate"),
            "drop" if is_data => self.confirm_object(Pending::Drop(e.schema, e.name), "Drop"),
            "copy" => {
                let full = if e.schema.is_empty() { e.name } else { format!("{}.{}", e.schema, e.name) };
                self.copy_to_clipboard(&full);
            }
            _ => {}
        }
    }

    fn confirm_object(&mut self, p: Pending, verb: &str) {
        let (schema, name) = match &p {
            Pending::Truncate(s, n) | Pending::Drop(s, n) => (s.clone(), n.clone()),
            _ => return,
        };
        let text = format!("{verb} {}.{}? This cannot be undone.", schema, name);
        self.ask(p, format!("{verb} {name}"), text);
    }

    /// Open the confirmation dialog for `p`. Typed confirmation is required on protected connections.
    fn ask(&mut self, p: Pending, title: String, text: String) {
        let typing = self.protected();
        let env = self.env.label().to_string();
        self.pending = Some(p);
        ui(&self.w, move |st| {
            st.set_confirm_title(if typing { format!("{title} on {env}") } else { title }.into());
            st.set_confirm_text(text.into());
            st.set_confirm_typing(typing);
            st.set_confirm_typed("".into());
            st.set_confirm_open(true);
        });
    }

    fn copy_to_clipboard(&mut self, text: &str) {
        match crate::clipboard::set(text) {
            Ok(()) => self.toast("Copied to clipboard"),
            Err(e) => self.toast(format!("Clipboard unavailable: {e}")),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Editing rows
// ---------------------------------------------------------------------------------------------

impl Worker {
    fn schedule_clear(&self) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let _ = tx.send(Cmd::ClearFlash);
        });
    }

    async fn edit_cell(&mut self, r: usize, c: usize, text: String, null: bool) {
        let Some(tab) = self.active_tab().cloned() else { return };
        if tab.kind != Kind::Table || !tab.editable {
            return;
        }
        let (Some(col), Some(row)) = (tab.cols.get(c).cloned(), tab.rows.get(r).cloned()) else { return };
        let new: Cell = if null { None } else { Some(text) };
        if row.get(c) == Some(&new) {
            return; // unchanged
        }
        if let Some(t) = self.active_mut() {
            t.rows[r][c] = new.clone();
        }
        self.push_rows(Some((r, c, 1)));
        let res = match self.conn.as_mut() {
            Some(conn) => conn.edit_cell(&tab.schema, &tab.name, &row, &col.name, new).await,
            None => return,
        };
        match res {
            Ok(()) => {
                self.push_rows(Some((r, c, 2)));
                self.log_activity(None, &format!("UPDATE {}.{} SET {}", tab.schema, tab.name, col.name));
                self.push_inspector();
            }
            Err(e) => {
                if let Some(t) = self.active_mut() {
                    t.rows[r][c] = row[c].clone();
                }
                self.push_rows(Some((r, c, 3)));
                self.set_banner(&e.to_string(), true);
                self.log_activity(None, &format!("ERROR {e}"));
            }
        }
        self.schedule_clear();
    }

    async fn toggle_bool(&mut self, r: usize, c: usize) {
        let Some(tab) = self.active_tab() else { return };
        let (Some(col), Some(row)) = (tab.cols.get(c), tab.rows.get(r)) else { return };
        let on = matches!(row.get(c).cloned().flatten().as_deref(), Some("true" | "t" | "1"));
        let numeric = col.type_name.contains("tinyint");
        let new = match (on, numeric) {
            (true, true) => "0",
            (false, true) => "1",
            (true, false) => "false",
            (false, false) => "true",
        };
        self.edit_cell(r, c, new.to_string(), false).await;
    }

    fn cell_text(&self, r: usize, c: usize) -> Option<Cell> {
        self.active_tab()?.rows.get(r)?.get(c).cloned()
    }

    fn open_json_cell(&mut self, r: usize, c: usize) {
        let Some(Some(v)) = self.cell_text(r, c).map(|v| v.or(Some(String::new()))) else { return };
        let col = self.active_tab().and_then(|t| t.cols.get(c)).cloned().unwrap_or_default();
        let pretty = serde_json::from_str::<serde_json::Value>(&v).ok().and_then(|j| serde_json::to_string_pretty(&j).ok()).unwrap_or(v);
        self.json_target = Some(JsonTarget::Cell(r, c));
        let title = format!("Edit {} · {}", if col.is_json { "JSON" } else { "text" }, col.name);
        ui(&self.w, move |st| {
            st.set_json_title(title.into());
            st.set_json_text(pretty.into());
            st.set_json_error("".into());
            st.set_json_open(true);
        });
    }

    async fn json_save(&mut self, text: String) {
        let Some(target) = self.json_target.take() else { return };
        let fail = |w: &Weak<App>, msg: String| ui(w, move |st| st.set_json_error(msg.into()));
        match target {
            JsonTarget::Cell(r, c) => {
                let is_json = self.active_tab().and_then(|t| t.cols.get(c)).is_some_and(|c| c.is_json);
                let value = if is_json {
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(v) => serde_json::to_string(&v).unwrap_or(text),
                        Err(e) => {
                            self.json_target = Some(JsonTarget::Cell(r, c));
                            return fail(&self.w, format!("Invalid JSON: {e}"));
                        }
                    }
                } else {
                    text
                };
                ui(&self.w, |st| st.set_json_open(false));
                self.edit_cell(r, c, value, false).await;
            }
            JsonTarget::Doc(row) => {
                let Some(tab) = self.active_tab().cloned() else { return };
                let res = match self.conn.as_mut() {
                    Some(conn) => conn.replace_document(&tab.schema, &tab.name, &row, &text).await,
                    None => return,
                };
                match res {
                    Ok(()) => {
                        ui(&self.w, |st| st.set_json_open(false));
                        self.toast("Document saved");
                        self.log_activity(None, &format!("REPLACE document in {}", tab.name));
                        self.load_active().await;
                    }
                    Err(e) => {
                        self.json_target = Some(JsonTarget::Doc(row));
                        fail(&self.w, e.to_string());
                    }
                }
            }
            JsonTarget::NewDoc => {
                let Some(tab) = self.active_tab().cloned() else { return };
                let res = match self.conn.as_mut() {
                    Some(conn) => conn.insert_document(&tab.schema, &tab.name, &text).await,
                    None => return,
                };
                match res {
                    Ok(()) => {
                        ui(&self.w, |st| st.set_json_open(false));
                        self.toast("Document inserted");
                        self.log_activity(None, &format!("INSERT document into {}", tab.name));
                        self.load_active().await;
                    }
                    Err(e) => {
                        self.json_target = Some(JsonTarget::NewDoc);
                        fail(&self.w, e.to_string());
                    }
                }
            }
        }
    }

    async fn open_doc(&mut self, r: usize) {
        if !self.is_mongo() {
            return;
        }
        let (Some(tab), Some(row)) = (self.active_tab().cloned(), self.active_tab().and_then(|t| t.rows.get(r).cloned())) else { return };
        let res = match self.conn.as_mut() {
            Some(conn) => conn.document_json(&tab.schema, &tab.name, &row).await,
            None => return,
        };
        match res {
            Ok(json) => {
                self.json_target = Some(JsonTarget::Doc(row));
                let title = format!("Edit document · {}", tab.name);
                ui(&self.w, move |st| {
                    st.set_json_title(title.into());
                    st.set_json_text(json.into());
                    st.set_json_error("".into());
                    st.set_json_open(true);
                });
            }
            Err(e) => self.toast(e.to_string()),
        }
    }

    fn open_insert(&mut self) {
        let Some(tab) = self.active_tab().cloned() else { return };
        if tab.kind != Kind::Table {
            return;
        }
        if self.is_mongo() {
            self.json_target = Some(JsonTarget::NewDoc);
            let title = format!("Insert document · {}", tab.name);
            ui(&self.w, move |st| {
                st.set_json_title(title.into());
                st.set_json_text("{\n  \n}".into());
                st.set_json_error("".into());
                st.set_json_open(true);
            });
            return;
        }
        let Some(table) = self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).cloned() else { return };
        if !matches!(table.kind, TableKind::Table) {
            return self.toast("Rows can only be inserted into tables");
        }
        self.insert_vals = vec![String::new(); table.columns.len()];
        let fields: Vec<(String, String, String)> = table
            .columns
            .iter()
            .map(|c| {
                let hint = match (&c.default, c.nullable) {
                    (Some(d), _) => format!("default: {d}"),
                    (None, true) => "NULL if left empty".to_string(),
                    (None, false) => "required".to_string(),
                };
                (c.name.clone(), format!("{}{}", c.type_name, if c.is_primary_key { " · PK" } else { "" }), hint)
            })
            .collect();
        let title = format!("Insert row · {}", table.full_name());
        ui(&self.w, move |st| {
            let v: Vec<FieldItem> = fields.into_iter().map(|(n, t, h)| FieldItem { name: n.into(), type_name: t.into(), hint: h.into() }).collect();
            st.set_insert_fields(ModelRc::new(VecModel::from(v)));
            st.set_insert_title(title.into());
            st.set_insert_error("".into());
            st.set_insert_open(true);
        });
    }

    async fn insert_submit(&mut self) {
        let Some(tab) = self.active_tab().cloned() else { return };
        let Some(table) = self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).cloned() else { return };
        let vals: Vec<(String, String)> = table
            .columns
            .iter()
            .zip(&self.insert_vals)
            .filter(|(_, v)| !v.is_empty())
            .map(|(c, v)| (c.name.clone(), v.clone()))
            .collect();
        let res = match self.conn.as_mut() {
            Some(conn) => conn.insert_row(&tab.schema, &tab.name, &vals).await,
            None => return,
        };
        match res {
            Ok(()) => {
                ui(&self.w, |st| st.set_insert_open(false));
                self.toast("Row inserted");
                self.log_activity(None, &format!("INSERT INTO {}.{}", tab.schema, tab.name));
                self.load_active().await;
            }
            Err(e) => {
                let m = e.to_string();
                ui(&self.w, move |st| st.set_insert_error(m.into()));
            }
        }
    }

    fn ask_delete_row(&mut self, r: usize) {
        let Some(tab) = self.active_tab() else { return };
        if tab.kind != Kind::Table || !tab.editable || r >= tab.rows.len() {
            return;
        }
        let title = format!("Delete row {}", tab.page.offset + r as i64 + 1);
        let text = format!("Delete the selected row from {}.{}? This cannot be undone.", tab.schema, tab.name);
        self.ask(Pending::DeleteRow(r), title, text);
    }

    async fn confirm_run(&mut self) {
        ui(&self.w, |st| st.set_confirm_open(false));
        match self.pending.take() {
            Some(Pending::Query(sql)) => self.run_sql(sql).await,
            Some(Pending::Explain(sql, a)) => self.run_explain(sql, a).await,
            Some(Pending::DeleteRow(r)) => {
                let (Some(tab), Some(row)) = (self.active_tab().cloned(), self.active_tab().and_then(|t| t.rows.get(r).cloned())) else { return };
                let res = match self.conn.as_mut() {
                    Some(conn) => conn.delete_row(&tab.schema, &tab.name, &row).await,
                    None => return,
                };
                match res {
                    Ok(()) => {
                        self.toast("Row deleted");
                        self.log_activity(None, &format!("DELETE FROM {}.{}", tab.schema, tab.name));
                        self.load_active().await;
                    }
                    Err(e) => self.set_banner(&e.to_string(), true),
                }
            }
            Some(Pending::Truncate(s, n)) => {
                let res = match self.conn.as_mut() {
                    Some(conn) => conn.truncate(&s, &n).await,
                    None => return,
                };
                match res {
                    Ok(()) => {
                        self.toast(format!("Truncated {n}"));
                        self.log_activity(None, &format!("TRUNCATE {s}.{n}"));
                        if self.active_tab().is_some_and(|t| t.kind == Kind::Table && t.schema == s && t.name == n) {
                            self.load_active().await;
                        }
                    }
                    Err(e) => self.toast(e.to_string()),
                }
            }
            Some(Pending::Drop(s, n)) => {
                let res = match self.conn.as_mut() {
                    Some(conn) => conn.drop_table(&s, &n).await,
                    None => return,
                };
                match res {
                    Ok(()) => {
                        self.tabs.retain(|t| !(matches!(t.kind, Kind::Table | Kind::Structure) && t.schema == s && t.name == n));
                        self.active = if self.tabs.is_empty() { None } else { Some(self.tabs.len() - 1) };
                        self.rebuild_tree();
                        self.show_active();
                        self.toast(format!("Dropped {n}"));
                        self.log_activity(None, &format!("DROP {s}.{n}"));
                    }
                    Err(e) => self.toast(e.to_string()),
                }
            }
            None => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Query editor, history, saved queries
// ---------------------------------------------------------------------------------------------

fn is_read_only(sql: &str) -> bool {
    let first = sql.trim_start().split_whitespace().next().unwrap_or("").to_uppercase();
    matches!(first.as_str(), "SELECT" | "WITH" | "VALUES" | "SHOW" | "EXPLAIN" | "TABLE" | "DESCRIBE" | "DESC")
}

fn changes_schema(sql: &str) -> bool {
    let first = sql.trim_start().split_whitespace().next().unwrap_or("").to_uppercase();
    matches!(first.as_str(), "CREATE" | "ALTER" | "DROP" | "RENAME")
}

impl Worker {
    fn push_saved_and_history(&self) {
        let saved: Vec<(String, String, String, String)> = self.saved.iter().map(|q| (q.id.clone(), q.name.clone(), q.folder.clone(), one_line(&q.sql))).collect();
        let hist: Vec<(String, String, bool)> = self
            .history
            .iter()
            .take(100)
            .map(|h| (one_line(&h.sql), format!("{} · {:.1} ms{}", when(h.at), h.duration_ms, if h.ok { "" } else { " · failed" }), h.ok))
            .collect();
        ui(&self.w, move |st| {
            st.set_saved(ModelRc::new(VecModel::from(
                saved.into_iter().map(|(id, name, folder, sql)| SavedItem { id: id.into(), name: name.into(), folder: folder.into(), sql: sql.into() }).collect::<Vec<_>>(),
            )));
            st.set_history(ModelRc::new(VecModel::from(
                hist.into_iter().map(|(sql, meta, ok)| HistoryItem { sql: sql.into(), meta: meta.into(), ok }).collect::<Vec<_>>(),
            )));
        });
    }

    fn record_history(&mut self, sql: &str, ms: f64, ok: bool) {
        let cid = self.conn.as_ref().map(|c| c.config.id.clone()).unwrap_or_default();
        self.history.retain(|h| !(h.sql == sql && h.connection_id == cid));
        self.history.insert(0, HistoryEntry { sql: sql.to_string(), connection_id: cid, at: config::now_secs(), duration_ms: ms, ok });
        self.history.truncate(500);
        let _ = self.store.save_history(&self.history);
        self.push_saved_and_history();
    }

    fn query_edited(&mut self, text: String) {
        let mongo = self.is_mongo();
        let enabled = self.settings.autocomplete;
        if let Some(t) = self.active_mut() {
            t.query_text = text.clone();
        }
        let tables = self.conn.as_ref().map(|c| c.metadata.tables.clone()).unwrap_or_default();
        let s = if enabled { suggest::suggest(&text, &tables, mongo) } else { Vec::new() };
        ui(&self.w, move |st| st.set_suggestions(strs(s)));
    }

    fn set_query_text(&mut self, text: String) {
        if let Some(t) = self.active_mut() {
            t.query_text = text.clone();
        }
        ui(&self.w, move |st| {
            st.set_query_text(text.into());
            st.set_suggestions(strs(Vec::new()));
        });
    }

    async fn run_query(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        if let Some(t) = self.active_mut() {
            t.query_text = text.clone();
        }
        if !self.is_mongo() && self.protected() && safety::is_destructive(&text) {
            let preview = one_line(&text);
            return self.ask(Pending::Query(text), "Destructive statement".into(), preview);
        }
        self.run_sql(text).await;
    }

    async fn run_sql(&mut self, sql: String) {
        let Some(i) = self.active else { return };
        let res = match self.conn.as_mut() {
            Some(c) => c.execute_query(&sql).await,
            None => return,
        };
        match res {
            Ok(r) => {
                let total = r.rows.len();
                let mut rows = r.rows;
                rows.truncate(RESULT_CAP);
                let cols: Vec<ColMeta> = r.columns.iter().map(|n| ColMeta::plain(n)).collect();
                let t = &mut self.tabs[i];
                t.widths = auto_widths(&cols, &rows);
                t.cols = cols;
                t.rows = rows;
                t.editable = false;
                t.banner = String::new();
                t.banner_err = false;
                t.page.offset = 0;
                t.timing = format!("{:.1} ms", r.duration_ms);
                t.page_info = if total > RESULT_CAP { format!("{total} rows (showing the first {RESULT_CAP})") } else { format!("{total} row(s)") };
                self.record_history(&sql, r.duration_ms, true);
                self.log_activity(Some(r.duration_ms), &sql);
                if changes_schema(&sql) {
                    if let Some(c) = self.conn.as_mut() {
                        let _ = c.refresh_metadata().await;
                    }
                    self.rebuild_tree();
                }
            }
            Err(e) => {
                let t = &mut self.tabs[i];
                t.banner = e.to_string();
                t.banner_err = true;
                self.record_history(&sql, 0.0, false);
                self.log_activity(None, &format!("ERROR {e}"));
            }
        }
        self.show_active();
    }

    async fn explain_query(&mut self, text: String, analyze: bool) {
        if text.trim().is_empty() {
            return;
        }
        if let Some(t) = self.active_mut() {
            t.query_text = text.clone();
        }
        // EXPLAIN ANALYZE really executes the statement.
        if analyze && self.protected() && !is_read_only(&text) {
            return self.ask(Pending::Explain(text.clone(), analyze), "EXPLAIN ANALYZE runs the statement".into(), one_line(&text));
        }
        self.run_explain(text, analyze).await;
    }

    async fn run_explain(&mut self, sql: String, analyze: bool) {
        let Some(i) = self.active else { return };
        let res = match self.conn.as_mut() {
            Some(c) => c.explain(&sql, analyze).await,
            None => return,
        };
        match res {
            Ok(r) => {
                let cols: Vec<ColMeta> = r.columns.iter().map(|n| ColMeta::plain(n)).collect();
                let t = &mut self.tabs[i];
                t.widths = auto_widths(&cols, &r.rows);
                if let Some(w) = t.widths.first_mut() {
                    *w = w.max(420.0);
                }
                t.cols = cols;
                t.page_info = format!("Execution plan{}", if analyze { " (analyze)" } else { "" });
                t.timing = format!("{:.1} ms", r.duration_ms);
                t.rows = r.rows;
                t.editable = false;
                t.banner = String::new();
                self.log_activity(Some(r.duration_ms), &format!("EXPLAIN {}", sql));
            }
            Err(e) => {
                let t = &mut self.tabs[i];
                t.banner = e.to_string();
                t.banner_err = true;
            }
        }
        self.show_active();
    }

    fn insert_template(&mut self, which: &str) {
        let coll = self
            .active_tab()
            .filter(|t| t.kind == Kind::Table)
            .map(|t| t.name.clone())
            .or_else(|| self.conn.as_ref().and_then(|c| c.metadata.tables.first().map(|t| t.name.clone())))
            .unwrap_or_else(|| "collection".into());
        let cur = self.active_tab().map(|t| t.query_text.clone()).unwrap_or_default();
        let text = match which {
            "find" => format!("db.{coll}.find({{}}).limit(100)"),
            stage => {
                let snippet = match stage {
                    "$match" => r#"{"$match": {}}"#,
                    "$group" => r#"{"$group": {"_id": "$field", "count": {"$sum": 1}}}"#,
                    "$sort" => r#"{"$sort": {"field": -1}}"#,
                    "$project" => r#"{"$project": {"field": 1}}"#,
                    _ => r#"{"$limit": 100}"#,
                };
                let trimmed = cur.trim_end();
                match trimmed.strip_suffix("])") {
                    Some(head) if trimmed.contains("aggregate([") => {
                        let sep = if head.trim_end().ends_with('[') { "" } else { ", " };
                        format!("{}{sep}{snippet}])", head.trim_end())
                    }
                    _ => format!("db.{coll}.aggregate([{snippet}])"),
                }
            }
        };
        self.set_query_text(text);
    }

    fn load_into_query_tab(&mut self, sql: String) {
        if self.active_tab().is_some_and(|t| t.kind == Kind::Query) {
            self.set_query_text(sql);
        } else {
            self.new_query_tab(sql);
        }
        ui(&self.w, |st| st.set_drawer_open(false));
    }

    fn save_query_submit(&mut self, name: String, folder: usize) {
        let sql = self.active_tab().map(|t| t.query_text.clone()).unwrap_or_default();
        if sql.trim().is_empty() {
            return self.toast("Nothing to save");
        }
        self.saved.push(SavedQuery { id: config::new_id(), name, folder: FOLDERS[folder.min(FOLDERS.len() - 1)].to_string(), sql });
        self.saved.sort_by(|a, b| a.folder.cmp(&b.folder).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        let _ = self.store.save_saved_queries(&self.saved);
        self.push_saved_and_history();
        ui(&self.w, |st| st.set_saveq_open(false));
        self.toast("Query saved");
    }
}

// ---------------------------------------------------------------------------------------------
// Palette, export, settings, inspector
// ---------------------------------------------------------------------------------------------

const COMMANDS: [(&str, &str, &str); 9] = [
    ("new-query", "New query tab", "Ctrl+N"),
    ("refresh", "Refresh database metadata", "Ctrl+R"),
    ("undo", "Undo last edit", "Ctrl+Z"),
    ("reopen", "Reopen closed tab", "Ctrl+Shift+T"),
    ("close-tab", "Close active tab", "Ctrl+W"),
    ("inspector", "Toggle inspector panel", "Ctrl+Alt+I"),
    ("export", "Export current results…", ""),
    ("settings", "Open preferences", ""),
    ("disconnect", "Disconnect / switch connection", ""),
];

impl Worker {
    fn build_palette(&mut self, q: &str) {
        let Some(conn) = &self.conn else { return };
        let mut scored: Vec<(i32, PaletteItem, PaletteAction)> = Vec::new();
        let mut add = |score: Option<i32>, title: String, subtitle: String, kind: &str, a: PaletteAction| {
            if let Some(s) = score {
                scored.push((s, PaletteItem { title: title.into(), subtitle: subtitle.into(), kind: kind.into() }, a));
            }
        };
        if !self.palette_search {
            for (id, title, key) in COMMANDS {
                add(suggest::fuzzy(q, title), title.to_string(), key.to_string(), "command", PaletteAction::Command(id));
            }
        }
        for t in &conn.metadata.tables {
            let label = t.full_name();
            let kind = match t.kind {
                TableKind::Table => "table",
                TableKind::View => "view",
                TableKind::MaterializedView => "materialized view",
                TableKind::Collection => "collection",
            };
            add(suggest::fuzzy(q, &label), label.clone(), t.estimated_rows.map(|n| format!("~{n} rows")).unwrap_or_default(), kind, PaletteAction::OpenTable(t.schema.clone(), t.name.clone()));
            if self.palette_search && !q.is_empty() {
                for c in &t.columns {
                    let full = format!("{}.{}", t.name, c.name);
                    add(suggest::fuzzy(q, &full), full, format!("{} · {}", t.full_name(), c.type_name), "column", PaletteAction::Column(t.schema.clone(), t.name.clone()));
                }
            }
        }
        for o in &conn.metadata.objects {
            let label = format!("{}.{}", o.schema, o.name);
            let kind = match o.kind {
                ObjectKind::Function => "function",
                ObjectKind::Procedure => "procedure",
                ObjectKind::Sequence => "sequence",
            };
            add(suggest::fuzzy(q, &label), label, String::new(), kind, PaletteAction::Object(o.clone()));
        }
        if !self.palette_search {
            for (i, s) in self.saved.iter().enumerate() {
                add(suggest::fuzzy(q, &s.name), s.name.clone(), s.folder.clone(), "saved query", PaletteAction::Saved(i));
            }
            for c in &self.connections {
                add(suggest::fuzzy(q, &c.display_name()), format!("Switch to {}", c.display_name()), c.host.clone(), "connection", PaletteAction::Connection(c.id.clone()));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.title.to_string().cmp(&b.1.title.to_string())));
        scored.truncate(50);
        let (items, actions): (Vec<_>, Vec<_>) = scored.into_iter().map(|(_, i, a)| (i, a)).unzip();
        self.palette = actions;
        let parts: Vec<(String, String, String)> = items.into_iter().map(|i| (i.title.to_string(), i.subtitle.to_string(), i.kind.to_string())).collect();
        ui(&self.w, move |st| {
            let v: Vec<PaletteItem> = parts.into_iter().map(|(t, s, k)| PaletteItem { title: t.into(), subtitle: s.into(), kind: k.into() }).collect();
            st.set_palette_items(ModelRc::new(VecModel::from(v)));
            st.set_palette_selected(0);
        });
    }

    fn open_palette(&mut self, search: bool) {
        self.palette_search = search;
        ui(&self.w, move |st| {
            st.set_palette_search_mode(search);
            st.set_palette_query("".into());
            st.set_palette_open(true);
        });
        self.build_palette("");
    }

    async fn palette_run(&mut self, i: usize) {
        let Some(a) = self.palette.get(i) else { return };
        let a = match a {
            PaletteAction::Command(c) => PaletteAction::Command(c),
            PaletteAction::OpenTable(s, n) => PaletteAction::OpenTable(s.clone(), n.clone()),
            PaletteAction::Column(s, n) => PaletteAction::Column(s.clone(), n.clone()),
            PaletteAction::Object(o) => PaletteAction::Object(o.clone()),
            PaletteAction::Saved(i) => PaletteAction::Saved(*i),
            PaletteAction::Connection(c) => PaletteAction::Connection(c.clone()),
        };
        ui(&self.w, |st| st.set_palette_open(false));
        match a {
            PaletteAction::Command(c) => match c {
                "new-query" => self.new_query_tab(String::new()),
                "refresh" => self.refresh().await,
                "undo" => self.undo().await,
                "reopen" => self.reopen_tab().await,
                "close-tab" => self.close_tab(-1),
                "inspector" => self.toggle_inspector(),
                "export" => self.open_export(),
                "settings" => ui(&self.w, |st| st.set_settings_open(true)),
                "disconnect" => self.disconnect(),
                _ => {}
            },
            PaletteAction::OpenTable(s, n) | PaletteAction::Column(s, n) => self.open_table(&s, &n).await,
            PaletteAction::Object(o) => {
                if o.kind == ObjectKind::Sequence || self.conn.is_some() {
                    self.open_routine(o).await;
                }
            }
            PaletteAction::Saved(i) => {
                if let Some(q) = self.saved.get(i).map(|q| q.sql.clone()) {
                    self.load_into_query_tab(q);
                }
            }
            PaletteAction::Connection(id) => self.switch_connection(&id).await,
        }
    }

    async fn switch_connection(&mut self, id: &str) {
        let Some(cfg) = self.connections.iter().find(|c| c.id == id).cloned() else { return };
        let pw = if cfg.remember_password { secrets::get(&cfg).unwrap_or_default() } else { String::new() };
        self.disconnect();
        self.select_conn(id);
        ui(&self.w, |st| st.set_busy(true));
        match Conn::connect(cfg.clone(), &pw).await {
            Ok(conn) => self.on_connected(cfg, conn),
            Err(e) => self.form_error(e.to_string()),
        }
    }

    fn toggle_inspector(&mut self) {
        self.settings.inspector_open = !self.settings.inspector_open;
        self.persist_settings();
        let open = self.settings.inspector_open;
        ui(&self.w, move |st| st.set_inspector_open(open));
    }

    async fn refresh(&mut self) {
        if let Some(c) = self.conn.as_mut() {
            if let Err(e) = c.refresh_metadata().await {
                return self.toast(e.to_string());
            }
        }
        self.rebuild_tree();
        self.load_active().await;
        self.toast("Metadata refreshed");
    }

    async fn undo(&mut self) {
        let res = match self.conn.as_mut() {
            Some(c) => c.undo().await,
            None => return,
        };
        match res {
            Ok(Some(r)) => {
                self.log_activity(None, &format!("UNDO {}.{} SET {}", r.schema, r.table, r.column));
                self.toast(format!("Reverted {}", r.column));
                if self.active_tab().is_some_and(|t| t.kind == Kind::Table && t.schema == r.schema && t.name == r.table) {
                    self.load_active().await;
                }
            }
            Ok(None) => self.toast("Nothing to undo"),
            Err(e) => self.toast(e.to_string()),
        }
        self.push_inspector();
    }

    fn push_inspector(&self) {
        let Some(conn) = &self.conn else { return };
        let log: Vec<String> = conn
            .history
            .entries()
            .iter()
            .rev()
            .map(|r| {
                let show = |v: &Cell| v.as_deref().map(|s| one_line(s).chars().take(24).collect::<String>()).unwrap_or_else(|| "NULL".into());
                format!("{}.{}.{}   {} → {}", r.schema, r.table, r.column, show(&r.old), show(&r.new))
            })
            .collect();
        let n_undo = conn.history.len() as i32;
        let c = &conn.config;
        let indexes: usize = conn.metadata.tables.iter().map(|t| t.indexes.len()).sum();
        let views = conn.metadata.tables.iter().filter(|t| matches!(t.kind, TableKind::View | TableKind::MaterializedView)).count();
        let mut meta = vec![
            format!("Connection: {}", if c.mongo_uri.is_empty() { format!("{}@{}:{}/{}", c.username, c.host, c.port, c.database) } else { c.mongo_uri.clone() }),
            format!("Engine: {} {}", c.db_type.label(), conn.server_version),
            format!("Environment: {}{}", c.environment.label(), if self.protected() { " (destructive statements need confirmation)" } else { "" }),
            format!(
                "Tables: {} · Views: {} · Routines & sequences: {} · Indexes: {}",
                conn.metadata.tables.len() - views,
                views,
                conn.metadata.objects.len(),
                indexes
            ),
        ];
        if let Some(t) = self.active_tab().filter(|t| t.kind == Kind::Table) {
            if let Some(tab) = conn.table(&t.schema, &t.name) {
                meta.push(String::new());
                meta.push(format!("Selected: {}", tab.full_name()));
                meta.push(format!(
                    "{} columns{}{}",
                    tab.columns.len(),
                    tab.estimated_rows.map(|n| format!(" · ~{n} rows")).unwrap_or_default(),
                    tab.size_bytes.map(|b| format!(" · {:.1} KB", b as f64 / 1024.0)).unwrap_or_default()
                ));
                let pk: Vec<&str> = tab.primary_keys().iter().map(|c| c.name.as_str()).collect();
                meta.push(format!("Primary key: {}", if pk.is_empty() { "none".to_string() } else { pk.join(", ") }));
            }
        }
        ui(&self.w, move |st| {
            st.set_edit_log(strs(log));
            st.set_undo_count(n_undo);
            st.set_meta_lines(strs(meta));
        });
    }

    // ---- export ----------------------------------------------------------------------------

    fn open_export(&mut self) {
        let Some(t) = self.active_tab() else { return };
        if t.rows.is_empty() {
            return self.toast("Nothing to export");
        }
        let info = format!("Exports the {} row(s) currently loaded from {}.", t.rows.len(), if t.name.is_empty() { t.title.clone() } else { t.name.clone() });
        self.export_dialect = self.dialect();
        ui(&self.w, move |st| {
            st.set_export_info(info.into());
            st.set_export_open(true);
        });
    }

    fn export_payload(&self, format: usize, headers: bool) -> Option<(String, String)> {
        let t = self.active_tab()?;
        let cols: Vec<ExportCol> = t.cols.iter().map(|c| ExportCol { name: c.name.clone(), type_name: c.type_name.clone() }).collect();
        let d = self.export_dialect;
        let table = if t.name.is_empty() {
            "results".to_string()
        } else if t.schema.is_empty() {
            d.quote(&t.name)
        } else {
            format!("{}.{}", d.quote(&t.schema), d.quote(&t.name))
        };
        let text = export::format(format, &cols, &t.rows, headers, &table, d);
        let base = sanitize(if t.name.is_empty() { &t.title } else { &t.name });
        Some((text, format!("{base}-{}.{}", chrono::Local::now().format("%Y%m%d-%H%M%S"), export::extension(format))))
    }

    fn export_copy(&mut self, format: usize, headers: bool) {
        if let Some((text, _)) = self.export_payload(format, headers) {
            self.copy_to_clipboard(&text);
            ui(&self.w, |st| st.set_export_open(false));
        }
    }

    fn export_save(&mut self, format: usize, headers: bool) {
        let Some((text, name)) = self.export_payload(format, headers) else { return };
        let path = downloads_dir().join(name);
        match std::fs::write(&path, text) {
            Ok(()) => {
                ui(&self.w, |st| st.set_export_open(false));
                self.toast(format!("Saved {}", path.display()));
                self.log_activity(None, &format!("EXPORT {}", path.display()));
            }
            Err(e) => self.toast(format!("Could not save: {e}")),
        }
    }

    // ---- settings --------------------------------------------------------------------------

    fn settings_changed(&mut self, dark: bool, compact: bool, page_idx: usize, confirm: bool, autocomplete: bool, font: i32) {
        self.settings.theme = if dark { ThemePref::Dark } else { ThemePref::Light };
        self.settings.compact_density = compact;
        self.settings.default_page_size = PAGE_SIZES[page_idx.min(PAGE_SIZES.len() - 1)];
        self.settings.confirm_destructive = confirm;
        self.settings.autocomplete = autocomplete;
        self.settings.editor_font_size = font.clamp(9, 24) as u32;
        self.persist_settings();
        self.push_settings();
        let protected = self.protected();
        ui(&self.w, move |st| st.set_is_protected(protected));
    }

    fn clear_credentials(&mut self) {
        for c in &self.connections {
            secrets::delete(c);
        }
        let n = self.connections.len();
        ui(&self.w, move |st| st.set_set_info(format!("Removed saved passwords for {n} connection(s).").into()));
    }
}

// ---------------------------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------------------------

impl Worker {
    pub async fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::NewConn => self.new_conn(),
            Cmd::SelectConn(id) => self.select_conn(&id),
            Cmd::SaveConn(f) => {
                self.save_conn(&f, false);
            }
            Cmd::TestConn(f) => self.test_conn(&f).await,
            Cmd::ConnectConn(f) => self.connect_conn(&f).await,
            Cmd::DeleteConn(id) => self.delete_conn(&id),
            Cmd::DuplicateConn(id) => self.duplicate_conn(&id),

            Cmd::Disconnect => self.disconnect(),
            Cmd::Refresh => self.refresh().await,
            Cmd::Undo => self.undo().await,

            Cmd::FilterTree(f) => {
                self.tree_filter = f;
                self.rebuild_tree();
            }
            Cmd::TreeClick(i) => self.tree_click(i).await,
            Cmd::TreeAction(i, a) => self.tree_action(i, &a).await,

            Cmd::NewQueryTab => self.new_query_tab(String::new()),
            Cmd::ActivateTab(i) => {
                if i < self.tabs.len() {
                    self.active = Some(i);
                    self.show_active();
                }
            }
            Cmd::CloseTab(i) => self.close_tab(i),
            Cmd::TabAction(i, a) => self.tab_action(i, &a).await,
            Cmd::ReopenTab => self.reopen_tab().await,

            Cmd::EditCell(r, c, t, n) => self.edit_cell(r, c, t, n).await,
            Cmd::ToggleBool(r, c) => self.toggle_bool(r, c).await,
            Cmd::OpenJsonCell(r, c) => self.open_json_cell(r, c),
            Cmd::SortBy(c) => {
                if let Some(t) = self.active_mut().filter(|t| t.kind == Kind::Table) {
                    if let Some(name) = t.cols.get(c).map(|c| c.name.clone()) {
                        if t.page.sort_column.as_deref() == Some(&name) {
                            t.page.sort_ascending = !t.page.sort_ascending;
                        } else {
                            t.page.sort_column = Some(name);
                            t.page.sort_ascending = true;
                        }
                        t.page.offset = 0;
                        self.load_active().await;
                    }
                }
            }
            Cmd::ColResized(i, w) => {
                if let Some(t) = self.active_mut() {
                    if let Some(slot) = t.widths.get_mut(i) {
                        *slot = w;
                    }
                    let total: f32 = t.widths.iter().sum();
                    ui(&self.w, move |st| st.set_grid_width(total));
                }
            }
            Cmd::ApplyFilter(f) => {
                if let Some(t) = self.active_mut().filter(|t| t.kind == Kind::Table) {
                    t.filter_text = f.clone();
                    t.page.filter = Some(f).filter(|f| !f.trim().is_empty());
                    t.page.offset = 0;
                    self.load_active().await;
                }
            }
            Cmd::NextPage => {
                if let Some(t) = self.active_mut().filter(|t| t.kind == Kind::Table && t.rows.len() as i64 == t.page.limit) {
                    t.page.offset += t.page.limit;
                    self.load_active().await;
                }
            }
            Cmd::PrevPage => {
                if let Some(t) = self.active_mut().filter(|t| t.kind == Kind::Table && t.page.offset > 0) {
                    t.page.offset = (t.page.offset - t.page.limit).max(0);
                    self.load_active().await;
                }
            }
            Cmd::SetPageSize(i) => {
                if let Some(t) = self.active_mut().filter(|t| t.kind == Kind::Table) {
                    t.page.limit = PAGE_SIZES[i.min(PAGE_SIZES.len() - 1)];
                    t.page.offset = 0;
                    self.load_active().await;
                }
            }
            Cmd::CopyText(t) => self.copy_to_clipboard(&t),
            Cmd::CellCopy(r, c) => {
                let text = self.cell_text(r, c).flatten().unwrap_or_default();
                self.copy_to_clipboard(&text);
            }
            Cmd::DeleteRow(r) => self.ask_delete_row(r),
            Cmd::OpenInsert => self.open_insert(),
            Cmd::OpenDoc(r) => self.open_doc(r).await,

            Cmd::QueryEdited(t) => self.query_edited(t),
            Cmd::ApplySuggestion(s) => {
                let cur = self.active_tab().map(|t| t.query_text.clone()).unwrap_or_default();
                self.set_query_text(suggest::apply(&cur, &s));
            }
            Cmd::RunQuery(t) => self.run_query(t).await,
            Cmd::ExplainQuery(t, a) => self.explain_query(t, a).await,
            Cmd::InsertTemplate(t) => self.insert_template(&t),
            Cmd::OpenSaveQuery => ui(&self.w, |st| {
                st.set_saveq_name("".into());
                st.set_saveq_folder(0);
                st.set_saveq_open(true);
            }),

            Cmd::ConfirmRun => self.confirm_run().await,
            Cmd::ConfirmCancel => {
                self.pending = None;
                ui(&self.w, |st| st.set_confirm_open(false));
            }
            Cmd::InsertFieldEdited(i, v) => {
                if let Some(slot) = self.insert_vals.get_mut(i) {
                    *slot = v;
                }
            }
            Cmd::InsertSubmit => self.insert_submit().await,
            Cmd::InsertCancel => ui(&self.w, |st| st.set_insert_open(false)),
            Cmd::JsonSave(t) => self.json_save(t).await,
            Cmd::JsonFormat(t) => match serde_json::from_str::<serde_json::Value>(&t) {
                Ok(v) => {
                    let pretty = serde_json::to_string_pretty(&v).unwrap_or(t);
                    ui(&self.w, move |st| {
                        st.set_json_text(pretty.into());
                        st.set_json_error("".into());
                    });
                }
                Err(e) => ui(&self.w, move |st| st.set_json_error(format!("Invalid JSON: {e}").into())),
            },
            Cmd::JsonCancel => {
                self.json_target = None;
                ui(&self.w, |st| st.set_json_open(false));
            }
            Cmd::SaveQuerySubmit(n, f) => self.save_query_submit(n, f),
            Cmd::SaveQueryCancel => ui(&self.w, |st| st.set_saveq_open(false)),
            Cmd::LoadHistory(i) => {
                if let Some(h) = self.history.get(i).map(|h| h.sql.clone()) {
                    self.load_into_query_tab(h);
                }
            }
            Cmd::LoadSaved(i) => {
                if let Some(q) = self.saved.get(i).map(|q| q.sql.clone()) {
                    self.load_into_query_tab(q);
                }
            }
            Cmd::DeleteSaved(i) => {
                if i < self.saved.len() {
                    self.saved.remove(i);
                    let _ = self.store.save_saved_queries(&self.saved);
                    self.push_saved_and_history();
                }
            }
            Cmd::ClearHistory => {
                self.history.clear();
                let _ = self.store.save_history(&self.history);
                self.push_saved_and_history();
            }
            Cmd::ClearActivity => {
                self.activity.clear();
                ui(&self.w, |st| st.set_activity(strs(Vec::new())));
            }
            Cmd::OpenPalette(s) => self.open_palette(s),
            Cmd::PaletteChanged(q) => self.build_palette(&q),
            Cmd::PaletteRun(i) => self.palette_run(i).await,
            Cmd::PaletteClose => ui(&self.w, |st| st.set_palette_open(false)),
            Cmd::OpenExport => self.open_export(),
            Cmd::ExportCopy(f, h) => self.export_copy(f, h),
            Cmd::ExportSave(f, h) => self.export_save(f, h),
            Cmd::ExportCancel => ui(&self.w, |st| st.set_export_open(false)),
            Cmd::OpenSettings => ui(&self.w, |st| {
                st.set_set_info("".into());
                st.set_settings_open(true);
            }),
            Cmd::SettingsChanged { dark, compact, page_idx, confirm, autocomplete, font } => self.settings_changed(dark, compact, page_idx, confirm, autocomplete, font),
            Cmd::SettingsClose => ui(&self.w, |st| st.set_settings_open(false)),
            Cmd::ClearCredentials => self.clear_credentials(),

            Cmd::ClearFlash => self.push_rows(None),
            Cmd::ClearToast(id) => {
                if id == self.toast_id {
                    ui(&self.w, |st| st.set_toast("".into()));
                }
            }
        }
    }
}
