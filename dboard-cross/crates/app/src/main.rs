// Hide the console window on Windows release builds.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

slint::include_modules!();

mod clipboard;
mod export;
mod suggest;
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

    wire!(on_new_conn, | | Cmd::NewConn);
    wire!(on_select_conn, |id| Cmd::SelectConn(id.to_string()));
    wire!(on_save_conn, |f| Cmd::SaveConn(f));
    wire!(on_test_conn, |f| Cmd::TestConn(f));
    wire!(on_connect_conn, |f| Cmd::ConnectConn(f));
    wire!(on_delete_conn, |id| Cmd::DeleteConn(id.to_string()));
    wire!(on_duplicate_conn, |id| Cmd::DuplicateConn(id.to_string()));
    wire!(on_disconnect, | | Cmd::Disconnect);
    wire!(on_refresh, | | Cmd::Refresh);
    wire!(on_undo, | | Cmd::Undo);
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
            while let Some(cmd) = rx.recv().await {
                w.handle(cmd).await;
            }
        });
    });

    app.run().expect("run event loop");
}
