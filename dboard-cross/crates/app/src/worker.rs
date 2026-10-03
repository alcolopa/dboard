//! The worker thread: owns the database connection and all persisted state, and talks to the
//! UI thread only through `ui()` (updates) and `Cmd` (requests).

use crate::export::{self, ExportCol};
use crate::suggest;
use crate::{App, AppState, ColInfo, ConnForm, ConnItem, CtxItem, EditItem, FieldItem, GridCell, HistoryItem, MetaRow, PaletteItem, SavedItem, SessionTab, TabInfo, TreeItem, UserRow};
use dboard_core::config::{self, secrets, HistoryEntry, SavedQuery, Settings, Store, Theme as ThemePref, FOLDERS};
use dboard_core::model::*;
use dboard_core::sql::Dialect;
use dboard_core::dump::{DumpOptions, ImportOptions};
use dboard_core::edit::EditKind;
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
    ParseConnUrl(String),
    ConnFilter(String),
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
    UndoEntry(usize),
    InspectorChanged(bool, bool),
    SwitchDatabase(usize),
    SwitchSession(usize),
    CloseSession(i32),
    // sidebar
    FilterTree(String),
    TreeClick(usize),
    TreeAction(usize, String),
    // tabs
    NewQueryTab,
    ActivateTab(usize),
    CycleTab(i32),
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
    FirstPage,
    LastPage,
    DraftSubmit(Vec<String>),
    SetPageSize(usize),
    CopyText(String),
    CopySelection { r0: usize, c0: usize, r1: usize, c1: usize, mode: i32 },
    PasteSelection { r0: usize, c0: usize, r1: usize, c1: usize },
    EditNext(usize, usize, i32),
    OpenEditRow(usize),
    EditRowFieldEdited(usize, String),
    EditRowSetNull(usize, bool),
    EditRowSubmit,
    EditRowCancel,
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
    // users & access
    OpenUsers,
    UserSelect(usize),
    UserCreate { name: String, host: String, password: String, level: i32, admin: bool },
    UserSetLevel(usize, i32),
    UserPassword(usize, String),
    UserDrop(usize),
    UsersClose,
    // export / import
    OpenTransfer(i32),
    XferBrowse,
    XferRun { path: String, a: bool, b: bool },
    XferCancel,
    Ctx(String, usize, usize, f32, f32),
    CtxPick(String, [i32; 4]),
    CtxClose,
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

fn group_digits(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn human_size(bytes: i64) -> String {
    let b = bytes as f64;
    if b >= 1_073_741_824.0 {
        format!("{:.1} GB", b / 1_073_741_824.0)
    } else if b >= 1_048_576.0 {
        format!("{:.1} MB", b / 1_048_576.0)
    } else {
        format!("{:.1} KB", b / 1024.0)
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
    DropUser(usize),
    /// (top-left row, top-left column, values)
    Paste(usize, usize, Vec<Vec<String>>),
    ImportDatabase(String, bool),
    ImportRows(String, bool),
}

/// The "edit whole row" dialog: the row as loaded and what the user has typed since.
struct EditRowState {
    row: usize,
    original: Vec<Cell>,
    values: Vec<Cell>,
}

enum JsonTarget {
    Cell(usize, usize),
    Doc(Vec<Cell>),
    NewDoc,
}

enum CtxTarget {
    Tree(usize),
    Tab(usize),
    Cell(usize, usize),
    Header(usize),
    Row(usize),
    DbMenu,
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
    /// Routine arguments, owning table, ... (see `DbObject::detail`).
    detail: String,
    key: String,
}

/// Everything that belongs to one open connection. The active one lives directly in `Worker`;
/// the others are parked here and swapped in when their tab is picked.
struct Session {
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
    session_pw: String,
    databases: Vec<String>,
    db_all_entry: bool,
    db_entries: Vec<String>,
    db_idx: i32,
    users: Vec<UserInfo>,
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
    conn_filter: String,
    ctx_target: Option<CtxTarget>,
    /// Kept in memory for this session only, to reconnect when another database is picked.
    session_pw: String,
    databases: Vec<String>,
    /// MySQL / MongoDB list an extra "All databases" entry first.
    db_all_entry: bool,
    users: Vec<UserInfo>,
    edit_row: Option<EditRowState>,
    xfer_mode: i32,
    /// The cell whose "saved ✓" / "!" marker is cleared by the next `ClearFlash`.
    flashed: Vec<(usize, usize)>,
    /// Entries of the database drop-down and the selected one.
    db_entries: Vec<String>,
    db_idx: i32,
    /// Open connections: (name, colour) per tab, and the parked state of every inactive one.
    sess_meta: Vec<(String, u32)>,
    parked: Vec<Option<Session>>,
    cur: usize,
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
            conn_filter: String::new(),
            ctx_target: None,
            session_pw: String::new(),
            databases: Vec::new(),
            db_all_entry: false,
            users: Vec::new(),
            edit_row: None,
            xfer_mode: 0,
            flashed: Vec::new(),
            db_entries: Vec::new(),
            db_idx: -1,
            sess_meta: Vec::new(),
            parked: Vec::new(),
            cur: 0,
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
                st.set_inspector_open(s.inspector_open && s.inspector_pinned);
                st.set_inspector_pinned(s.inspector_pinned);
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
        let q = self.conn_filter.clone();
        if !q.is_empty() {
            list.retain(|c| format!("{} {} {}", c.display_name(), c.host, c.environment.label()).to_lowercase().contains(&q));
        }
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
            ssh_host: f.ssh_host.trim().to_string(),
            ssh_port: f.ssh_port.trim().parse().unwrap_or(22),
            ssh_user: f.ssh_user.trim().to_string(),
            ssh_key: f.ssh_key.trim().to_string(),
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
                ssh_port: if c.ssh_host.is_empty() { String::new() } else { c.ssh_port.to_string() }.into(),
                ssh_host: c.ssh_host.into(),
                ssh_user: c.ssh_user.into(),
                ssh_key: c.ssh_key.into(),
            });
            st.set_form_is_new(is_new);
            st.set_form_info(info.into());
            st.set_form_error(error.into());
            st.set_form_busy(false);
        });
        self.push_connections();
    }

    /// Fill the form from a pasted `postgres://` / `mysql://` string.
    fn parse_conn_url(&mut self, text: &str) {
        let Some(p) = dboard_core::url::parse(text) else {
            return self.form_error("Not a postgres:// or mysql:// connection string.");
        };
        let mut c = ConnectionConfig::new_blank();
        if let Some(existing) = self.connections.iter().find(|c| c.id == self.form_id) {
            c.id = existing.id.clone();
        } else if !self.form_id.is_empty() {
            c.id = self.form_id.clone();
        }
        c.db_type = p.db_type;
        c.host = p.host.clone();
        c.port = p.port.unwrap_or(p.db_type.default_port());
        c.username = p.user;
        c.database = p.database;
        c.name = p.host;
        if let Some(s) = p.ssl {
            c.ssl = s;
        }
        self.show_form(&c, p.password, true, "Filled from connection string. Pick an environment, then Save or Connect.".into(), String::new());
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
            Ok(conn) => self.on_connected(cfg, conn, pw).await,
            Err(e) => self.form_error(e.to_string()),
        }
    }

    async fn on_connected(&mut self, cfg: ConnectionConfig, conn: Conn, pw: String) {
        // Park the connection we were using, if any; the new one gets its own tab.
        if self.conn.is_some() {
            let old = self.take_session();
            if self.cur < self.parked.len() {
                self.parked[self.cur] = Some(old);
            }
            self.cur = self.sess_meta.len();
        } else {
            self.sess_meta.clear();
            self.parked.clear();
            self.cur = 0;
        }
        self.sess_meta.push((cfg.display_name(), cfg.environment.color()));
        self.parked.push(None);
        self.env = cfg.environment;
        self.pending = None;
        self.edit_row = None;
        if let Some(c) = self.connections.iter_mut().find(|c| c.id == cfg.id) {
            c.last_used = config::now_secs();
        }
        let _ = self.store.save_connections(&self.connections);
        self.settings.last_connection_id = cfg.id.clone();
        self.persist_settings();
        self.session_pw = pw;
        self.conn = Some(conn);
        self.load_databases().await;
        self.show_session();
    }

    fn take_session(&mut self) -> Session {
        Session {
            conn: self.conn.take(),
            env: self.env,
            tabs: std::mem::take(&mut self.tabs),
            active: self.active.take(),
            closed: std::mem::take(&mut self.closed),
            query_counter: std::mem::take(&mut self.query_counter),
            tree_filter: std::mem::take(&mut self.tree_filter),
            collapsed: std::mem::take(&mut self.collapsed),
            tree: std::mem::take(&mut self.tree),
            activity: std::mem::take(&mut self.activity),
            session_pw: std::mem::take(&mut self.session_pw),
            databases: std::mem::take(&mut self.databases),
            db_all_entry: std::mem::take(&mut self.db_all_entry),
            db_entries: std::mem::take(&mut self.db_entries),
            db_idx: std::mem::replace(&mut self.db_idx, -1),
            users: std::mem::take(&mut self.users),
        }
    }

    fn put_session(&mut self, s: Session) {
        self.conn = s.conn;
        self.env = s.env;
        self.tabs = s.tabs;
        self.active = s.active;
        self.closed = s.closed;
        self.query_counter = s.query_counter;
        self.tree_filter = s.tree_filter;
        self.collapsed = s.collapsed;
        self.tree = s.tree;
        self.activity = s.activity;
        self.session_pw = s.session_pw;
        self.databases = s.databases;
        self.db_all_entry = s.db_all_entry;
        self.db_entries = s.db_entries;
        self.db_idx = s.db_idx;
        self.users = s.users;
        self.pending = None;
        self.edit_row = None;
    }

    fn push_sessions(&self) {
        let v: Vec<(String, u32, bool)> = self.sess_meta.iter().enumerate().map(|(i, (n, c))| (n.clone(), *c, i == self.cur)).collect();
        ui(&self.w, move |st| {
            let items: Vec<SessionTab> = v
                .into_iter()
                .map(|(n, c, active)| SessionTab { name: n.into(), color: slint::Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8), active })
                .collect();
            st.set_sessions(ModelRc::new(VecModel::from(items)));
        });
    }

    fn push_databases(&self) {
        let (entries, idx) = (self.db_entries.clone(), self.db_idx);
        ui(&self.w, move |st| {
            st.set_databases(strs(entries));
            st.set_database_index(idx);
        });
    }

    fn close_dialogs(&self) {
        ui(&self.w, |st| {
            st.set_busy(false);
            st.set_confirm_open(false);
            st.set_insert_open(false);
            st.set_json_open(false);
            st.set_palette_open(false);
            st.set_export_open(false);
            st.set_editrow_open(false);
            st.set_users_open(false);
            st.set_xfer_open(false);
            st.set_ctx_open(false);
            st.set_draft_open(false);
        });
    }

    /// Show the active session: header, databases, tree, tabs, grid and inspector.
    fn show_session(&mut self) {
        let Some(conn) = &self.conn else { return };
        let (name, color) = self.sess_meta.get(self.cur).cloned().unwrap_or_default();
        let _ = color;
        let (env, protected, db) = (self.env, self.protected(), conn.db_type().index() as i32);
        let status = self.status_line();
        let act = self.activity.clone();
        self.push_sessions();
        self.push_databases();
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
            st.set_tree_filter("".into());
            st.set_activity(strs(act));
            st.set_busy(false);
            st.set_connected(true);
        });
    }

    fn switch_session(&mut self, i: usize) {
        if i >= self.sess_meta.len() {
            return;
        }
        if i == self.cur && self.conn.is_some() {
            ui(&self.w, |st| st.set_connected(true));
            return;
        }
        let old = self.take_session();
        if self.cur < self.parked.len() {
            self.parked[self.cur] = Some(old);
        }
        if let Some(s) = self.parked[i].take() {
            self.put_session(s);
        }
        self.cur = i;
        self.close_dialogs();
        self.show_session();
    }

    /// Close one open connection (`-1` = the current one). The last one returns to the connection list.
    fn close_session(&mut self, idx: i32) {
        let i = if idx < 0 { self.cur } else { idx as usize };
        if i >= self.sess_meta.len() {
            return;
        }
        self.sess_meta.remove(i);
        if i == self.cur {
            drop(self.take_session());
            self.parked.remove(i);
            self.close_dialogs();
            if self.sess_meta.is_empty() {
                self.cur = 0;
                self.push_sessions();
                self.push_connections();
                ui(&self.w, |st| {
                    st.set_connected(false);
                    st.set_databases(strs(Vec::new()));
                });
            } else {
                self.cur = i.min(self.sess_meta.len() - 1);
                if let Some(s) = self.parked[self.cur].take() {
                    self.put_session(s);
                }
                self.show_session();
            }
        } else {
            self.parked.remove(i);
            if i < self.cur {
                self.cur -= 1;
            }
            self.push_sessions();
        }
    }

    /// "PostgreSQL 16.4 · user@host:5432 / database", for the top bar.
    fn status_line(&self) -> String {
        let Some(conn) = &self.conn else { return String::new() };
        let cfg = &conn.config;
        let who = if cfg.db_type == DbType::Mongo && !cfg.mongo_uri.is_empty() {
            cfg.mongo_uri.clone()
        } else {
            format!(
                "{}{}:{}{}",
                if cfg.username.is_empty() { String::new() } else { format!("{}@", cfg.username) },
                cfg.host,
                cfg.port,
                if cfg.database.is_empty() { String::new() } else { format!(" / {}", cfg.database) }
            )
        };
        format!("{} {} · {who}", cfg.db_type.label(), conn.server_version)
    }

    /// Fill the database drop-down with every database on the server.
    async fn load_databases(&mut self) {
        let Some(conn) = self.conn.as_mut() else { return };
        let list = conn.list_databases().await.unwrap_or_default();
        let current = conn.current_database().await.ok().flatten();
        self.db_all_entry = conn.db_type() != DbType::Postgres;
        let mut entries = Vec::new();
        if self.db_all_entry && !list.is_empty() {
            entries.push("All databases".to_string());
        }
        entries.extend(list.iter().cloned());
        let idx = match &current {
            Some(c) => entries.iter().position(|e| e == c).unwrap_or(0),
            None => 0,
        };
        self.databases = list;
        self.db_entries = entries;
        self.db_idx = idx as i32;
        self.push_databases();
    }

    async fn switch_database(&mut self, i: usize) {
        let pick: Option<String> = if self.db_all_entry {
            if i == 0 { None } else { self.databases.get(i - 1).cloned() }
        } else {
            self.databases.get(i).cloned()
        };
        let pw = self.session_pw.clone();
        let Some(conn) = self.conn.as_mut() else { return };
        ui(&self.w, |st| st.set_busy(true));
        let res = conn.switch_database(pick.as_deref(), &pw).await;
        match res {
            Ok(()) => {
                self.tabs.clear();
                self.closed.clear();
                self.active = None;
                self.collapsed.clear();
                self.tree_filter.clear();
                let name = pick.clone().unwrap_or_else(|| "all databases".into());
                self.log_activity(None, &format!("USE {name}"));
                let status = self.status_line();
                self.rebuild_tree();
                self.show_active();
                self.push_inspector();
                ui(&self.w, move |st| {
                    st.set_status(status.into());
                    st.set_tree_filter("".into());
                    st.set_busy(false);
                });
                self.toast(format!("Now using {name}"));
            }
            Err(e) => {
                // Put the drop-down back on the database that is still in use.
                self.load_databases().await;
                ui(&self.w, |st| st.set_busy(false));
                self.toast(format!("Could not switch database: {e}"));
            }
        }
    }

    fn disconnect(&mut self) {
        self.close_session(-1);
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
            entries.push((TreeEntry { kind: 0, schema: s.clone(), name: String::new(), detail: String::new(), key: skey }, label, 0, open));
            if !open {
                continue;
            }
            // (label, badge, name, detail)
            type Item = (String, i32, String, String);
            let of_kind = |k: ObjectKind, badge: i32| -> Vec<Item> {
                objects
                    .iter()
                    .filter(|o| o.kind == k)
                    .map(|o| {
                        let label = match k {
                            ObjectKind::Function | ObjectKind::Procedure if !o.detail.is_empty() => format!("{}({})", o.name, o.detail),
                            ObjectKind::Trigger | ObjectKind::Index | ObjectKind::Extension if !o.detail.is_empty() => format!("{}  ({})", o.name, o.detail),
                            _ => o.name.clone(),
                        };
                        (label, badge, o.name.clone(), o.detail.clone())
                    })
                    .collect()
            };
            let table_items = |f: &dyn Fn(&Table) -> Option<i32>| -> Vec<Item> {
                tables.iter().filter_map(|t| f(t).map(|b| (t.name.clone(), b, t.name.clone(), String::new()))).collect()
            };
            let mut routines = of_kind(ObjectKind::Function, 6);
            routines.extend(of_kind(ObjectKind::Procedure, 7));
            routines.sort_by(|a, b| a.0.cmp(&b.0));
            // (title, items, open by default). Bulky lists start collapsed so the tree stays readable.
            let sections: Vec<(&str, Vec<Item>, bool)> = vec![
                ("Tables", table_items(&|t| (t.kind == TableKind::Table).then_some(2)), true),
                ("Views", table_items(&|t| match t.kind { TableKind::View => Some(3), TableKind::MaterializedView => Some(4), _ => None }), true),
                ("Collections", table_items(&|t| (t.kind == TableKind::Collection).then_some(5)), true),
                ("Routines", routines, true),
                ("Sequences", of_kind(ObjectKind::Sequence, 8), true),
                ("Triggers", of_kind(ObjectKind::Trigger, 9), false),
                ("Types", of_kind(ObjectKind::Type, 10), false),
                ("Indexes", of_kind(ObjectKind::Index, 11), false),
                ("Events", of_kind(ObjectKind::Event, 12), false),
                ("Extensions", of_kind(ObjectKind::Extension, 13), false),
            ];
            for (title, items, default_open) in sections {
                if items.is_empty() {
                    continue;
                }
                let key = format!("s:{s}/{title}");
                // `collapsed` holds sections the user flipped away from their default.
                let sopen = filtering || (default_open != self.collapsed.contains(&key));
                entries.push((TreeEntry { kind: 1, schema: s.clone(), name: title.into(), detail: String::new(), key: key.clone() }, format!("{title} ({})", items.len()), 1, sopen));
                if sopen {
                    for (label, kind, name, detail) in items {
                        entries.push((TreeEntry { kind, schema: s.clone(), key: format!("{key}/{label}"), name, detail }, label, 2, false));
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

    fn find_object(&self, e: &TreeEntry) -> Option<DbObject> {
        let want = match e.kind {
            6 => ObjectKind::Function,
            7 => ObjectKind::Procedure,
            8 => ObjectKind::Sequence,
            9 => ObjectKind::Trigger,
            10 => ObjectKind::Type,
            11 => ObjectKind::Index,
            12 => ObjectKind::Event,
            13 => ObjectKind::Extension,
            _ => return None,
        };
        self.conn
            .as_ref()?
            .metadata
            .objects
            .iter()
            .find(|o| o.schema == e.schema && o.name == e.name && o.kind == want && o.detail == e.detail)
            .cloned()
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
        let active = self.active.map_or(-1, |i| i as i32);
        ui(&self.w, move |st| {
            st.set_active_tab(active);
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
                st.set_draft_open(false);
                st.set_has_next(false);
                st.set_tab_kind(-1);
                st.set_cols(ModelRc::new(VecModel::from(Vec::<ColInfo>::new())));
                st.set_rows(grid_model(Vec::new()));
                st.set_banner("".into());
                st.set_page_info("".into());
                st.set_timing("".into());
                st.set_table_title("".into());
                st.set_ddl_text("".into());
                st.set_selected_row(-1);
                st.set_sel_kind(0);
            }
            Some(t) => {
                let cols: Vec<ColInfo> = t
                    .cols
                    .iter()
                    .map(|c| ColInfo { name: c.name.clone().into(), type_name: c.type_name.clone().into(), pk: c.pk, fk: c.fk.clone().into(), is_bool: c.is_bool, is_json: c.is_json })
                    .collect();
                st.set_draft_open(false);
                st.set_has_next(t.rows.len() as i64 >= t.page.limit);
                st.set_tab_kind(t.kind as i32);
                st.set_cols(ModelRc::new(VecModel::from(cols)));
                st.set_col_widths(ModelRc::new(VecModel::from(t.widths.clone())));
                st.set_grid_width(t.widths.iter().sum());
                st.set_rows(grid_model(t.rows.iter().map(|r| r.iter().map(|c| (c.clone(), 0)).collect()).collect()));
                st.set_row_offset(t.page.offset as i32);
                st.set_selected_row(-1);
                st.set_sel_kind(0);
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

    /// Change one displayed cell in place (value and 0 idle / 1 saving / 2 saved / 3 error flag).
    /// Unlike rebuilding the grid this keeps every other cell, including one being edited, intact.
    fn set_cell(&self, r: usize, c: usize, value: Cell, state: i32) {
        ui(&self.w, move |st| {
            let rows = st.get_rows();
            if let Some(row) = slint::Model::row_data(&rows, r) {
                if c < slint::Model::row_count(&row) {
                    slint::Model::set_row_data(&row, c, GridCell { is_null: value.is_none(), text: value.unwrap_or_default().into(), state });
                }
            }
        });
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
        self.open_table_in(schema, name, false).await
    }

    /// Open a table; with `new_tab` always in a fresh tab (e.g. to compare two sort orders or filters).
    async fn open_table_in(&mut self, schema: &str, name: &str, new_tab: bool) {
        if !new_tab {
            if let Some(i) = self.tabs.iter().position(|t| t.kind == Kind::Table && t.schema == schema && t.name == name) {
                self.active = Some(i);
                return self.show_active();
            }
        }
        let same = self.tabs.iter().filter(|t| t.kind == Kind::Table && t.schema == schema && t.name == name).count();
        let title = if same == 0 { name.to_string() } else { format!("{name} ({})", same + 1) };
        let mut t = Tab::new(Kind::Table, title, self.default_page_size());
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
            _ => {
                if let Some(o) = self.find_object(&e) {
                    self.open_routine(o).await;
                }
            }
        }
    }

    /// SQL that calls a routine, with its arguments spelled out as NULL placeholders.
    fn call_template(&self, o: &DbObject) -> String {
        let d = self.dialect();
        let name = format!("{}.{}", d.quote(&o.schema), d.quote(&o.name));
        let args: Vec<String> = o.detail.split(", ").filter(|a| !a.trim().is_empty()).map(|a| format!("/* {} */ NULL", a.trim())).collect();
        let args = args.join(", ");
        if o.kind == ObjectKind::Procedure { format!("CALL {name}({args});") } else { format!("SELECT {name}({args});") }
    }

    async fn tree_action(&mut self, i: usize, action: &str) {
        let Some(e) = self.tree.get(i).cloned() else { return };
        let is_data = (2..=5).contains(&e.kind);
        match action {
            "open" if is_data => self.open_table(&e.schema, &e.name).await,
            "open-new" if is_data => self.open_table_in(&e.schema, &e.name, true).await,
            "structure" if is_data => self.open_structure(&e.schema, &e.name).await,
            "structure" | "definition" => {
                if let Some(o) = self.find_object(&e) {
                    self.open_routine(o).await;
                }
            }
            "copy-def" => {
                if is_data {
                    let ddl = match self.conn.as_mut() {
                        Some(c) => c.ddl(&e.schema, &e.name).await.unwrap_or_default(),
                        None => String::new(),
                    };
                    self.copy_to_clipboard(&ddl);
                } else if let Some(o) = self.find_object(&e) {
                    let def = match self.conn.as_mut() {
                        Some(c) => c.object_def(&o).await.unwrap_or_default(),
                        None => String::new(),
                    };
                    self.copy_to_clipboard(&def);
                }
            }
            "run" => {
                if let Some(o) = self.find_object(&e) {
                    let text = self.call_template(&o);
                    self.new_query_tab(text);
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
            "import" if is_data => {
                self.open_table(&e.schema, &e.name).await;
                self.open_transfer(2);
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

    /// Write one cell. On success the grid shows the new value; on failure it is left unchanged.
    async fn try_edit_cell(&mut self, r: usize, c: usize, text: String, null: bool) -> Result<bool, String> {
        let Some(tab) = self.active_tab().cloned() else { return Ok(false) };
        if tab.kind != Kind::Table || !tab.editable {
            return Err("This table cannot be edited.".into());
        }
        let (Some(col), Some(row)) = (tab.cols.get(c).cloned(), tab.rows.get(r).cloned()) else { return Ok(false) };
        let new: Cell = if null { None } else { Some(text) };
        if row.get(c) == Some(&new) {
            return Ok(false); // unchanged
        }
        if let Some(t) = self.active_mut() {
            t.rows[r][c] = new.clone();
        }
        self.set_cell(r, c, new.clone(), 1);
        self.flashed.push((r, c));
        let res = match self.conn.as_mut() {
            Some(conn) => conn.edit_cell(&tab.schema, &tab.name, &row, &col.name, new.clone()).await,
            None => return Ok(false),
        };
        match res {
            Ok(()) => {
                self.set_cell(r, c, new, 2);
                self.log_activity(None, &format!("UPDATE {}.{} SET {}", tab.schema, tab.name, col.name));
                self.push_inspector();
                self.schedule_clear();
                Ok(true)
            }
            Err(e) => {
                if let Some(t) = self.active_mut() {
                    t.rows[r][c] = row[c].clone();
                }
                self.set_cell(r, c, row[c].clone(), 3);
                self.log_activity(None, &format!("ERROR {e}"));
                self.schedule_clear();
                Err(e.to_string())
            }
        }
    }

    async fn edit_cell(&mut self, r: usize, c: usize, text: String, null: bool) {
        if let Err(e) = self.try_edit_cell(r, c, text, null).await {
            self.set_banner(&e, true);
        }
    }

    /// After Tab / Shift+Tab in a cell editor: open the neighbouring editable cell.
    fn edit_next(&mut self, r: usize, c: usize, dir: i32) {
        let Some(t) = self.active_tab().filter(|t| t.kind == Kind::Table && t.editable) else { return };
        let (rows, cols) = (t.rows.len() as i64, t.cols.len() as i64);
        if rows == 0 || cols == 0 {
            return;
        }
        let flat = r as i64 * cols + c as i64 + dir as i64;
        if flat < 0 || flat >= rows * cols {
            return;
        }
        let (nr, nc) = ((flat / cols) as i32, (flat % cols) as i32);
        ui(&self.w, move |st| {
            st.invoke_request_edit(nr, nc);
            st.set_selected_row(nr);
            st.set_sel_kind(1);
            st.set_sel_r0(nr);
            st.set_sel_r1(nr);
            st.set_sel_c0(nc);
            st.set_sel_c1(nc);
        });
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
        // The new row is typed straight into the grid: a hint per column says what an empty cell becomes.
        let hints: Vec<String> = table
            .columns
            .iter()
            .map(|c| match (&c.default, c.nullable) {
                (Some(d), _) => format!("default: {d}"),
                (None, true) => "NULL".to_string(),
                (None, false) => "required".to_string(),
            })
            .collect();
        let blank = vec![String::new(); hints.len()];
        ui(&self.w, move |st| {
            st.set_draft_open(false);
            st.set_draft_hints(strs(hints));
            st.set_draft(strs(blank));
            st.set_draft_open(true);
        });
    }

    /// Jump to the last page. SQL engines count the rows exactly; MongoDB uses its estimate.
    async fn last_page(&mut self) {
        let Some(tab) = self.active_tab().filter(|t| t.kind == Kind::Table).cloned() else { return };
        let dialect = self.dialect();
        let limit = tab.page.limit.max(1);
        let total: Option<i64> = if self.is_mongo() {
            self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).and_then(|t| t.estimated_rows)
        } else {
            let Some(table) = self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).cloned() else { return };
            let mut sql = format!("SELECT COUNT(*) FROM {}", dialect.qualified(&table));
            if let Some(f) = tab.page.filter.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
                sql.push_str(&format!(" WHERE {f}"));
            }
            match self.conn.as_mut() {
                Some(c) => match c.execute_query(&sql).await {
                    Ok(r) => r.rows.first().and_then(|row| row.first()).and_then(|v| v.as_deref()).and_then(|v| v.trim().parse().ok()),
                    Err(e) => return self.toast(format!("Could not count rows: {e}")),
                },
                None => return,
            }
        };
        let Some(total) = total.filter(|n| *n > 0) else { return };
        let offset = ((total - 1) / limit) * limit;
        if let Some(t) = self.active_mut() {
            t.page.offset = offset;
        }
        self.load_active().await;
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
                self.set_banner(&m, true);
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
        let text = format!("Delete the selected row from {}.{}? You can bring it back with Undo (top bar or History) until you disconnect.", tab.schema, tab.name);
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
                        self.toast("Row deleted. Undo is in History.");
                        self.log_activity(None, &format!("DELETE FROM {}.{}", tab.schema, tab.name));
                        self.push_inspector();
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
            Some(Pending::DropUser(i)) => self.user_drop(i).await,
            Some(Pending::Paste(r, c, grid)) => self.apply_paste(r, c, grid).await,
            Some(Pending::ImportDatabase(path, stop)) => self.run_import_db(path, stop).await,
            Some(Pending::ImportRows(path, header)) => self.run_import_rows(path, header).await,
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

const COMMANDS: [(&str, &str, &str); 14] = [
    ("new-query", "New query tab", "Ctrl+N"),
    ("refresh", "Refresh database metadata", "Ctrl+R"),
    ("undo", "Undo last database edit", ""),
    ("reopen", "Reopen closed tab", "Ctrl+Shift+T"),
    ("close-tab", "Close active tab", "Ctrl+W"),
    ("inspector", "Toggle inspector panel", "Ctrl+Alt+I"),
    ("export", "Export current results…", ""),
    ("settings", "Open preferences", ""),
    ("disconnect", "Disconnect / switch connection", ""),
    ("users", "Users & access…", ""),
    ("export-db", "Export database…", ""),
    ("import-db", "Import database…", ""),
    ("import-rows", "Import rows into the open table…", ""),
    ("history", "Show history, saved queries and changes", ""),
];

impl Worker {
    fn build_palette(&mut self, q: &str) {
        let Some(conn) = &self.conn else { return };
        let mut scored: Vec<(i32, PaletteItem, PaletteAction)> = Vec::new();
        // Stable order for equal scores (e.g. an empty query): keep insertion order.
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
            let sub = if o.detail.is_empty() || o.kind == ObjectKind::Type { String::new() } else { o.detail.clone() };
            add(suggest::fuzzy(q, &label), label, sub, o.kind.label(), PaletteAction::Object(o.clone()));
        }
        if !self.palette_search {
            for (i, s) in self.saved.iter().enumerate() {
                add(suggest::fuzzy(q, &s.name), s.name.clone(), s.folder.clone(), "saved query", PaletteAction::Saved(i));
            }
            for c in &self.connections {
                add(suggest::fuzzy(q, &c.display_name()), format!("Switch to {}", c.display_name()), c.host.clone(), "connection", PaletteAction::Connection(c.id.clone()));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0)); // stable: ties keep insertion order
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
                "users" => self.open_users().await,
                "export-db" => self.open_transfer(0),
                "import-db" => self.open_transfer(1),
                "import-rows" => self.open_transfer(2),
                "history" => ui(&self.w, |st| st.set_drawer_open(true)),
                _ => {}
            },
            PaletteAction::OpenTable(s, n) | PaletteAction::Column(s, n) => self.open_table(&s, &n).await,
            PaletteAction::Object(o) => self.open_routine(o).await,
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
        self.select_conn(id);
        ui(&self.w, |st| st.set_busy(true));
        match Conn::connect(cfg.clone(), &pw).await {
            Ok(conn) => self.on_connected(cfg, conn, pw).await,
            Err(e) => self.form_error(e.to_string()),
        }
    }

    fn toggle_inspector(&mut self) {
        let (open, pinned) = (!self.settings.inspector_open, self.settings.inspector_pinned);
        self.inspector_changed(open, pinned);
    }

    async fn refresh(&mut self) {
        if let Some(c) = self.conn.as_mut() {
            if let Err(e) = c.refresh_metadata().await {
                return self.toast(e.to_string());
            }
        }
        self.load_databases().await;
        self.rebuild_tree();
        self.load_active().await;
        self.toast("Metadata refreshed");
    }

    fn push_inspector(&self) {
        let Some(conn) = &self.conn else { return };
        let entries = conn.history.entries();
        let show = |v: &Cell| v.as_deref().map(|s| one_line(s).chars().take(24).collect::<String>()).unwrap_or_else(|| "NULL".into());
        let log: Vec<(String, String, bool)> = entries
            .iter()
            .enumerate()
            .rev()
            .map(|(i, r)| {
                let can_undo = !conn.history.blocked_by_newer(i);
                match &r.kind {
                    EditKind::Update { column, old, new } => (
                        format!("{}.{}   {} → {}", r.table, column, show(old), show(new)),
                        format!("{}.{} · {}{}", r.schema, r.table, when(r.at), if can_undo { "" } else { " · undo the newer change first" }),
                        can_undo,
                    ),
                    EditKind::Delete { row, .. } => (
                        format!("Deleted a row from {}", r.table),
                        format!("{} · {}", row.iter().take(4).map(&show).collect::<Vec<_>>().join(", "), when(r.at)),
                        true,
                    ),
                }
            })
            .collect();
        let n_undo = conn.history.len() as i32;
        let c = &conn.config;
        let indexes: usize = conn.metadata.tables.iter().map(|t| t.indexes.len()).sum();
        let views = conn.metadata.tables.iter().filter(|t| matches!(t.kind, TableKind::View | TableKind::MaterializedView)).count();
        let row = |l: &str, v: String| (l.to_string(), v);
        let mut meta = vec![
            row("Connection", if c.mongo_uri.is_empty() { format!("{}@{}:{}/{}", c.username, c.host, c.port, c.database) } else { c.mongo_uri.clone() }),
            row("Engine", format!("{} {}", c.db_type.label(), conn.server_version)),
            row("Environment", format!("{}{}", c.environment.label(), if self.protected() { " · destructive statements need confirmation" } else { "" })),
            row("Tables", (conn.metadata.tables.len() - views).to_string()),
            row("Views", views.to_string()),
            row("Routines & sequences", conn.metadata.objects.len().to_string()),
            row("Indexes", indexes.to_string()),
        ];
        if let Some(t) = self.active_tab().filter(|t| t.kind == Kind::Table) {
            if let Some(tab) = conn.table(&t.schema, &t.name) {
                meta.push(row("", String::new()));
                meta.push(row(&format!("Selected: {}", tab.full_name()), String::new()));
                meta.push(row("Columns", tab.columns.len().to_string()));
                if let Some(n) = tab.estimated_rows {
                    meta.push(row("Rows (estimate)", group_digits(n)));
                }
                if let Some(b) = tab.size_bytes {
                    meta.push(row("Size", human_size(b)));
                }
                let pk: Vec<&str> = tab.primary_keys().iter().map(|c| c.name.as_str()).collect();
                meta.push(row("Primary key", if pk.is_empty() { "none".to_string() } else { pk.join(", ") }));
            }
        }
        ui(&self.w, move |st| {
            let items: Vec<EditItem> = log.into_iter().map(|(t, d, u)| EditItem { text: t.into(), detail: d.into(), can_undo: u }).collect();
            st.set_edit_items(ModelRc::new(VecModel::from(items)));
            st.set_undo_count(n_undo);
            st.set_meta_rows(ModelRc::new(VecModel::from(meta.into_iter().map(|(l, v)| MetaRow { label: l.into(), value: v.into() }).collect::<Vec<_>>())));
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
        let table = self.export_table_name(t);
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
// Selection, clipboard, whole-row editing
// ---------------------------------------------------------------------------------------------

impl Worker {
    /// A selection rectangle (any corner order) clamped to what is loaded: (row0, col0, row1, col1).
    fn norm_rect(&self, r0: usize, c0: usize, r1: usize, c1: usize) -> Option<(usize, usize, usize, usize)> {
        let t = self.active_tab()?;
        if t.rows.is_empty() || t.cols.is_empty() {
            return None;
        }
        let (rmax, cmax) = (t.rows.len() - 1, t.cols.len() - 1);
        Some((r0.min(r1).min(rmax), c0.min(c1).min(cmax), r0.max(r1).min(rmax), c0.max(c1).min(cmax)))
    }

    fn selection_data(&self, rect: (usize, usize, usize, usize)) -> Option<(Vec<ExportCol>, Vec<Vec<Cell>>)> {
        let t = self.active_tab()?;
        let (r0, c0, r1, c1) = rect;
        let cols = t.cols.iter().skip(c0).take(c1 - c0 + 1).map(|c| ExportCol { name: c.name.clone(), type_name: c.type_name.clone() }).collect();
        let rows = t.rows.iter().skip(r0).take(r1 - r0 + 1).map(|r| r.iter().skip(c0).take(c1 - c0 + 1).cloned().collect()).collect();
        Some((cols, rows))
    }

    fn export_table_name(&self, t: &Tab) -> String {
        let d = self.export_dialect;
        if t.name.is_empty() {
            "results".to_string()
        } else if t.schema.is_empty() {
            d.quote(&t.name)
        } else {
            format!("{}.{}", d.quote(&t.schema), d.quote(&t.name))
        }
    }

    /// Copy the selected cells. Modes: 0 plain, 1 with header row, 2 CSV, 3 JSON, 4 SQL INSERT, 5 column names.
    fn copy_selection(&mut self, r0: usize, c0: usize, r1: usize, c1: usize, mode: i32) {
        let Some(rect) = self.norm_rect(r0, c0, r1, c1) else { return self.toast("Nothing selected") };
        let Some((cols, rows)) = self.selection_data(rect) else { return };
        self.export_dialect = self.dialect();
        let table = self.active_tab().map(|t| self.export_table_name(t)).unwrap_or_default();
        let text = match mode {
            1 => export::tsv(&cols, &rows, true),
            2 => export::csv(&cols, &rows, true),
            3 => export::json(&cols, &rows),
            4 => export::sql_inserts(&cols, &rows, &table, self.export_dialect),
            5 => cols.iter().map(|c| c.name.clone()).collect::<Vec<_>>().join("\t"),
            _ => export::tsv(&cols, &rows, false),
        };
        let what = if mode == 5 {
            format!("{} column name(s)", cols.len())
        } else if cols.len() == 1 && rows.len() > 1 {
            format!("column “{}” ({} values)", cols[0].name, rows.len())
        } else if rows.len() == 1 && cols.len() > 1 {
            format!("row ({} values)", cols.len())
        } else if rows.len() == 1 {
            "value".to_string()
        } else {
            format!("{} rows × {} columns", rows.len(), cols.len())
        };
        match crate::clipboard::set(&text) {
            Ok(()) => self.toast(format!("Copied {what}")),
            Err(e) => self.toast(format!("Clipboard unavailable: {e}")),
        }
    }

    fn paste_selection(&mut self, r0: usize, c0: usize, r1: usize, c1: usize) {
        let Some(t) = self.active_tab() else { return };
        if t.kind != Kind::Table || !t.editable {
            return self.toast("This table is read-only, so nothing can be pasted into it");
        }
        let Some((rr0, cc0, rr1, cc1)) = self.norm_rect(r0, c0, r1, c1) else { return };
        let text = match crate::clipboard::get() {
            Ok(t) => t,
            Err(e) => return self.toast(format!("Clipboard unavailable: {e}")),
        };
        let mut grid = export::parse_tsv(&text);
        if grid.is_empty() {
            return self.toast("The clipboard is empty");
        }
        let (sel_rows, sel_cols) = (rr1 - rr0 + 1, cc1 - cc0 + 1);
        if grid.len() == 1 && grid[0].len() == 1 && sel_rows * sel_cols > 1 {
            // One value over a selection fills the whole selection, like a spreadsheet.
            grid = vec![vec![grid[0][0].clone(); sel_cols]; sel_rows];
        }
        let (rows, cols) = (t.rows.len(), t.cols.len());
        grid.truncate(rows - rr0);
        for r in grid.iter_mut() {
            r.truncate(cols - cc0);
        }
        let n: usize = grid.iter().map(Vec::len).sum();
        if n == 0 {
            return;
        }
        if n == 1 {
            let v = grid[0][0].clone();
            let tx = self.tx.clone();
            let _ = tx.send(Cmd::EditCell(rr0, cc0, v, false));
            return;
        }
        let name = if t.name.is_empty() { t.title.clone() } else { t.name.clone() };
        self.ask(Pending::Paste(rr0, cc0, grid), format!("Paste {n} cells"), format!("Write {n} pasted values into {name}, starting at row {} of this page? Every change can be undone from History.", rr0 + 1));
    }

    async fn apply_paste(&mut self, r0: usize, c0: usize, grid: Vec<Vec<String>>) {
        let (mut done, mut failed) = (0usize, None);
        'outer: for (dr, row) in grid.iter().enumerate() {
            for (dc, v) in row.iter().enumerate() {
                match self.try_edit_cell(r0 + dr, c0 + dc, v.clone(), false).await {
                    Ok(true) => done += 1,
                    Ok(false) => {}
                    Err(e) => {
                        failed = Some(e);
                        break 'outer;
                    }
                }
            }
        }
        match failed {
            None => self.toast(format!("Pasted {done} value(s)")),
            Some(e) => {
                self.set_banner(&format!("Paste stopped after {done} value(s): {e}"), true);
            }
        }
    }

    // ---- edit a whole row -------------------------------------------------------------------

    fn push_edit_row(&self) {
        let (Some(er), Some(t)) = (&self.edit_row, self.active_tab()) else { return };
        let fields: Vec<(String, String, String, String, bool)> = t
            .cols
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let v = er.values.get(i).cloned().flatten();
                (c.name.clone(), format!("{}{}", c.type_name, if c.pk { " · PK" } else { "" }), if c.pk { "primary key" } else { "" }.to_string(), v.clone().unwrap_or_default(), v.is_none())
            })
            .collect();
        ui(&self.w, move |st| {
            let v: Vec<FieldItem> = fields.into_iter().map(|(n, t, h, val, nul)| FieldItem { name: n.into(), type_name: t.into(), hint: h.into(), value: val.into(), is_null: nul }).collect();
            st.set_editrow_fields(ModelRc::new(VecModel::from(v)));
        });
    }

    fn open_edit_row(&mut self, r: usize) {
        let Some(t) = self.active_tab() else { return };
        if t.kind != Kind::Table || !t.editable {
            return self.toast("This table is read-only");
        }
        let Some(row) = t.rows.get(r).cloned() else { return };
        let title = format!("Edit row {} · {}.{}", t.page.offset + r as i64 + 1, t.schema, t.name);
        self.edit_row = Some(EditRowState { row: r, original: row.clone(), values: row });
        self.push_edit_row();
        ui(&self.w, move |st| {
            st.set_editrow_title(title.into());
            st.set_editrow_error("".into());
            st.set_editrow_open(true);
        });
    }

    async fn editrow_submit(&mut self) {
        let Some(er) = self.edit_row.as_ref() else { return };
        let (row, original, values) = (er.row, er.original.clone(), er.values.clone());
        let names: Vec<String> = self.active_tab().map(|t| t.cols.iter().map(|c| c.name.clone()).collect()).unwrap_or_default();
        let mut saved = 0;
        for c in 0..values.len() {
            if values[c] == original[c] {
                continue;
            }
            let (text, null) = (values[c].clone().unwrap_or_default(), values[c].is_none());
            match self.try_edit_cell(row, c, text, null).await {
                Ok(_) => {
                    saved += 1;
                    if let Some(er) = self.edit_row.as_mut() {
                        er.original[c] = values[c].clone();
                    }
                }
                Err(e) => {
                    let msg = format!("Could not save “{}”: {e}", names.get(c).cloned().unwrap_or_default());
                    ui(&self.w, move |st| st.set_editrow_error(msg.into()));
                    return;
                }
            }
        }
        self.edit_row = None;
        ui(&self.w, |st| st.set_editrow_open(false));
        self.toast(if saved == 0 { "No changes".to_string() } else { format!("Saved {saved} change(s)") });
    }
}

// ---------------------------------------------------------------------------------------------
// Undo / history
// ---------------------------------------------------------------------------------------------

impl Worker {
    async fn after_undo(&mut self, r: dboard_core::edit::EditRecord) {
        let what = match &r.kind {
            EditKind::Update { column, .. } => format!("Reverted {column}"),
            EditKind::Delete { .. } => format!("Restored the deleted row in {}", r.table),
        };
        self.log_activity(None, &format!("UNDO {}.{}", r.schema, r.table));
        self.toast(what);
        if self.active_tab().is_some_and(|t| t.kind == Kind::Table && t.schema == r.schema && t.name == r.table) {
            self.load_active().await;
        }
    }

    async fn undo(&mut self) {
        let res = match self.conn.as_mut() {
            Some(c) => c.undo().await,
            None => return,
        };
        match res {
            Ok(Some(r)) => self.after_undo(r).await,
            Ok(None) => self.toast("Nothing to undo"),
            Err(e) => self.toast(e.to_string()),
        }
        self.push_inspector();
    }

    /// Undo one entry of the list, which is shown newest first.
    async fn undo_entry(&mut self, shown: usize) {
        let Some(conn) = self.conn.as_mut() else { return };
        let n = conn.history.len();
        if shown >= n {
            return;
        }
        match conn.undo_at(n - 1 - shown).await {
            Ok(r) => self.after_undo(r).await,
            Err(e) => self.toast(e.to_string()),
        }
        self.push_inspector();
    }

    fn inspector_changed(&mut self, open: bool, pinned: bool) {
        self.settings.inspector_open = open;
        self.settings.inspector_pinned = pinned;
        self.persist_settings();
        ui(&self.w, move |st| {
            st.set_inspector_open(open);
            st.set_inspector_pinned(pinned);
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Users & access
// ---------------------------------------------------------------------------------------------

impl Worker {
    fn user_scope_text(&self, db: Option<String>) -> String {
        match (self.conn.as_ref().map(|c| c.db_type()), db) {
            (Some(DbType::MySql), None) => "Access levels apply to all databases (pick one in the top bar to limit them to it).".to_string(),
            (Some(DbType::Mongo), None) => "Pick a database in the top bar first: MongoDB access levels apply to one database.".to_string(),
            (_, Some(d)) => format!("Access levels apply to the database “{d}”. Users themselves are server-wide."),
            _ => String::new(),
        }
    }

    fn push_users(&self, select: i32) {
        let rows: Vec<(String, String)> = self
            .users
            .iter()
            .map(|u| (u.display(), u.summary.clone()))
            .collect();
        ui(&self.w, move |st| {
            let v: Vec<UserRow> = rows.into_iter().map(|(n, d)| UserRow { name: n.into(), detail: d.into() }).collect();
            st.set_users(ModelRc::new(VecModel::from(v)));
            st.set_user_sel(select);
        });
    }

    async fn open_users(&mut self) {
        let Some(conn) = self.conn.as_mut() else { return };
        let db = conn.current_database().await.ok().flatten();
        let ty = conn.db_type();
        let scope = self.user_scope_text(db);
        ui(&self.w, move |st| {
            st.set_user_is_mysql(ty == DbType::MySql);
            st.set_user_is_pg(ty == DbType::Postgres);
            st.set_user_scope(scope.into());
            st.set_user_error("".into());
            st.set_user_info("".into());
            st.set_user_grants(strs(Vec::new()));
            st.set_user_create_open(false);
            st.set_users_open(true);
        });
        self.reload_users(None).await;
    }

    /// Re-read the user list; keep `select` (by display name) selected when it still exists.
    async fn reload_users(&mut self, select: Option<String>) {
        let Some(conn) = self.conn.as_mut() else { return };
        match conn.list_users().await {
            Ok(list) => {
                self.users = list;
                let idx = select.and_then(|n| self.users.iter().position(|u| u.display() == n)).map_or(-1, |i| i as i32);
                self.push_users(idx);
                if idx >= 0 {
                    self.user_select(idx as usize).await;
                } else {
                    ui(&self.w, |st| st.set_user_grants(strs(Vec::new())));
                }
                ui(&self.w, |st| st.set_user_error("".into()));
            }
            Err(e) => {
                self.users.clear();
                self.push_users(-1);
                let m = e.to_string();
                ui(&self.w, move |st| st.set_user_error(m.into()));
            }
        }
    }

    async fn user_select(&mut self, i: usize) {
        let Some(u) = self.users.get(i).cloned() else { return };
        ui(&self.w, move |st| st.set_user_sel(i as i32));
        let lines = match self.conn.as_mut() {
            Some(c) => c.user_grants(&u).await.unwrap_or_else(|e| vec![format!("Could not read privileges: {e}")]),
            None => return,
        };
        ui(&self.w, move |st| {
            st.set_user_grants(strs(lines));
            st.set_user_error("".into());
        });
    }

    fn user_busy(&self, busy: bool) {
        ui(&self.w, move |st| st.set_user_busy(busy));
    }

    fn user_result(&self, ok: Option<String>, err: Option<String>) {
        ui(&self.w, move |st| {
            st.set_user_busy(false);
            st.set_user_info(ok.unwrap_or_default().into());
            st.set_user_error(err.unwrap_or_default().into());
        });
    }

    async fn user_create(&mut self, name: String, host: String, password: String, level: i32, admin: bool) {
        let new = NewUser { name: name.trim().to_string(), host: host.trim().to_string(), password, access: AccessLevel::from_index(level), admin };
        self.user_busy(true);
        let res = match self.conn.as_mut() {
            Some(c) => c.create_user(&new).await,
            None => return,
        };
        match res {
            Ok(()) => {
                self.log_activity(None, &format!("CREATE USER {}", new.name));
                let shown = UserInfo { name: new.name.clone(), origin: if self.conn.as_ref().is_some_and(|c| c.db_type() == DbType::MySql) { if new.host.is_empty() { "%".into() } else { new.host.clone() } } else { String::new() }, summary: String::new() }.display();
                ui(&self.w, |st| st.set_user_create_open(false));
                self.reload_users(Some(shown)).await;
                self.user_result(Some(format!("User “{}” created.", new.name)), None);
            }
            Err(e) => self.user_result(None, Some(e.to_string())),
        }
    }

    async fn user_set_level(&mut self, i: usize, level: i32) {
        let Some(u) = self.users.get(i).cloned() else { return };
        self.user_busy(true);
        let res = match self.conn.as_mut() {
            Some(c) => c.set_access(&u, AccessLevel::from_index(level)).await,
            None => return,
        };
        match res {
            Ok(()) => {
                self.log_activity(None, &format!("SET ACCESS {} = {}", u.name, AccessLevel::LABELS[level.clamp(0, 3) as usize]));
                self.user_select(i).await;
                self.user_result(Some(format!("{} now has “{}”.", u.name, AccessLevel::LABELS[level.clamp(0, 3) as usize])), None);
            }
            Err(e) => self.user_result(None, Some(e.to_string())),
        }
    }

    async fn user_password(&mut self, i: usize, pw: String) {
        let Some(u) = self.users.get(i).cloned() else { return };
        self.user_busy(true);
        let res = match self.conn.as_mut() {
            Some(c) => c.set_password(&u, &pw).await,
            None => return,
        };
        match res {
            Ok(()) => {
                self.log_activity(None, &format!("CHANGE PASSWORD {}", u.name));
                self.user_result(Some(format!("Password changed for {}.", u.name)), None);
            }
            Err(e) => self.user_result(None, Some(e.to_string())),
        }
    }

    async fn user_drop(&mut self, i: usize) {
        let Some(u) = self.users.get(i).cloned() else { return };
        self.user_busy(true);
        let res = match self.conn.as_mut() {
            Some(c) => c.drop_user(&u).await,
            None => return,
        };
        match res {
            Ok(()) => {
                self.log_activity(None, &format!("DROP USER {}", u.name));
                self.reload_users(None).await;
                self.user_result(Some(format!("User {} dropped.", u.name)), None);
            }
            Err(e) => self.user_result(None, Some(e.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Export / import
// ---------------------------------------------------------------------------------------------

/// One record to import: (column, value) pairs, plus the raw JSON object when the source was JSON.
type Record = (Vec<(String, String)>, Option<String>);

impl Worker {
    fn xfer_progress(&self) -> impl FnMut(String) {
        let w = self.w.clone();
        move |m| ui(&w, move |st| st.set_xfer_info(m.into()))
    }

    fn xfer_done(&self, info: String, error: String) {
        ui(&self.w, move |st| {
            st.set_xfer_busy(false);
            st.set_xfer_info(info.into());
            st.set_xfer_error(error.into());
        });
    }

    fn open_transfer(&mut self, mode: i32) {
        let Some(conn) = &self.conn else { return };
        let mongo = conn.db_type() == DbType::Mongo;
        let db = if conn.config.database.is_empty() { conn.config.display_name() } else { conn.config.database.clone() };
        let (title, note, path, a) = match mode {
            0 => {
                let ext = if mongo { "json" } else { "sql" };
                let name = format!("{}-{}.{ext}", sanitize(&db), chrono::Local::now().format("%Y%m%d-%H%M%S"));
                (
                    format!("Export {db}"),
                    if mongo {
                        "Writes every collection (documents and indexes) to one JSON file that this app can import again.".to_string()
                    } else {
                        "Writes a SQL script that recreates the database: tables, constraints, indexes, views, routines, triggers and data. It runs on an empty database with this app, psql or mysql.".to_string()
                    },
                    downloads_dir().join(name).display().to_string(),
                    true,
                )
            }
            1 => {
                let kind = if mongo { "a dboard MongoDB export (.json)" } else { "a SQL script (.sql), including pg_dump and mysqldump files" };
                (
                    format!("Import into {db}"),
                    format!("Runs {kind} against this database. Existing objects with the same names will make statements fail, so use an empty database for a clean restore."),
                    String::new(),
                    true,
                )
            }
            _ => {
                let Some(t) = self.active_tab().filter(|t| t.kind == Kind::Table) else { return self.toast("Open a table first, then import rows into it") };
                (
                    format!("Import rows into {}", t.name),
                    "Reads a CSV / TSV file (first row = column names) or a JSON file (an array of objects, or one object per line) and inserts each record as a new row. Empty values use the column default.".to_string(),
                    String::new(),
                    true,
                )
            }
        };
        self.xfer_mode = mode;
        ui(&self.w, move |st| {
            st.set_xfer_mode(mode);
            st.set_xfer_title(title.into());
            st.set_xfer_note(note.into());
            st.set_xfer_path(path.into());
            st.set_xfer_opt_a(a);
            st.set_xfer_opt_b(true);
            st.set_xfer_info("".into());
            st.set_xfer_error("".into());
            st.set_xfer_busy(false);
            st.set_xfer_open(true);
        });
    }

    async fn xfer_browse(&mut self) {
        let mongo = self.is_mongo();
        let mut dlg = rfd::AsyncFileDialog::new();
        let picked = if self.xfer_mode == 0 {
            let ext = if mongo { "json" } else { "sql" };
            dlg = dlg.set_title("Save export").add_filter(ext, &[ext]).set_directory(downloads_dir());
            dlg.set_file_name(format!("export.{ext}")).save_file().await
        } else {
            dlg = dlg.set_title("Choose a file to import").set_directory(downloads_dir());
            dlg = if self.xfer_mode == 1 { dlg.add_filter("SQL / JSON", &["sql", "json", "txt"]) } else { dlg.add_filter("CSV / TSV / JSON", &["csv", "tsv", "txt", "json"]) };
            dlg.pick_file().await
        };
        if let Some(f) = picked {
            let p = f.path().display().to_string();
            ui(&self.w, move |st| st.set_xfer_path(p.into()));
        }
    }

    async fn xfer_run(&mut self, path: String, a: bool, b: bool) {
        let path = path.trim().to_string();
        if path.is_empty() {
            return;
        }
        match self.xfer_mode {
            0 => self.run_export(path, a, b).await,
            1 => {
                if self.protected() {
                    let env = self.env.label();
                    self.ask(Pending::ImportDatabase(path.clone(), a), format!("Import into {env}"), format!("Run every statement in {path} against this {env} database?"));
                } else {
                    self.run_import_db(path, a).await;
                }
            }
            _ => {
                if self.protected() {
                    let env = self.env.label();
                    self.ask(Pending::ImportRows(path.clone(), a), format!("Import rows into {env}"), format!("Insert every record in {path} as a new row?"));
                } else {
                    self.run_import_rows(path, a).await;
                }
            }
        }
    }

    async fn run_export(&mut self, path: String, schema: bool, data: bool) {
        if !schema && !data {
            return self.xfer_done(String::new(), "Choose structure, data, or both.".into());
        }
        ui(&self.w, |st| {
            st.set_xfer_busy(true);
            st.set_xfer_error("".into());
            st.set_xfer_info("Exporting…".into());
        });
        let mut progress = self.xfer_progress();
        let res = match self.conn.as_mut() {
            Some(c) => c.export_database(std::path::Path::new(&path), &DumpOptions { schema, data }, &mut progress).await,
            None => return,
        };
        match res {
            Ok(st) => {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                self.log_activity(None, &format!("EXPORT DATABASE {path}"));
                self.xfer_done(format!("Done: {} table(s), {} row(s), {} other object(s) · {:.1} KB written to {path}", st.tables, st.rows, st.objects, size as f64 / 1024.0), String::new());
            }
            Err(e) => self.xfer_done(String::new(), e.to_string()),
        }
    }

    async fn run_import_db(&mut self, path: String, stop: bool) {
        ui(&self.w, |st| {
            st.set_xfer_busy(true);
            st.set_xfer_error("".into());
            st.set_xfer_info("Importing…".into());
        });
        let mut progress = self.xfer_progress();
        let res = match self.conn.as_mut() {
            Some(c) => c.import_database(std::path::Path::new(&path), &ImportOptions { stop_on_error: stop }, &mut progress).await,
            None => return,
        };
        match res {
            Ok(st) => {
                self.log_activity(None, &format!("IMPORT DATABASE {path}"));
                let mut info = format!("Done: {} statement(s) run", st.statements);
                if st.rows_copied > 0 {
                    info.push_str(&format!(", {} row(s) loaded", st.rows_copied));
                }
                let err = if st.errors.is_empty() {
                    String::new()
                } else {
                    format!("{} problem(s):\n{}", st.errors.len(), st.errors.iter().take(5).cloned().collect::<Vec<_>>().join("\n"))
                };
                self.xfer_done(info, err);
                self.rebuild_tree();
                self.load_databases().await;
                self.load_active().await;
            }
            Err(e) => {
                self.xfer_done(String::new(), e.to_string());
                self.rebuild_tree();
            }
        }
    }

    /// Records (column name, value) from a CSV / TSV / JSON file, plus the raw JSON object when the
    /// source was JSON (MongoDB keeps nested values that way).
    fn read_records(path: &str, header: bool, columns: &[String]) -> Result<Vec<Record>, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let trimmed = text.trim_start_matches('\u{feff}').trim_start();
        let lower = path.to_lowercase();
        if lower.ends_with(".json") || trimmed.starts_with('[') || trimmed.starts_with('{') {
            let values: Vec<serde_json::Value> = match serde_json::from_str::<serde_json::Value>(trimmed) {
                Ok(serde_json::Value::Array(a)) => a,
                Ok(v @ serde_json::Value::Object(_)) => vec![v],
                _ => trimmed
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .enumerate()
                    .map(|(i, l)| serde_json::from_str(l.trim().trim_end_matches(',')).map_err(|e| format!("Line {}: {e}", i + 1)))
                    .collect::<Result<_, _>>()?,
            };
            return values
                .into_iter()
                .enumerate()
                .map(|(i, v)| {
                    let serde_json::Value::Object(m) = &v else { return Err(format!("Record {} is not a JSON object", i + 1)) };
                    let pairs = m
                        .iter()
                        .filter(|(_, x)| !x.is_null())
                        .map(|(k, x)| (k.clone(), match x { serde_json::Value::String(s) => s.clone(), other => other.to_string() }))
                        .collect();
                    Ok((pairs, Some(v.to_string())))
                })
                .collect();
        }
        let sep = if lower.ends_with(".tsv") || (trimmed.lines().next().unwrap_or("").matches('\t').count() > trimmed.lines().next().unwrap_or("").matches(',').count()) { '\t' } else { ',' };
        let mut grid = export::parse_delimited(&text, sep);
        let names: Vec<String> = if header {
            if grid.is_empty() {
                return Ok(Vec::new());
            }
            grid.remove(0).into_iter().map(|h| h.trim().to_string()).collect()
        } else {
            columns.to_vec()
        };
        Ok(grid
            .into_iter()
            .map(|r| (names.iter().cloned().zip(r).filter(|(_, v)| !v.is_empty()).collect(), None))
            .collect())
    }

    async fn run_import_rows(&mut self, path: String, header: bool) {
        let Some(tab) = self.active_tab().filter(|t| t.kind == Kind::Table).cloned() else { return };
        let Some(table) = self.conn.as_ref().and_then(|c| c.table(&tab.schema, &tab.name)).cloned() else { return };
        let all_cols: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
        ui(&self.w, |st| {
            st.set_xfer_busy(true);
            st.set_xfer_error("".into());
            st.set_xfer_info("Reading file…".into());
        });
        let records = match Self::read_records(&path, header, &all_cols) {
            Ok(r) => r,
            Err(e) => return self.xfer_done(String::new(), e),
        };
        // Match file columns to table columns ignoring case; unknown ones are reported, not skipped.
        let resolve = |name: &str| -> Option<String> { table.columns.iter().find(|c| c.name.eq_ignore_ascii_case(name.trim())).map(|c| c.name.clone()) };
        let mongo = self.is_mongo();
        let total = records.len();
        let mut progress = self.xfer_progress();
        for (n, (pairs, raw)) in records.into_iter().enumerate() {
            let result: Result<(), String> = async {
                let conn = self.conn.as_mut().ok_or("Not connected")?;
                if let (true, Some(json)) = (mongo, raw) {
                    return conn.insert_document(&tab.schema, &tab.name, &json).await.map_err(|e| e.to_string());
                }
                let mut vals = Vec::new();
                for (k, v) in pairs {
                    let col = resolve(&k).ok_or_else(|| format!("the table has no column “{k}”"))?;
                    vals.push((col, v));
                }
                conn.insert_row(&tab.schema, &tab.name, &vals).await.map_err(|e| e.to_string())
            }
            .await;
            if let Err(e) = result {
                self.xfer_done(String::new(), format!("Record {} failed: {e}\n{n} record(s) before it were inserted and kept.", n + 1));
                self.load_active().await;
                return;
            }
            if n % 50 == 0 {
                progress(format!("Inserted {n} of {total}…"));
            }
        }
        self.log_activity(None, &format!("IMPORT {total} rows into {}.{}", tab.schema, tab.name));
        self.xfer_done(format!("Done: {total} row(s) inserted into {}.", tab.name), String::new());
        self.load_active().await;
    }
}

// ---------------------------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------------------------

impl Worker {
    pub async fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::NewConn => self.new_conn(),
            Cmd::ParseConnUrl(u) => self.parse_conn_url(&u),
            Cmd::ConnFilter(q) => {
                self.conn_filter = q.to_lowercase();
                self.push_connections();
            }
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
            Cmd::UndoEntry(i) => self.undo_entry(i).await,
            Cmd::InspectorChanged(open, pinned) => self.inspector_changed(open, pinned),
            Cmd::SwitchDatabase(i) => self.switch_database(i).await,
            Cmd::SwitchSession(i) => self.switch_session(i),
            Cmd::CloseSession(i) => self.close_session(i),

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
            Cmd::CycleTab(d) => {
                let n = self.tabs.len() as i32;
                if n > 1 {
                    let cur = self.active.unwrap_or(0) as i32;
                    self.active = Some((cur + d).rem_euclid(n) as usize);
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
            Cmd::FirstPage => {
                if let Some(t) = self.active_mut().filter(|t| t.kind == Kind::Table && t.page.offset > 0) {
                    t.page.offset = 0;
                    self.load_active().await;
                }
            }
            Cmd::LastPage => self.last_page().await,
            Cmd::DraftSubmit(v) => {
                self.insert_vals = v;
                self.insert_submit().await;
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
            Cmd::CopySelection { r0, c0, r1, c1, mode } => self.copy_selection(r0, c0, r1, c1, mode),
            Cmd::PasteSelection { r0, c0, r1, c1 } => self.paste_selection(r0, c0, r1, c1),
            Cmd::EditNext(r, c, d) => self.edit_next(r, c, d),
            Cmd::OpenEditRow(r) => self.open_edit_row(r),
            Cmd::EditRowFieldEdited(i, v) => {
                if let Some(slot) = self.edit_row.as_mut().and_then(|er| er.values.get_mut(i)) {
                    *slot = Some(v);
                }
            }
            Cmd::EditRowSetNull(i, null) => {
                if let Some(er) = self.edit_row.as_mut() {
                    if let Some(slot) = er.values.get_mut(i) {
                        *slot = if null { None } else { Some(er.original.get(i).cloned().flatten().unwrap_or_default()) };
                    }
                }
                self.push_edit_row();
            }
            Cmd::EditRowSubmit => self.editrow_submit().await,
            Cmd::EditRowCancel => {
                self.edit_row = None;
                ui(&self.w, |st| st.set_editrow_open(false));
            }
            Cmd::OpenUsers => self.open_users().await,
            Cmd::UserSelect(i) => self.user_select(i).await,
            Cmd::UserCreate { name, host, password, level, admin } => self.user_create(name, host, password, level, admin).await,
            Cmd::UserSetLevel(i, l) => self.user_set_level(i, l).await,
            Cmd::UserPassword(i, pw) => self.user_password(i, pw).await,
            Cmd::UserDrop(i) => {
                if let Some(u) = self.users.get(i) {
                    let name = u.display();
                    self.ask(Pending::DropUser(i), format!("Drop user {name}"), format!("Remove the account {name}? Anyone or anything using it will lose access. This cannot be undone."));
                }
            }
            Cmd::UsersClose => ui(&self.w, |st| st.set_users_open(false)),
            Cmd::OpenTransfer(m) => self.open_transfer(m),
            Cmd::XferBrowse => self.xfer_browse().await,
            Cmd::XferRun { path, a, b } => self.xfer_run(path, a, b).await,
            Cmd::XferCancel => ui(&self.w, |st| st.set_xfer_open(false)),
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

            Cmd::Ctx(kind, i, j, x, y) => self.open_ctx(&kind, i, j, x, y),
            Cmd::CtxPick(a, rect) => self.ctx_pick(&a, rect).await,
            Cmd::CtxClose => {
                self.ctx_target = None;
                ui(&self.w, |st| st.set_ctx_open(false));
            }
            Cmd::ClearFlash => {
                for (r, c) in std::mem::take(&mut self.flashed) {
                    if let Some(v) = self.active_tab().and_then(|t| t.rows.get(r)).and_then(|row| row.get(c)).cloned() {
                        self.set_cell(r, c, v, 0);
                    }
                }
            }
            Cmd::ClearToast(id) => {
                if id == self.toast_id {
                    ui(&self.w, |st| st.set_toast("".into()));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Context menus
// ---------------------------------------------------------------------------------------------

fn item(label: &str, action: &str) -> (String, String, bool, bool) {
    (label.to_string(), action.to_string(), false, false)
}
fn danger(label: &str, action: &str) -> (String, String, bool, bool) {
    (label.to_string(), action.to_string(), false, true)
}
fn sep() -> (String, String, bool, bool) {
    (String::new(), String::new(), true, false)
}

impl Worker {
    fn open_ctx(&mut self, kind: &str, i: usize, j: usize, x: f32, y: f32) {
        let editable = self.active_tab().is_some_and(|t| t.kind == Kind::Table && t.editable);
        let is_table_tab = self.active_tab().is_some_and(|t| t.kind == Kind::Table);
        let relational = !self.is_mongo();
        let (target, items) = match kind {
            "tree" => {
                let Some(e) = self.tree.get(i) else { return };
                let items = match e.kind {
                    2 | 5 => vec![item("Open data", "open"), item("Open in new tab", "open-new"), item("Inspect structure", "structure"), item("Query…", "query"), item("Insert row…", "insert"), item("Import rows…", "import"), item("Export data…", "export"), sep(), item("Copy definition", "copy-def"), item("Copy name", "copy"), sep(), danger("Truncate…", "truncate"), danger("Drop…", "drop")],
                    3 | 4 => vec![item("Open data", "open"), item("Open in new tab", "open-new"), item("Inspect structure", "structure"), item("Query…", "query"), item("Export data…", "export"), sep(), item("Copy definition", "copy-def"), item("Copy name", "copy"), sep(), danger("Drop…", "drop")],
                    6 | 7 => vec![item("View definition", "definition"), item("Run in query tab", "run"), item("Copy definition", "copy-def"), item("Copy name", "copy")],
                    _ => vec![item("View definition", "definition"), item("Copy definition", "copy-def"), item("Copy name", "copy")],
                };
                (CtxTarget::Tree(i), items)
            }
            "tab" => {
                let Some(t) = self.tabs.get(i) else { return };
                (CtxTarget::Tab(i), vec![item(if t.pinned { "Unpin tab" } else { "Pin tab" }, "pin"), item("Duplicate tab", "duplicate"), sep(), item("Close tab", "close"), item("Close other tabs", "close-others")])
            }
            "cell" => {
                let mut v = vec![item("Copy", "copy"), item("Copy with column header", "copy-h")];
                if editable {
                    v.push(item("Paste", "paste"));
                }
                v.push(sep());
                v.push(item("Select whole row", "sel-row"));
                v.push(item("Select whole column", "sel-col"));
                if editable {
                    v.push(sep());
                    if relational {
                        v.push(item("Edit row…", "edit-row"));
                    }
                    v.push(item("Set to NULL", "null"));
                    v.push(item("Edit as JSON / text…", "json"));
                    v.push(sep());
                    v.push(danger("Delete row…", "delete"));
                }
                (CtxTarget::Cell(i, j), v)
            }
            "header" => {
                let mut v = vec![item("Copy column", "copy"), item("Copy column with header", "copy-h"), item("Copy column name", "copy-names"), item("Copy as CSV", "copy-csv"), item("Copy as JSON", "copy-json")];
                if editable {
                    v.push(item("Paste into column", "paste"));
                }
                if is_table_tab {
                    v.push(sep());
                    v.push(item("Sort ascending", "sort-asc"));
                    v.push(item("Sort descending", "sort-desc"));
                }
                (CtxTarget::Header(j), v)
            }
            "row" => {
                let mut v = vec![item("Copy row", "copy"), item("Copy row with header", "copy-h"), item("Copy as CSV", "copy-csv"), item("Copy as JSON", "copy-json")];
                if relational {
                    v.push(item("Copy as SQL INSERT", "copy-sql"));
                }
                if editable {
                    v.push(sep());
                    if relational {
                        v.push(item("Edit row…", "edit-row"));
                    }
                    v.push(danger("Delete row…", "delete"));
                }
                (CtxTarget::Row(i), v)
            }
            "dbmenu" => (
                CtxTarget::DbMenu,
                vec![item("Users & access…", "users"), sep(), item("Export database…", "export-db"), item("Import database…", "import-db"), item("Import rows into open table…", "import-rows"), sep(), item("Refresh metadata", "refresh")],
            ),
            _ => return,
        };
        self.ctx_target = Some(target);
        ui(&self.w, move |st| {
            let v: Vec<CtxItem> = items.into_iter().map(|(l, a, s, d)| CtxItem { label: l.into(), action: a.into(), sep: s, danger: d }).collect();
            st.set_ctx_items(ModelRc::new(VecModel::from(v)));
            st.set_ctx_x(x);
            st.set_ctx_y(y);
            st.set_ctx_open(true);
        });
    }

    async fn ctx_pick(&mut self, action: &str, rect: [i32; 4]) {
        ui(&self.w, |st| st.set_ctx_open(false));
        let Some(target) = self.ctx_target.take() else { return };
        let [r0, c0, r1, c1] = rect.map(|v| v.max(0) as usize);
        match target {
            CtxTarget::Tree(i) => self.tree_action(i, action).await,
            CtxTarget::Tab(i) => match action {
                "close" => self.close_tab(i as i32),
                other => self.tab_action(i, other).await,
            },
            CtxTarget::DbMenu => match action {
                "users" => self.open_users().await,
                "export-db" => self.open_transfer(0),
                "import-db" => self.open_transfer(1),
                "import-rows" => self.open_transfer(2),
                "refresh" => self.refresh().await,
                _ => {}
            },
            CtxTarget::Cell(r, c) => match action {
                "copy" => self.copy_selection(r0, c0, r1, c1, 0),
                "copy-h" => self.copy_selection(r0, c0, r1, c1, 1),
                "paste" => self.paste_selection(r0, c0, r1, c1),
                "sel-row" => ui(&self.w, move |st| st.invoke_select_row(r as i32, false)),
                "sel-col" => ui(&self.w, move |st| st.invoke_select_column(c as i32, false)),
                "edit-row" => self.open_edit_row(r),
                "null" => self.edit_cell(r, c, String::new(), true).await,
                "json" => self.open_json_cell(r, c),
                "delete" => self.ask_delete_row(r),
                _ => {}
            },
            CtxTarget::Header(c) => match action {
                "copy" => self.copy_selection(r0, c0, r1, c1, 0),
                "copy-h" => self.copy_selection(r0, c0, r1, c1, 1),
                "copy-names" => self.copy_selection(r0, c0, r1, c1, 5),
                "copy-csv" => self.copy_selection(r0, c0, r1, c1, 2),
                "copy-json" => self.copy_selection(r0, c0, r1, c1, 3),
                "paste" => self.paste_selection(r0, c0, r1, c1),
                "sort-asc" | "sort-desc" => {
                    let asc = action == "sort-asc";
                    let name = self.active_tab().and_then(|t| t.cols.get(c)).map(|c| c.name.clone());
                    if let (Some(name), Some(t)) = (name, self.active_mut().filter(|t| t.kind == Kind::Table)) {
                        t.page.sort_column = Some(name);
                        t.page.sort_ascending = asc;
                        t.page.offset = 0;
                        self.load_active().await;
                    }
                }
                _ => {}
            },
            CtxTarget::Row(r) => match action {
                "copy" => self.copy_selection(r0, c0, r1, c1, 0),
                "copy-h" => self.copy_selection(r0, c0, r1, c1, 1),
                "copy-csv" => self.copy_selection(r0, c0, r1, c1, 2),
                "copy-json" => self.copy_selection(r0, c0, r1, c1, 3),
                "copy-sql" => self.copy_selection(r0, c0, r1, c1, 4),
                "edit-row" => self.open_edit_row(r),
                "delete" => self.ask_delete_row(r),
                _ => {}
            },
        }
    }
}
