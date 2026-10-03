//! The worker thread: owns the database connection and all persisted state, and talks to the
//! UI thread only through `ui()` (updates) and `Cmd` (requests).

use crate::export::{self, ExportCol};
use crate::suggest;
use crate::{App, AppState, ColInfo, ConnForm, ConnItem, CtxItem, EditItem, ErBox, FieldItem, GridCell, HistoryItem, MetaRow, PaletteItem, SavedItem, SessionTab, TabInfo, TreeItem, UserRow};
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
    TxBegin,
    SetTimeout(u32),
    ColFilter(usize, String),
    GotoFk(usize, usize),
    OpenEr,
    ErOpen(String, String),
    CheckUpdates,
    OpenLink(String),
    UpdateResult(Result<(String, String), String>),
    PickResult(usize),
    TxCommit,
    TxRollback,
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
    /// Run part of the editor text without replacing what is in the editor.
    RunSnippet(String),
    ExplainQuery(String, bool),
    InsertTemplate(String),
    OpenSaveQuery,
    // dialogs
    ConfirmRun,
    ConfirmCancel,
    VarsEdited(usize, String),
    VarsSubmit,
    VarsCancel,
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
pub(crate) struct ColMeta {
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
pub(crate) enum Kind {
    Table = 0,
    Query = 1,
    Structure = 2,
    Routine = 3,
    Diagram = 4,
}

#[derive(Clone)]
pub(crate) struct Tab {
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
    /// One entry per statement when a query tab ran a multi-statement script.
    results: Vec<ResultSet>,
    result_idx: usize,
    er: ErLayout,
    /// Per-column "contains" filters from the filter row, by column index.
    col_filters: Vec<String>,
}

/// Pre-computed ER diagram geometry (boxes, FK lines, canvas size).
#[derive(Clone, Default)]
pub(crate) struct ErLayout {
    boxes: Vec<(f32, f32, f32, f32, String, String, String, String)>,
    lines: Vec<String>,
    size: (f32, f32),
}

#[derive(Clone)]
pub(crate) struct ResultSet {
    label: String,
    cols: Vec<ColMeta>,
    widths: Vec<f32>,
    rows: Vec<Vec<Cell>>,
    info: String,
    timing: String,
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
            results: Vec::new(),
            result_idx: 0,
            er: ErLayout::default(),
            col_filters: Vec::new(),
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

pub(crate) enum Pending {
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
pub(crate) struct EditRowState {
    row: usize,
    original: Vec<Cell>,
    values: Vec<Cell>,
}

pub(crate) enum JsonTarget {
    Cell(usize, usize),
    Doc(Vec<Cell>),
    NewDoc,
}

pub(crate) enum CtxTarget {
    Tree(usize),
    Tab(usize),
    Session(usize),
    Cell(usize, usize),
    Header(usize),
    Row(usize),
    DbMenu,
}

pub(crate) enum PaletteAction {
    Command(&'static str),
    OpenTable(String, String),
    Column(String, String),
    Object(DbObject),
    Saved(usize),
    Connection(String),
}

#[derive(Clone)]
pub(crate) struct TreeEntry {
    kind: i32,
    schema: String,
    name: String,
    /// Routine arguments, owning table, ... (see `DbObject::detail`).
    detail: String,
    key: String,
}

/// Everything that belongs to one open connection. The active one lives directly in `Worker`;
/// the others are parked here and swapped in when their tab is picked.
pub(crate) struct Session {
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
    /// Values typed for `{{variables}}`, remembered for the next run, and the run waiting on them.
    var_values: std::collections::HashMap<String, String>,
    var_pending: Option<(String, Vec<String>)>,
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
    sess_ids: Vec<String>,
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
            var_values: std::collections::HashMap::new(),
            var_pending: None,
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
            sess_ids: Vec::new(),
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
                st.set_timeout_secs(s.statement_timeout_secs as i32);
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
        // Grouped by environment (Production first), most recently used first within a group.
        list.sort_by(|a, b| {
            a.environment.index().cmp(&b.environment.index()).then(b.last_used.cmp(&a.last_used)).then(a.display_name().to_lowercase().cmp(&b.display_name().to_lowercase()))
        });
        let mut items: Vec<(String, String, String, u32, bool, bool)> = Vec::new();
        let mut last_env = None;
        for c in &list {
            if last_env != Some(c.environment) {
                last_env = Some(c.environment);
                items.push((String::new(), c.environment.label().to_string(), String::new(), c.environment.color(), false, true));
            }
            let who = if c.username.is_empty() { c.host.clone() } else { format!("{}@{}", c.username, c.host) };
            items.push((c.id.clone(), c.display_name(), format!("{} • {}", c.db_type.label(), who), c.environment.color(), c.id == sel, false));
        }
        ui(&self.w, move |st| {
            let v: Vec<ConnItem> = items
                .into_iter()
                .map(|(id, name, sub, col, active, header)| ConnItem {
                    id: id.into(),
                    name: name.into(),
                    subtitle: sub.into(),
                    color: slint::Color::from_rgb_u8((col >> 16) as u8, (col >> 8) as u8, col as u8),
                    active,
                    header,
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

mod connections;
mod tree;
mod tabs;
mod editing;
mod query;
pub(crate) use query::CANCEL;
mod palette;
mod selection;
mod undo;
mod users;
mod transfer;
mod dispatch;
mod context;
