// Hide the console window on Windows release builds.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

slint::include_modules!();

mod clipboard;
mod export;
mod highlight;
mod suggest;
mod update;
mod vars;
mod worker;

use dboard_core::config::Store;
use dboard_core::model::DbType;
use slint::ComponentHandle;
use tokio::sync::mpsc::unbounded_channel;
use worker::{Cmd, Worker};

fn main() {
    let app = App::new().expect("create window");
    let (tx, mut rx) = unbounded_channel::<Cmd>();
    let st = app.global::<AppState>();

    // Every UI callback is a one-liner that forwards to the worker thread.
    macro_rules! wire {
        ($on:ident, |$($a:ident),*| $cmd:expr) => {{
            let tx = tx.clone();
            st.$on(move |$($a),*| { let _ = tx.send($cmd); });
        }};
    }

    // Editor colouring runs synchronously on the UI thread so text never flashes invisible.
    st.on_highlight(|text| {
        let toks: Vec<HlToken> = highlight::tokens(text.as_str())
            .into_iter()
            .map(|t| HlToken { line: t.line as i32, col: t.col as i32, text: t.text.into(), kind: t.kind as i32 })
            .collect();
        slint::ModelRc::new(slint::VecModel::from(toks))
    });
    st.on_bracket_match(|text, off| {
        let v: Vec<i32> = highlight::match_bracket(text.as_str(), off.max(0) as usize)
            .map(|[a, b]| vec![a.0 as i32, a.1 as i32, b.0 as i32, b.1 as i32])
            .unwrap_or_default();
        slint::ModelRc::new(slint::VecModel::from(v))
    });
    st.on_count_lines(|t| t.as_str().split('\n').count() as i32);
    st.on_gutter(|n| (1..=n.max(1)).map(|i| i.to_string()).collect::<Vec<_>>().join("\n").into());
    wire!(on_vars_edited, |i, v| Cmd::VarsEdited(i.max(0) as usize, v.to_string()));
    wire!(on_vars_submit, | | Cmd::VarsSubmit);
    wire!(on_vars_cancel, | | Cmd::VarsCancel);
    wire!(on_new_conn, | | Cmd::NewConn);
    wire!(on_col_filter, |c, t| Cmd::ColFilter(c.max(0) as usize, t.to_string()));
    wire!(on_goto_fk, |r, c| Cmd::GotoFk(r.max(0) as usize, c.max(0) as usize));
    wire!(on_open_er, | | Cmd::OpenEr);
    wire!(on_er_open, |s, n| Cmd::ErOpen(s.to_string(), n.to_string()));
    wire!(on_check_updates, | | Cmd::CheckUpdates);
    wire!(on_open_link, |u| Cmd::OpenLink(u.to_string()));
    wire!(on_pick_result, |i| Cmd::PickResult(i.max(0) as usize));
    wire!(on_tx_begin, | | Cmd::TxBegin);
    wire!(on_tx_commit, | | Cmd::TxCommit);
    wire!(on_tx_rollback, | | Cmd::TxRollback);
    wire!(on_parse_conn_url, |u| Cmd::ParseConnUrl(u.to_string()));
    wire!(on_conn_filter, |q| Cmd::ConnFilter(q.to_string()));
    wire!(on_select_conn, |id| Cmd::SelectConn(id.to_string()));
    wire!(on_save_conn, |f| Cmd::SaveConn(f));
    wire!(on_test_conn, |f| Cmd::TestConn(f));
    wire!(on_connect_conn, |f| Cmd::ConnectConn(f));
    wire!(on_delete_conn, |id| Cmd::DeleteConn(id.to_string()));
    wire!(on_duplicate_conn, |id| Cmd::DuplicateConn(id.to_string()));
    wire!(on_disconnect, | | Cmd::Disconnect);
    wire!(on_refresh, | | Cmd::Refresh);
    wire!(on_undo, | | Cmd::Undo);
    wire!(on_undo_entry, |i| Cmd::UndoEntry(i as usize));
    wire!(on_inspector_changed, |o, p| Cmd::InspectorChanged(o, p));
    wire!(on_switch_database, |i| Cmd::SwitchDatabase(i as usize));
    wire!(on_switch_session, |i| Cmd::SwitchSession(i.max(0) as usize));
    wire!(on_close_session, |i| Cmd::CloseSession(i));
    wire!(on_first_page, | | Cmd::FirstPage);
    wire!(on_last_page, | | Cmd::LastPage);
    wire!(on_draft_submit, |v| Cmd::DraftSubmit(slint::Model::iter(&v).map(|s| s.to_string()).collect()));
    wire!(on_cycle_tab, |d| Cmd::CycleTab(d));
    wire!(on_copy_selection, |a, b, c, d, m| Cmd::CopySelection { r0: a.max(0) as usize, c0: b.max(0) as usize, r1: c.max(0) as usize, c1: d.max(0) as usize, mode: m });
    wire!(on_paste_selection, |a, b, c, d| Cmd::PasteSelection { r0: a.max(0) as usize, c0: b.max(0) as usize, r1: c.max(0) as usize, c1: d.max(0) as usize });
    wire!(on_edit_next, |r, c, d| Cmd::EditNext(r.max(0) as usize, c.max(0) as usize, d));
    wire!(on_open_edit_row, |r| Cmd::OpenEditRow(r.max(0) as usize));
    wire!(on_editrow_field_edited, |i, v| Cmd::EditRowFieldEdited(i as usize, v.to_string()));
    wire!(on_editrow_set_null, |i, n| Cmd::EditRowSetNull(i as usize, n));
    wire!(on_editrow_submit, | | Cmd::EditRowSubmit);
    wire!(on_editrow_cancel, | | Cmd::EditRowCancel);
    wire!(on_open_users, | | Cmd::OpenUsers);
    wire!(on_user_select, |i| Cmd::UserSelect(i.max(0) as usize));
    wire!(on_user_create, |n, h, p, l, a| Cmd::UserCreate { name: n.to_string(), host: h.to_string(), password: p.to_string(), level: l, admin: a });
    wire!(on_user_set_level, |i, l| Cmd::UserSetLevel(i.max(0) as usize, l));
    wire!(on_user_password, |i, p| Cmd::UserPassword(i.max(0) as usize, p.to_string()));
    wire!(on_user_drop, |i| Cmd::UserDrop(i.max(0) as usize));
    wire!(on_users_close, | | Cmd::UsersClose);
    wire!(on_open_transfer, |m| Cmd::OpenTransfer(m));
    wire!(on_xfer_browse, | | Cmd::XferBrowse);
    wire!(on_xfer_run, |p, a, b| Cmd::XferRun { path: p.to_string(), a, b });
    wire!(on_xfer_cancel, | | Cmd::XferCancel);
    st.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
    app.global::<Theme>().set_mod(if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" }.into());
    wire!(on_filter_tree, |f| Cmd::FilterTree(f.to_string()));
    wire!(on_tree_click, |i| Cmd::TreeClick(i as usize));
    wire!(on_tree_action, |i, a| Cmd::TreeAction(i as usize, a.to_string()));
    wire!(on_new_query_tab, | | Cmd::NewQueryTab);
    wire!(on_activate_tab, |i| Cmd::ActivateTab(i as usize));
    wire!(on_close_tab, |i| Cmd::CloseTab(i));
    wire!(on_tab_action, |i, a| Cmd::TabAction(i as usize, a.to_string()));
    wire!(on_reopen_tab, | | Cmd::ReopenTab);
    wire!(on_edit_cell, |r, c, t, n| Cmd::EditCell(r as usize, c as usize, t.to_string(), n));
    wire!(on_toggle_bool, |r, c| Cmd::ToggleBool(r as usize, c as usize));
    wire!(on_open_json_cell, |r, c| Cmd::OpenJsonCell(r as usize, c as usize));
    wire!(on_sort_by, |c| Cmd::SortBy(c as usize));
    wire!(on_col_resized, |i, w| Cmd::ColResized(i as usize, w));
    wire!(on_apply_filter, |f| Cmd::ApplyFilter(f.to_string()));
    wire!(on_next_page, | | Cmd::NextPage);
    wire!(on_prev_page, | | Cmd::PrevPage);
    wire!(on_set_page_size, |i| Cmd::SetPageSize(i as usize));
    wire!(on_copy_text, |t| Cmd::CopyText(t.to_string()));
    wire!(on_cell_copy, |r, c| Cmd::CellCopy(r as usize, c as usize));
    wire!(on_delete_row, |r| Cmd::DeleteRow(r as usize));
    wire!(on_open_insert, | | Cmd::OpenInsert);
    wire!(on_open_doc, |r| Cmd::OpenDoc(r as usize));
    wire!(on_open_export, | | Cmd::OpenExport);
    wire!(on_query_edited, |t| Cmd::QueryEdited(t.to_string()));
    wire!(on_apply_suggestion, |s| Cmd::ApplySuggestion(s.to_string()));
    // "Run selection": runs on the UI thread so the clipboard round-trip is synchronous.
    {
        use std::sync::Mutex;
        const MARK: &str = "\u{1}dboard-selection-probe\u{1}";
        static SAVED: Mutex<Option<String>> = Mutex::new(None);
        st.on_stash_clipboard(|| {
            *SAVED.lock().unwrap() = clipboard::get().ok();
            let _ = clipboard::set(MARK);
        });
        let tx = tx.clone();
        st.on_run_selection(move |full| {
            let got = clipboard::get().unwrap_or_default();
            if let Some(old) = SAVED.lock().unwrap().take() {
                let _ = clipboard::set(&old);
            }
            let text = if got.trim().is_empty() || got == MARK { full.to_string() } else { got };
            let _ = tx.send(Cmd::RunSnippet(text));
        });
    }
    wire!(on_run_query, |t| Cmd::RunQuery(t.to_string()));
    wire!(on_explain_query, |t, a| Cmd::ExplainQuery(t.to_string(), a));
    wire!(on_insert_template, |t| Cmd::InsertTemplate(t.to_string()));
    wire!(on_open_save_query, | | Cmd::OpenSaveQuery);
    wire!(on_clear_activity, | | Cmd::ClearActivity);
    wire!(on_load_history, |i| Cmd::LoadHistory(i as usize));
    wire!(on_load_saved, |i| Cmd::LoadSaved(i as usize));
    wire!(on_delete_saved, |i| Cmd::DeleteSaved(i as usize));
    wire!(on_clear_history, | | Cmd::ClearHistory);
    wire!(on_confirm_run, | | Cmd::ConfirmRun);
    wire!(on_confirm_cancel, | | Cmd::ConfirmCancel);
    wire!(on_insert_field_edited, |i, v| Cmd::InsertFieldEdited(i as usize, v.to_string()));
    wire!(on_insert_submit, | | Cmd::InsertSubmit);
    wire!(on_insert_cancel, | | Cmd::InsertCancel);
    wire!(on_json_save, |t| Cmd::JsonSave(t.to_string()));
    wire!(on_json_format, |t| Cmd::JsonFormat(t.to_string()));
    wire!(on_json_cancel, | | Cmd::JsonCancel);
    wire!(on_saveq_submit, |n, f| Cmd::SaveQuerySubmit(n.to_string(), f as usize));
    wire!(on_saveq_cancel, | | Cmd::SaveQueryCancel);
    wire!(on_open_palette, |s| Cmd::OpenPalette(s));
    wire!(on_palette_changed, |q| Cmd::PaletteChanged(q.to_string()));
    wire!(on_palette_run, |i| Cmd::PaletteRun(i as usize));
    wire!(on_palette_close, | | Cmd::PaletteClose);
    wire!(on_export_copy, |f, h| Cmd::ExportCopy(f as usize, h));
    wire!(on_export_save, |f, h| Cmd::ExportSave(f as usize, h));
    wire!(on_export_cancel, | | Cmd::ExportCancel);
    wire!(on_open_settings, | | Cmd::OpenSettings);
    wire!(on_settings_changed, |d, c, p, cf, ac, f| Cmd::SettingsChanged { dark: d, compact: c, page_idx: p as usize, confirm: cf, autocomplete: ac, font: f });
    wire!(on_settings_close, | | Cmd::SettingsClose);
    wire!(on_clear_credentials, | | Cmd::ClearCredentials);
    wire!(on_ctx, |k, i, j, x, y| Cmd::Ctx(k.to_string(), i as usize, j as usize, x, y));
    wire!(on_ctx_pick, |a, r0, c0, r1, c1| Cmd::CtxPick(a.to_string(), [r0, c0, r1, c1]));
    wire!(on_ctx_close, | | Cmd::CtxClose);

    // Switching the engine in the form keeps the port in sync unless the user typed their own.
    {
        let weak = app.as_weak();
        st.on_type_changed(move |idx| {
            let Some(app) = weak.upgrade() else { return };
            let st = app.global::<AppState>();
            let mut form = st.get_form();
            let is_default = form.port.is_empty() || DbType::ALL.iter().any(|t| form.port.as_str() == t.default_port().to_string());
            if is_default {
                form.port = DbType::ALL[(idx.max(0) as usize).min(2)].default_port().to_string().into();
            }
            st.set_form(form);
        });
    }

    let weak = app.as_weak();
    let worker_tx = tx.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
        rt.block_on(async move {
            let mut w = Worker::new(weak, worker_tx, Store::open_default());
            w.init();
            w.restore_sessions().await;
            while let Some(cmd) = rx.recv().await {
                w.handle(cmd).await;
            }
        });
    });

    app.run().expect("run event loop");
}

#[cfg(test)]
mod ui_tests {
    use super::*;

    /// The whole window builds headlessly and the state bindings the worker relies on behave.
    #[test]
    fn window_builds_and_state_defaults_are_sane() {
        i_slint_backend_testing::init_no_event_loop();
        let app = App::new().expect("window builds");
        let st = app.global::<AppState>();
        assert!(!st.get_connected());
        assert!(!st.get_in_tx());
        assert_eq!(slint::Model::row_count(&st.get_sessions()), 0);

        // Callbacks the Rust side wires must exist and be invocable without a worker attached.
        st.invoke_new_conn();
        st.invoke_conn_filter("x".into());
    }

    #[test]
    fn edit_menu_targets_grid_when_no_text_field_is_focused() {
        i_slint_backend_testing::init_no_event_loop();
        let app = App::new().unwrap();
        let st = app.global::<AppState>();
        let copied = std::rc::Rc::new(std::cell::Cell::new(false));
        let flag = copied.clone();
        st.on_copy_selection(move |_, _, _, _, _| flag.set(true));
        st.set_sel_kind(1);
        st.invoke_edit(1); // Copy
        assert!(copied.get());
        // With a text field focused the grid is left alone.
        copied.set(false);
        st.set_text_focus(1);
        st.invoke_edit(1);
        assert!(!copied.get());
    }
}
