//! Pinned results and result comparison (part of the worker).

use super::*;
use std::collections::HashMap;

impl Worker {
    /// Copy the active result into a static tab that stays as it is.
    pub(crate) fn pin_result(&mut self) {
        let Some(t) = self.active_tab() else { return };
        if t.rows.is_empty() && t.cols.is_empty() {
            return self.toast("Nothing to pin yet.");
        }
        let mut p = Tab::new(Kind::Pinned, format!("Pinned: {}", t.title.trim_start_matches("Pinned: ")), self.default_page_size());
        p.cols = t.cols.clone();
        p.widths = t.widths.clone();
        p.rows = t.rows.clone();
        p.src_rows = t.rows.clone();
        p.editable = false;
        p.page_info = format!("{} row(s) pinned {}", t.rows.len(), hms());
        self.add_tab(p);
        self.show_active();
        self.toast("Pinned. Run again, then use “Compare with pinned”.");
    }

    /// Diff the active result against the newest pinned one: added and removed rows.
    pub(crate) fn compare_result(&mut self) {
        let Some(active) = self.active_tab() else { return };
        let Some(pinned) = self.tabs.iter().rev().find(|t| t.kind == Kind::Pinned && !t.title.starts_with("Diff")) else {
            return self.toast("Pin a result first, then run again and compare.");
        };
        if !active.cols.iter().map(|c| &c.name).eq(pinned.cols.iter().map(|c| &c.name)) {
            return self.toast("The columns differ from the pinned result, so they cannot be compared.");
        }
        let (rows, added, removed, same) = diff_rows(&pinned.rows, &active.rows);
        let mut cols = vec![ColMeta::plain("Δ")];
        cols.extend(active.cols.iter().cloned());
        let mut d = Tab::new(Kind::Pinned, format!("Diff: {} vs pinned", active.title), self.default_page_size());
        d.widths = auto_widths(&cols, &rows);
        d.cols = cols;
        d.src_rows = rows.clone();
        d.rows = rows;
        d.page_info = format!("{added} added (+), {removed} removed (−), {same} unchanged");
        self.add_tab(d);
        self.show_active();
    }
}

/// Multiset diff of `before` -> `after`: rows tagged "+" (only after) or "−" (only before).
pub(crate) fn diff_rows(before: &[Vec<Cell>], after: &[Vec<Cell>]) -> (Vec<Vec<Cell>>, usize, usize, usize) {
    let mut pool: HashMap<&Vec<Cell>, usize> = HashMap::new();
    for r in before {
        *pool.entry(r).or_default() += 1;
    }
    let mut out: Vec<Vec<Cell>> = Vec::new();
    let (mut added, mut same) = (0, 0);
    for r in after {
        match pool.get_mut(r) {
            Some(n) if *n > 0 => {
                *n -= 1;
                same += 1;
            }
            _ => {
                added += 1;
                let mut row = vec![Some("+".to_string())];
                row.extend(r.iter().cloned());
                out.push(row);
            }
        }
    }
    let mut removed = 0;
    for r in before {
        if let Some(n) = pool.get_mut(r) {
            if *n > 0 {
                *n -= 1;
                removed += 1;
                let mut row = vec![Some("−".to_string())];
                row.extend(r.iter().cloned());
                out.push(row);
            }
        }
    }
    (out, added, removed, same)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(a: &str, b: &str) -> Vec<Cell> {
        vec![Some(a.into()), Some(b.into())]
    }

    #[test]
    fn diff_reports_added_removed_and_unchanged() {
        let before = vec![r("1", "a"), r("2", "b"), r("3", "c")];
        let after = vec![r("1", "a"), r("2", "B"), r("4", "d")];
        let (rows, added, removed, same) = diff_rows(&before, &after);
        assert_eq!((added, removed, same), (2, 2, 1));
        assert!(rows.iter().any(|x| x[0].as_deref() == Some("+") && x[2].as_deref() == Some("B")));
        assert!(rows.iter().any(|x| x[0].as_deref() == Some("−") && x[2].as_deref() == Some("b")));
    }

    #[test]
    fn duplicates_are_counted_not_collapsed() {
        let (_, added, removed, same) = diff_rows(&[r("1", "a"), r("1", "a")], &[r("1", "a")]);
        assert_eq!((added, removed, same), (0, 1, 1));
    }
}

impl Worker {
    /// Open a script that makes connection `target` match the current one (tables and columns).
    pub(crate) async fn schema_diff(&mut self, target: usize) {
        if target == self.cur {
            return;
        }
        let Some(Some(other)) = self.parked.get(target) else { return };
        let Some(dst) = other.conn.as_ref() else { return };
        let Some(src) = self.conn.as_ref() else { return };
        if src.db_type() != dst.db_type() {
            return self.toast("Schema diff needs two connections of the same database type.");
        }
        if matches!(src.db_type(), DbType::Mongo | DbType::Sqlite) {
            return self.toast("Schema diff supports PostgreSQL and MySQL / MariaDB.");
        }
        let target_name = self.sess_meta.get(target).map(|m| m.0.clone()).unwrap_or_default();
        let diff = dboard_core::schemadiff::diff(&src.metadata.tables, &dst.metadata.tables);
        let mut ddls: HashMap<String, String> = HashMap::new();
        for t in &diff.create {
            let text = match self.conn.as_mut() {
                Some(c) => c.ddl(&t.schema, &t.name).await.unwrap_or_else(|e| format!("-- could not read the definition of {}: {e}", t.name)),
                None => String::new(),
            };
            ddls.insert(t.name.clone(), text);
        }
        let sql = dboard_core::schemadiff::render(&diff, self.dialect(), &|t| ddls.get(&t.name).cloned().unwrap_or_default());
        let mut tab = Tab::new(Kind::Routine, format!("Schema diff → {target_name}"), self.default_page_size());
        tab.ddl = sql;
        self.add_tab(tab);
        self.show_active();
        self.toast(if diff.is_empty() { "The schemas already match.".to_string() } else { format!("Script ready: run it on “{target_name}”.") });
    }
}

impl Worker {
    /// Show the local audit trail of write statements as a searchable, exportable grid.
    pub(crate) fn open_audit(&mut self) {
        let entries = self.store.read_audit(5000);
        let when = |ts: u64| chrono::DateTime::from_timestamp(ts as i64, 0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default();
        let rows: Vec<Vec<Cell>> = entries.iter().map(|e| vec![Some(when(e.at)), Some(e.connection.clone()), Some(e.environment.clone()), Some(one_line(&e.text))]).collect();
        let cols: Vec<ColMeta> = ["Time", "Connection", "Environment", "Statement"].iter().map(|n| ColMeta::plain(n)).collect();
        let idx = match self.tabs.iter().position(|t| t.kind == Kind::Pinned && t.title == "Audit log") {
            Some(i) => i,
            None => {
                self.add_tab(Tab::new(Kind::Pinned, "Audit log", self.default_page_size()));
                self.tabs.len() - 1
            }
        };
        let t = &mut self.tabs[idx];
        t.widths = auto_widths(&cols, &rows).into_iter().enumerate().map(|(i, w)| if i == 3 { w.clamp(300.0, 900.0) } else { w }).collect();
        t.cols = cols;
        t.page_info = format!("{} write statement(s) recorded on this computer (newest first). Use the filter row to search.", rows.len());
        t.src_rows = rows.clone();
        t.rows = rows;
        t.col_filters.clear();
        self.active = Some(idx);
        self.show_active();
    }
}
