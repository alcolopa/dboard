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
                    let r = crate::update::latest_release().map(|r| (r.tag, r.url, r.assets));
                    let _ = tx.send(Cmd::UpdateResult(r));
                });
            }
            Cmd::UpdateResult(r) => match r {
                Ok((tag, url, assets)) if crate::update::is_newer(&tag, env!("CARGO_PKG_VERSION")) => {
                    let arch = std::env::consts::ARCH;
                    let os = match std::env::consts::OS {
                        "macos" => "macos",
                        "windows" => "windows",
                        _ => "linux",
                    };
                    match crate::update::pick_asset(&assets, os, arch) {
                        Some((name, dl, size)) => {
                            let text = format!("dboard {tag} is available (you have v{}). Download {name} ({:.1} MB) to your Downloads folder?", env!("CARGO_PKG_VERSION"), *size as f64 / 1_048_576.0);
                            self.ask_plain(Pending::DownloadUpdate(dl.clone(), name.clone()), "Update available".into(), text);
                        }
                        None => {
                            let _ = crate::clipboard::set(&url);
                            self.toast(format!("dboard {tag} is available. No installer matches this computer; the release page link was copied."));
                        }
                    }
                }
                Ok(_) => self.toast(format!("You are up to date (v{}).", env!("CARGO_PKG_VERSION"))),
                Err(e) => self.toast(format!("Could not check for updates: {e}")),
            },
            Cmd::UpdateDownloaded(r) => match r {
                Ok(path) => {
                    self.toast(format!("Downloaded {}. Quit dboard and open it to finish updating.", path.display()));
                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
                    #[cfg(target_os = "windows")]
                    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
                    #[cfg(all(unix, not(target_os = "macos")))]
                    if let Some(dir) = path.parent() {
                        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
                    }
                }
                Err(e) => self.toast(format!("Download failed: {e}")),
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
            Cmd::ExportEr => self.export_er().await,
            Cmd::ErOpen(s, n) => self.open_table(&s, &n).await,
            Cmd::ColFilter(c, text) if self.active_tab().is_some_and(|t| t.kind == Kind::Pinned) => {
                if let Some(t) = self.active_mut() {
                    if t.col_filters.len() <= c {
                        t.col_filters.resize(c + 1, String::new());
                    }
                    t.col_filters[c] = text;
                    let needles: Vec<String> = t.col_filters.iter().map(|f| f.trim().to_lowercase()).collect();
                    t.rows = t.src_rows.iter().filter(|r| needles.iter().enumerate().all(|(i, n)| n.is_empty() || r.get(i).and_then(|c| c.as_deref()).is_some_and(|v| v.to_lowercase().contains(n)))).cloned().collect();
                    t.page_info = format!("{} of {} row(s)", t.rows.len(), t.src_rows.len());
                }
                self.show_active();
            }
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
            Cmd::SetTimeout(secs) => {
                self.settings.statement_timeout_secs = secs;
                self.persist_settings();
                ui(&self.w, move |st| st.set_timeout_secs(secs as i32));
                if let Some(c) = self.conn.as_mut() {
                    if let Err(e) = c.set_statement_timeout(secs as u64 * 1000).await {
                        return self.toast(format!("Could not set the timeout: {e}"));
                    }
                }
                self.toast(if secs == 0 { "Query timeout off.".to_string() } else { format!("Queries stop after {secs} s.") });
            }
            Cmd::StageToggle => self.stage_toggle(),
            Cmd::SetSaveMode(on) => self.set_save_mode(on),
            Cmd::ReviewOpen => self.review_open(),
            Cmd::ReviewApply => self.review_apply().await,
            Cmd::ReviewCancel => ui(&self.w, |st| st.set_review_open(false)),
            Cmd::ReviewDiscard => self.review_discard(),
            Cmd::PinResult => self.pin_result(),
            Cmd::CompareResult => self.compare_result(),
            Cmd::XferPreview(p, h) => self.xfer_preview(p, h),
            Cmd::XferMapPick(i, j) => self.xfer_map_pick(i, j),
            Cmd::GenerateRows(n) => self.generate_rows(n, false).await,
            Cmd::OpenAudit => self.open_audit(),
            Cmd::BackupNow => self.backup_current(true).await,
            Cmd::BackupTick => self.backup_due().await,
            Cmd::SetBackup(h, k) => {
                self.settings.backup_every_hours = h;
                self.settings.backup_keep = k;
                self.persist_settings();
                self.push_settings();
                self.toast(if h == 0 { "Automatic backups off.".to_string() } else { format!("Backing up every {h} h while dboard is open, keeping the last {k}.") });
            }
            Cmd::OpenHooks => {
                let path = self.store.hooks_path();
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg(&path).spawn();
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(&path).spawn();
                #[cfg(all(unix, not(target_os = "macos")))]
                let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
                self.toast(format!("Hooks file: {}", path.display()));
            }
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
            Cmd::UserSelect(i, d) => self.user_select(i, &d).await,
            Cmd::UserCreate { name, host, password, level, admin, database } => self.user_create(name, host, password, level, admin, database).await,
            Cmd::UserSetLevel(i, l, d) => self.user_set_level(i, l, d).await,
            Cmd::UserSetTable(i, l, d, t) => self.user_set_table(i, l, d, t).await,
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
            Cmd::XferFormat(f, path) => self.xfer_set_format(f, path),
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

            Cmd::ConfirmRun(force) => {
                self.force = force;
                self.confirm_run().await;
            }
            Cmd::TreeSelect(i, mode) => self.tree_select(i, mode),
            Cmd::TreeClearSel => self.tree_clear_selection(),
            Cmd::TreeDropSelected => self.ask_drop_selected(),
            Cmd::TreeSelectAll => self.tree_select_all(),
            Cmd::TreeDropAll => self.ask_drop_all(),
            Cmd::NewDbRequest => {
                ui(&self.w, |st| {
                    st.set_new_db_name("".into());
                    st.set_new_db_open(true);
                });
            }
            Cmd::NewDbCancel => ui(&self.w, |st| st.set_new_db_open(false)),
            Cmd::NewDbSubmit(name) => self.create_database(name).await,
            Cmd::ConfirmCancel => {
                self.pending = None;
                ui(&self.w, |st| st.set_confirm_open(false));
            }
            Cmd::VarsEdited(i, v) => {
                if let Some((_, names)) = &self.var_pending {
                    if let Some(name) = names.get(i) {
                        self.var_values.insert(name.clone(), v);
                    }
                }
            }
            Cmd::VarsSubmit => {
                if let Some((text, _)) = self.var_pending.take() {
                    ui(&self.w, |st| st.set_vars_open(false));
                    let resolved = crate::vars::substitute(&text, &self.var_values);
                    self.run_guarded(resolved).await;
                }
            }
            Cmd::VarsCancel => {
                self.var_pending = None;
                ui(&self.w, |st| st.set_vars_open(false));
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

