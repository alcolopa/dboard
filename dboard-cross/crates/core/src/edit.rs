use crate::model::Cell;

/// What a history entry did, with enough data to build the inverse.
#[derive(Debug, Clone, PartialEq)]
pub enum EditKind {
    /// One cell changed from `old` to `new`.
    Update { column: String, old: Cell, new: Cell },
    /// A whole row was deleted. `row` holds every displayed value; `doc` is the full JSON
    /// document for MongoDB (so nested fields survive an undo).
    Delete { row: Vec<Cell>, doc: Option<String> },
}

/// One committed change, newest last in [`EditHistory`].
#[derive(Debug, Clone, PartialEq)]
pub struct EditRecord {
    pub schema: String,
    pub table: String,
    /// Values that address the affected row *as it is now* (primary key, or the whole row for
    /// tables without one), so the inverse can find it again even if the key itself was edited.
    pub key: Vec<Cell>,
    pub kind: EditKind,
    /// Unix seconds.
    pub at: u64,
}

impl EditRecord {
    pub fn column(&self) -> Option<&str> {
        match &self.kind {
            EditKind::Update { column, .. } => Some(column),
            EditKind::Delete { .. } => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct EditHistory {
    stack: Vec<EditRecord>,
}

impl EditHistory {
    const MAX: usize = 500;

    pub fn push(&mut self, r: EditRecord) {
        self.stack.push(r);
        if self.stack.len() > Self::MAX {
            self.stack.remove(0);
        }
    }

    pub fn pop(&mut self) -> Option<EditRecord> {
        self.stack.pop()
    }

    /// Remove and return the entry at `index` (0 = oldest).
    pub fn remove(&mut self, index: usize) -> Option<EditRecord> {
        (index < self.stack.len()).then(|| self.stack.remove(index))
    }

    pub fn insert(&mut self, index: usize, r: EditRecord) {
        self.stack.insert(index.min(self.stack.len()), r);
    }

    pub fn entries(&self) -> &[EditRecord] {
        &self.stack
    }

    pub fn len(&self) -> usize {
        self.stack.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    pub fn clear(&mut self) {
        self.stack.clear();
    }

    /// Undoing entry `index` would overwrite a newer change to the same cell; the newer one
    /// has to be undone first.
    pub fn blocked_by_newer(&self, index: usize) -> bool {
        let Some(r) = self.stack.get(index) else { return false };
        let EditKind::Update { column, .. } = &r.kind else { return false };
        self.stack[index + 1..].iter().any(|n| {
            n.schema == r.schema
                && n.table == r.table
                && n.key == r.key
                && matches!(&n.kind, EditKind::Update { column: c, .. } if c == column)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(col: &str, n: &str) -> EditRecord {
        EditRecord {
            schema: "public".into(),
            table: "t".into(),
            key: vec![Some("1".into())],
            kind: EditKind::Update { column: col.into(), old: Some("a".into()), new: Some(n.into()) },
            at: 0,
        }
    }

    #[test]
    fn lifo_undo() {
        let mut h = EditHistory::default();
        h.push(rec("c", "b"));
        h.push(rec("c", "c"));
        let EditKind::Update { new, .. } = h.pop().unwrap().kind else { panic!() };
        assert_eq!(new.as_deref(), Some("c"));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn older_entry_blocked_by_newer_edit_of_same_cell() {
        let mut h = EditHistory::default();
        h.push(rec("c", "b"));
        h.push(rec("other", "x"));
        assert!(!h.blocked_by_newer(0));
        h.push(rec("c", "z"));
        assert!(h.blocked_by_newer(0));
        assert!(!h.blocked_by_newer(2));
        assert_eq!(h.remove(1).unwrap().column(), Some("other"));
    }
}
