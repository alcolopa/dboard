//! Pinned results and result comparison (part of the worker).

use super::*;
use std::collections::HashMap;

impl Worker {
    /// Copy the active result into a static tab that stays as it is.
    pub(crate) fn pin_result(&mut self) {
        let Some(t) = self.active_tab().cloned() else { return };
        if t.rows.is_empty() && t.cols.is_empty() {
            return self.toast("Nothing to pin yet.");
        }
        let mut p = Tab::new(Kind::Pinned, format!("Pinned: {}", t.title.trim_start_matches("Pinned: ")), self.default_page_size());
        p.cols = t.cols.clone();
        p.widths = t.widths.clone();
        p.rows = t.rows.clone();
        p.editable = false;
        p.page_info = format!("{} row(s) pinned {}", t.rows.len(), hms());
        self.add_tab(p);
        self.show_active();
        self.toast("Pinned. Run again, then use “Compare with pinned”.");
    }

    /// Diff the active result against the newest pinned one: added and removed rows.
    pub(crate) fn compare_result(&mut self) {
        let Some(active) = self.active_tab().cloned() else { return };
        let Some(pinned) = self.tabs.iter().rev().find(|t| t.kind == Kind::Pinned && !t.title.starts_with("Diff")).cloned() else {
            return self.toast("Pin a result first, then run again and compare.");
        };
        let names = |t: &Tab| t.cols.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        if names(&active) != names(&pinned) {
            return self.toast("The columns differ from the pinned result, so they cannot be compared.");
        }
        let (rows, added, removed, same) = diff_rows(&pinned.rows, &active.rows);
        let mut cols = vec![ColMeta::plain("Δ")];
        cols.extend(active.cols.iter().cloned());
        let mut d = Tab::new(Kind::Pinned, format!("Diff: {} vs pinned", active.title), self.default_page_size());
        d.widths = auto_widths(&cols, &rows);
        d.cols = cols;
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
