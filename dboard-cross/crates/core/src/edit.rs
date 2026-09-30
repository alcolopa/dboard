use crate::model::Cell;

/// One committed cell edit; enough to build the inverse for undo.
#[derive(Debug, Clone, PartialEq)]
pub struct EditRecord {
    pub schema: String,
    pub table: String,
    pub column: String,
    /// Primary key values in the table's PK column order.
    pub key: Vec<String>,
    pub old: Cell,
    pub new: Cell,
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

    pub fn len(&self) -> usize {
        self.stack.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(n: &str) -> EditRecord {
        EditRecord {
            schema: "public".into(),
            table: "t".into(),
            column: "c".into(),
            key: vec!["1".into()],
            old: Some("a".into()),
            new: Some(n.into()),
        }
    }

    #[test]
    fn lifo_undo() {
        let mut h = EditHistory::default();
        h.push(rec("b"));
        h.push(rec("c"));
        assert_eq!(h.pop().unwrap().new.as_deref(), Some("c"));
        assert_eq!(h.len(), 1);
    }
}
