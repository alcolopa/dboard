//! Undo (part of the worker).

use super::*;

// ---------------------------------------------------------------------------------------------
// Undo / history
// ---------------------------------------------------------------------------------------------

impl Worker {
    pub(crate) async fn after_undo(&mut self, r: dboard_core::edit::EditRecord) {
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

    pub(crate) async fn undo(&mut self) {
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
    pub(crate) async fn undo_entry(&mut self, shown: usize) {
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

    pub(crate) fn inspector_changed(&mut self, open: bool, pinned: bool) {
        self.settings.inspector_open = open;
        self.settings.inspector_pinned = pinned;
        self.persist_settings();
        ui(&self.w, move |st| {
            st.set_inspector_open(open);
            st.set_inspector_pinned(pinned);
        });
    }
}

