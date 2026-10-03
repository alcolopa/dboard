//! Dispatch (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------------------------

impl Worker {
    pub(crate) async fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::NewConn => self.new_conn(),
            Cmd::PickResult(i) => self.pick_result(i),
            Cmd::CheckUpdates => {
                self.toast("Checking for updates…");
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let r = crate::update::latest_release().map(|r| (r.tag, r.url));
                    let _ = tx.send(Cmd::UpdateResult(r));
                });
            }
            Cmd::UpdateResult(r) => match r {
                Ok((tag, url)) if crate::update::is_newer(&tag, env!("CARGO_PKG_VERSION")) => {
                    let _ = crate::clipboard::set(&url);
                    self.toast(format!("dboard {tag} is available. The download link was copied to your clipboard."));
                }
                Ok(_) => self.toast(format!("You are up to date (v{}).", env!("CARGO_PKG_VERSION"))),
                Err(e) => self.toast(format!("Could not check for updates: {e}")),
            },
            Cmd::OpenLink(url) => {
                if url.starts_with("https://") {
                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg(&url).spawn();
                    #[cfg(target_os = "windows")]
                    let _ = std::process::Command::new("cmd").args(["/C", "start", "", &url]).spawn();
                    #[cfg(all(unix, not(target_os = "macos")))]
                    let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
                }
            }
            Cmd::OpenEr => self.open_er().await,
            Cmd::ErOpen(s, n) => self.open_table(&s, &n).await,
            Cmd::ColFilter(c, text) => {
                if let Some(t) = self.active_mut().filter(|t| t.kind == Kind::Table) {
                    if t.col_filters.len() <= c {
                        t.col_filters.resize(c + 1, String::new());
                    }
                    t.col_filters[c] = text;
                    t.page.offset = 0;
                    self.apply_effective_filter();
                    self.load_active().await;
                }
            }
            Cmd::GotoFk(r, c) => self.goto_fk(r, c).await,
            Cmd::TxBegin => self.tx_action(0).await,
            Cmd::TxCommit => self.tx_action(1).await,
            Cmd::TxRollback => self.tx_action(2).await,
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
                    t.filter_text = f;
                    t.page.offset = 0;
                    self.apply_effective_filter();
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
            Cmd::RunSnippet(t) => self.run_snippet(t).await,
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

