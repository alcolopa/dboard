//! Tabs (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Tabs & data
// ---------------------------------------------------------------------------------------------

impl Worker {
    pub(crate) fn dialect(&self) -> Dialect {
        match self.conn.as_ref().map(|c| c.db_type()) {
            Some(DbType::MySql) => Dialect::My,
            _ => Dialect::Pg,
        }
    }

    pub(crate) fn is_mongo(&self) -> bool {
        self.conn.as_ref().is_some_and(|c| c.db_type() == DbType::Mongo)
    }

    pub(crate) fn active_tab(&self) -> Option<&Tab> {
        self.active.and_then(|i| self.tabs.get(i))
    }

    pub(crate) fn active_mut(&mut self) -> Option<&mut Tab> {
        let i = self.active?;
        self.tabs.get_mut(i)
    }

    pub(crate) fn push_tabs(&self) {
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
    pub(crate) fn show_active(&self) {
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
                st.set_result_labels(strs(Vec::new()));
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
                st.set_result_labels(strs(t.results.iter().map(|r| r.label.clone()).collect()));
                st.set_result_index(t.result_idx as i32);
            }
        });
    }

    /// Change one displayed cell in place (value and 0 idle / 1 saving / 2 saved / 3 error flag).
    /// Unlike rebuilding the grid this keeps every other cell, including one being edited, intact.
    pub(crate) fn set_cell(&self, r: usize, c: usize, value: Cell, state: i32) {
        ui(&self.w, move |st| {
            let rows = st.get_rows();
            if let Some(row) = slint::Model::row_data(&rows, r) {
                if c < slint::Model::row_count(&row) {
                    slint::Model::set_row_data(&row, c, GridCell { is_null: value.is_none(), text: value.unwrap_or_default().into(), state });
                }
            }
        });
    }

    pub(crate) fn set_banner(&mut self, msg: &str, is_err: bool) {
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

    pub(crate) fn add_tab(&mut self, tab: Tab) {
        self.tabs.push(tab);
        self.active = Some(self.tabs.len() - 1);
    }

    pub(crate) fn default_page_size(&self) -> i64 {
        self.settings.default_page_size
    }

    pub(crate) async fn open_table(&mut self, schema: &str, name: &str) {
        self.open_table_in(schema, name, false).await
    }

    /// Open a table; with `new_tab` always in a fresh tab (e.g. to compare two sort orders or filters).
    pub(crate) async fn open_table_in(&mut self, schema: &str, name: &str, new_tab: bool) {
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

    pub(crate) async fn open_structure(&mut self, schema: &str, name: &str) {
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

    pub(crate) async fn open_routine(&mut self, o: DbObject) {
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

    pub(crate) fn select_star(&self, schema: &str, name: &str) -> String {
        if self.is_mongo() {
            return format!("db.{name}.find({{}}).limit(100)");
        }
        let d = self.dialect();
        let q = if schema.is_empty() { d.quote(name) } else { format!("{}.{}", d.quote(schema), d.quote(name)) };
        format!("SELECT * FROM {q} LIMIT 100;")
    }

    pub(crate) fn new_query_tab(&mut self, text: String) {
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
    pub(crate) async fn load_active(&mut self) {
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

    pub(crate) async fn load_table(&mut self, i: usize, tab: Tab) {
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

    pub(crate) async fn load_structure(&mut self, i: usize, tab: Tab) {
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

    pub(crate) fn close_tab(&mut self, idx: i32) {
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

    pub(crate) async fn tab_action(&mut self, i: usize, action: &str) {
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

    pub(crate) async fn reopen_tab(&mut self) {
        let Some(t) = self.closed.pop() else { return self.toast("No recently closed tabs") };
        self.add_tab(t);
        self.load_active().await;
    }

    pub(crate) async fn tree_click(&mut self, i: usize) {
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
    pub(crate) fn call_template(&self, o: &DbObject) -> String {
        let d = self.dialect();
        let name = format!("{}.{}", d.quote(&o.schema), d.quote(&o.name));
        let args: Vec<String> = o.detail.split(", ").filter(|a| !a.trim().is_empty()).map(|a| format!("/* {} */ NULL", a.trim())).collect();
        let args = args.join(", ");
        if o.kind == ObjectKind::Procedure { format!("CALL {name}({args});") } else { format!("SELECT {name}({args});") }
    }

    pub(crate) async fn tree_action(&mut self, i: usize, action: &str) {
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

    pub(crate) fn confirm_object(&mut self, p: Pending, verb: &str) {
        let (schema, name) = match &p {
            Pending::Truncate(s, n) | Pending::Drop(s, n) => (s.clone(), n.clone()),
            _ => return,
        };
        let text = format!("{verb} {}.{}? This cannot be undone.", schema, name);
        self.ask(p, format!("{verb} {name}"), text);
    }

    /// Open the confirmation dialog for `p`. Typed confirmation is required on protected connections.
    pub(crate) fn ask(&mut self, p: Pending, title: String, text: String) {
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

    pub(crate) fn copy_to_clipboard(&mut self, text: &str) {
        match crate::clipboard::set(text) {
            Ok(()) => self.toast("Copied to clipboard"),
            Err(e) => self.toast(format!("Clipboard unavailable: {e}")),
        }
    }
}

