//! Staged cell edits (part of the worker): queue edits, review old -> new, apply in one transaction.

use super::*;
use std::collections::HashSet;

fn show(c: &Cell) -> String {
    match c {
        None => "NULL".into(),
        Some(s) if s.chars().count() > 40 => format!("{}…", s.chars().take(40).collect::<String>()),
        Some(s) => format!("'{s}'"),
    }
}

impl Worker {
    pub(crate) fn stage_toggle(&mut self) {
        let Some(t) = self.active_mut().filter(|t| t.kind == Kind::Table) else { return };
        if t.stage && !t.staged.is_empty() {
            return self.toast("Save or discard the pending changes before switching to automatic edits.");
        }
        t.stage = !t.stage;
        let on = t.stage;
        self.show_active();
        self.toast(if on { "Edits now wait until you press Save." } else { "Edits are now applied immediately." });
    }

    /// Preference: do edits wait for Save (`on`) or get written as they are made? Open tabs follow,
    /// except ones that still hold pending changes, which must be saved or discarded first.
    pub(crate) fn set_save_mode(&mut self, on: bool) {
        self.settings.edits_need_save = on;
        self.persist_settings();
        let (protected, mongo) = (self.protected(), self.is_mongo());
        for t in self.tabs.iter_mut().filter(|t| t.kind == Kind::Table && t.staged.is_empty()) {
            t.stage = (on || protected) && !mongo;
        }
        ui(&self.w, move |st| st.set_save_mode(on));
        self.show_active();
        self.toast(if on { "Edits wait until you press Save." } else { "Edits are applied as you make them." });
    }

    /// Queue one edit; the grid shows the new value highlighted as pending.
    pub(crate) fn stage_edit(&mut self, r: usize, c: usize, new: Cell) {
        let Some(t) = self.active_mut() else { return };
        let Some(col) = t.cols.get(c).map(|c| c.name.clone()) else { return };
        let Some(row_now) = t.rows.get(r).cloned() else { return };
        let orig = t.orig_rows.entry(r).or_insert(row_now).clone();
        t.staged.retain(|s| !(s.orig == orig && s.c == c));
        if orig.get(c) != Some(&new) {
            t.staged.push(Staged { orig, c, col, new: new.clone() });
        }
        t.rows[r][c] = new.clone();
        let pending = t.staged.iter().any(|s| s.orig == t.orig_rows[&r] && s.c == c);
        let count = t.staged.len() as i32;
        self.set_cell(r, c, new, if pending { 4 } else { 0 });
        ui(&self.w, move |st| st.set_pending_count(count));
    }

    /// After (re)loading a table page, show staged values on the rows they belong to.
    pub(crate) fn overlay_staged(t: &mut Tab) {
        t.orig_rows.clear();
        if t.staged.is_empty() { return; }
        {
            // Reverse insertion preserves the first matching row when duplicates exist.
            let positions: std::collections::HashMap<&Vec<Cell>, usize> = t.rows.iter().enumerate().rev().map(|(i, r)| (r, i)).collect();
            for s in &t.staged {
                if let Some(&idx) = positions.get(&s.orig) {
                    t.orig_rows.entry(idx).or_insert_with(|| s.orig.clone());
                }
            }
        }
        let mut edits: std::collections::HashMap<&Vec<Cell>, Vec<&Staged>> = Default::default();
        for s in &t.staged { edits.entry(&s.orig).or_default().push(s); }
        for (idx, orig) in &t.orig_rows {
            if let Some(changes) = edits.get(orig) {
                for s in changes { t.rows[*idx][s.c] = s.new.clone(); }
            }
        }
    }

    pub(crate) fn pending_cells(t: &Tab) -> HashSet<(usize, usize)> {
        let mut out = HashSet::new();
        if t.staged.is_empty() { return out; }
        let mut edits: std::collections::HashMap<&Vec<Cell>, Vec<usize>> = Default::default();
        for s in &t.staged { edits.entry(&s.orig).or_default().push(s.c); }
        for (idx, orig) in &t.orig_rows {
            if let Some(columns) = edits.get(orig) {
                out.extend(columns.iter().map(|c| (*idx, *c)));
            }
        }
        out
    }

    pub(crate) fn review_open(&mut self) {
        let Some(t) = self.active_tab() else { return };
        let table = if t.schema.is_empty() { t.name.clone() } else { format!("{}.{}", t.schema, t.name) };
        let key_cols: Vec<usize> = t.cols.iter().enumerate().filter(|(_, c)| c.pk).map(|(i, _)| i).collect();
        let items: Vec<(String, String)> = t
            .staged
            .iter()
            .map(|s| {
                let who = if key_cols.is_empty() {
                    "row".to_string()
                } else {
                    key_cols.iter().filter_map(|i| s.orig.get(*i).map(|v| format!("{}={}", t.cols[*i].name, v.clone().unwrap_or_else(|| "NULL".into())))).collect::<Vec<_>>().join(", ")
                };
                (format!("{table} · {who} · {}", s.col), format!("{}  →  {}", show(&s.orig.get(s.c).cloned().flatten().into()), show(&s.new)))
            })
            .collect();
        if items.is_empty() {
            return;
        }
        ui(&self.w, move |st| {
            let v: Vec<EditItem> = items.into_iter().map(|(a, b)| EditItem { text: a.into(), detail: b.into(), can_undo: false }).collect();
            st.set_review_items(ModelRc::new(VecModel::from(v)));
            st.set_review_error("".into());
            st.set_review_open(true);
        });
    }

    pub(crate) fn review_discard(&mut self) {
        let Some(t) = self.active_mut() else { return };
        t.staged.clear();
        t.orig_rows.clear();
        ui(&self.w, |st| st.set_review_open(false));
        // reload to drop the shown edits
        let tx = self.tx.clone();
        let _ = tx.send(Cmd::Refresh);
    }

    pub(crate) async fn review_apply(&mut self) {
        let Some(i) = self.active else { return };
        let mut tab = self.tabs[i].request_snapshot();
        tab.staged = self.tabs[i].staged.clone();
        let sql = !self.is_mongo();
        if !self.hook_gate(&format!("UPDATE {}.{} ({} staged change(s))", tab.schema, tab.name, tab.staged.len())) {
            let msg = self.tabs.get(i).map(|t| t.banner.clone()).unwrap_or_default();
            ui(&self.w, move |st| st.set_review_error(msg.into()));
            return;
        }
        let Some(conn) = self.conn.as_mut() else { return };
        let own_tx = !conn.in_transaction() && sql;
        let fail = |w: &Weak<App>, msg: String| ui(w, move |st| st.set_review_error(msg.into()));
        if own_tx {
            if let Err(e) = conn.begin().await {
                return fail(&self.w, e.to_string());
            }
        }
        let mut running: std::collections::HashMap<&Vec<Cell>, Vec<Cell>> = Default::default();
        for s in &tab.staged {
            let now = running.entry(&s.orig).or_insert_with(|| s.orig.clone());
            if let Err(e) = conn.edit_cell(&tab.schema, &tab.name, now, &s.col, s.new.clone()).await {
                if own_tx {
                    let _ = conn.rollback().await;
                }
                return fail(&self.w, format!("{e} — nothing was changed."));
            }
            now[s.c] = s.new.clone();
        }
        if own_tx {
            if let Err(e) = conn.commit().await {
                return fail(&self.w, e.to_string());
            }
        }
        let n = tab.staged.len();
        if let Some(t) = self.active_mut() {
            t.staged.clear();
            t.orig_rows.clear();
        }
        ui(&self.w, |st| st.set_review_open(false));
        self.log_activity(None, &format!("UPDATE {}.{} ({n} staged change(s))", tab.schema, tab.name));
        self.load_active().await;
        self.toast(format!("{n} change(s) applied."));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(a: &str, b: &str) -> Vec<Cell> {
        vec![Some(a.into()), Some(b.into())]
    }

    #[test]
    fn staged_values_overlay_the_rows_they_belong_to() {
        let mut t = Tab::new(Kind::Table, "t", 100);
        t.rows = vec![row("1", "a"), row("2", "b")];
        t.staged = vec![Staged { orig: row("2", "b"), c: 1, col: "name".into(), new: Some("B!".into()) }];
        Worker::overlay_staged(&mut t);
        assert_eq!(t.rows[1], row("2", "B!"));
        assert_eq!(t.rows[0], row("1", "a"));
        assert_eq!(Worker::pending_cells(&t), HashSet::from([(1, 1)]));
    }

    #[test]
    fn staged_rows_missing_from_this_page_are_kept_but_not_shown() {
        let mut t = Tab::new(Kind::Table, "t", 100);
        t.rows = vec![row("1", "a")];
        t.staged = vec![Staged { orig: row("9", "z"), c: 1, col: "name".into(), new: None }];
        Worker::overlay_staged(&mut t);
        assert_eq!(t.staged.len(), 1);
        assert!(Worker::pending_cells(&t).is_empty());
        assert_eq!(t.rows[0], row("1", "a"));
    }

    #[test]
    fn several_edits_on_one_row_all_apply() {
        let mut t = Tab::new(Kind::Table, "t", 100);
        t.rows = vec![row("1", "a")];
        let o = row("1", "a");
        t.staged = vec![
            Staged { orig: o.clone(), c: 0, col: "id".into(), new: Some("5".into()) },
            Staged { orig: o, c: 1, col: "name".into(), new: Some("q".into()) },
        ];
        Worker::overlay_staged(&mut t);
        assert_eq!(t.rows[0], row("5", "q"));
    }

    #[test]
    fn duplicate_rows_overlay_only_the_first_match() {
        let mut t = Tab::new(Kind::Table, "t", 100);
        t.rows = vec![row("1", "a"), row("1", "a")];
        t.staged = vec![Staged { orig: row("1", "a"), c: 1, col: "name".into(), new: Some("edited".into()) }];
        Worker::overlay_staged(&mut t);
        assert_eq!(t.rows, vec![row("1", "edited"), row("1", "a")]);
        assert_eq!(Worker::pending_cells(&t), HashSet::from([(0, 1)]));
    }
}
