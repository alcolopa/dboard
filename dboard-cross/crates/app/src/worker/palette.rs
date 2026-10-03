//! Palette (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Palette, export, settings, inspector
// ---------------------------------------------------------------------------------------------

pub(crate) const COMMANDS: [(&str, &str, &str); 14] = [
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
    pub(crate) fn build_palette(&mut self, q: &str) {
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

    pub(crate) fn open_palette(&mut self, search: bool) {
        self.palette_search = search;
        ui(&self.w, move |st| {
            st.set_palette_search_mode(search);
            st.set_palette_query("".into());
            st.set_palette_open(true);
        });
        self.build_palette("");
    }

    pub(crate) async fn palette_run(&mut self, i: usize) {
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

    pub(crate) async fn switch_connection(&mut self, id: &str) {
        let Some(cfg) = self.connections.iter().find(|c| c.id == id).cloned() else { return };
        let pw = if cfg.remember_password { secrets::get(&cfg).unwrap_or_default() } else { String::new() };
        self.select_conn(id);
        ui(&self.w, |st| st.set_busy(true));
        match Conn::connect(cfg.clone(), &pw).await {
            Ok(conn) => self.on_connected(cfg, conn, pw).await,
            Err(e) => self.form_error(e.to_string()),
        }
    }

    pub(crate) fn toggle_inspector(&mut self) {
        let (open, pinned) = (!self.settings.inspector_open, self.settings.inspector_pinned);
        self.inspector_changed(open, pinned);
    }

    pub(crate) async fn refresh(&mut self) {
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

    pub(crate) fn push_inspector(&self) {
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

    pub(crate) fn open_export(&mut self) {
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

    pub(crate) fn export_payload(&self, format: usize, headers: bool) -> Option<(String, String)> {
        let t = self.active_tab()?;
        let cols: Vec<ExportCol> = t.cols.iter().map(|c| ExportCol { name: c.name.clone(), type_name: c.type_name.clone() }).collect();
        let d = self.export_dialect;
        let table = self.export_table_name(t);
        let text = export::format(format, &cols, &t.rows, headers, &table, d);
        let base = sanitize(if t.name.is_empty() { &t.title } else { &t.name });
        Some((text, format!("{base}-{}.{}", chrono::Local::now().format("%Y%m%d-%H%M%S"), export::extension(format))))
    }

    pub(crate) fn export_copy(&mut self, format: usize, headers: bool) {
        if let Some((text, _)) = self.export_payload(format, headers) {
            self.copy_to_clipboard(&text);
            ui(&self.w, |st| st.set_export_open(false));
        }
    }

    pub(crate) fn export_save(&mut self, format: usize, headers: bool) {
        let Some((text, name)) = self.export_payload(format, headers) else { return };
        let path = downloads_dir().join(name);
        let written = if format == 3 {
            let Some(t) = self.active_tab() else { return };
            let cols: Vec<ExportCol> = t.cols.iter().map(|c| ExportCol { name: c.name.clone(), type_name: c.type_name.clone() }).collect();
            export::xlsx(&cols, &t.rows, headers, &path)
        } else {
            std::fs::write(&path, text).map_err(|e| e.to_string())
        };
        match written {
            Ok(()) => {
                ui(&self.w, |st| st.set_export_open(false));
                self.toast(format!("Saved {}", path.display()));
                self.log_activity(None, &format!("EXPORT {}", path.display()));
            }
            Err(e) => self.toast(format!("Could not save: {e}")),
        }
    }

    // ---- settings --------------------------------------------------------------------------

    pub(crate) fn settings_changed(&mut self, dark: bool, compact: bool, page_idx: usize, confirm: bool, autocomplete: bool, font: i32) {
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

    pub(crate) fn clear_credentials(&mut self) {
        for c in &self.connections {
            secrets::delete(c);
        }
        let n = self.connections.len();
        ui(&self.w, move |st| st.set_set_info(format!("Removed saved passwords for {n} connection(s).").into()));
    }
}

