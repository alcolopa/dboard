//! Shared pieces of "export database" / "import database": options, statistics and the
//! batched `INSERT` writer. The engine-specific parts live next to each driver.

use crate::sql::{literal, quote_ident, Dialect};
use crate::{Error, Result};
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct DumpOptions {
    pub schema: bool,
    pub data: bool,
}

impl Default for DumpOptions {
    fn default() -> Self {
        Self { schema: true, data: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DumpStats {
    pub tables: usize,
    pub rows: u64,
    /// Views, routines, triggers, sequences, ... written in addition to tables.
    pub objects: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ImportOptions {
    /// Stop at the first failing statement (PostgreSQL also rolls everything back). When off,
    /// failures are collected and the import carries on.
    pub stop_on_error: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self { stop_on_error: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub statements: usize,
    pub rows_copied: u64,
    pub errors: Vec<String>,
}

pub type Progress<'a> = &'a mut dyn FnMut(String);

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Db(e.to_string())
    }
}

pub fn is_numeric(ty: &str) -> bool {
    let t = ty.to_lowercase();
    ["int", "numeric", "decimal", "float", "double", "real", "serial"].iter().any(|k| t.contains(k)) && !t.contains("interval") && t != "tinyint(1)"
}

pub fn is_binary(ty: &str) -> bool {
    let t = ty.to_lowercase();
    t.contains("blob") || t.contains("binary")
}

/// A value as it appears in an `INSERT`: numbers bare, everything else quoted.
pub fn sql_value(d: Dialect, v: Option<&str>, numeric: bool) -> String {
    match v {
        None => "NULL".into(),
        Some(s) if numeric && s.parse::<f64>().is_ok_and(f64::is_finite) && !s.starts_with('+') => s.to_string(),
        Some(s) => literal(d, s),
    }
}

pub fn hex_literal(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "''".into();
    }
    let mut s = String::with_capacity(bytes.len() * 2 + 2);
    s.push_str("0x");
    for b in bytes {
        s.push_str(&format!("{b:02X}"));
    }
    s
}

/// Collects row tuples and writes them as multi-row `INSERT` statements.
pub struct InsertWriter {
    head: String,
    rows: Vec<String>,
    bytes: usize,
    pub total: u64,
}

impl InsertWriter {
    const MAX_ROWS: usize = 250;
    const MAX_BYTES: usize = 256 * 1024;

    pub fn new(d: Dialect, qualified_table: &str, columns: &[String]) -> Self {
        let cols = columns.iter().map(|c| if d == Dialect::Pg { quote_ident(c) } else { d.quote(c) }).collect::<Vec<_>>().join(", ");
        Self { head: format!("INSERT INTO {qualified_table} ({cols}) VALUES\n"), rows: Vec::new(), bytes: 0, total: 0 }
    }

    pub fn push(&mut self, values: &[String], out: &mut dyn Write) -> Result<()> {
        let tuple = format!("({})", values.join(", "));
        self.bytes += tuple.len();
        self.rows.push(tuple);
        self.total += 1;
        if self.rows.len() >= Self::MAX_ROWS || self.bytes >= Self::MAX_BYTES {
            self.flush(out)?;
        }
        Ok(())
    }

    pub fn flush(&mut self, out: &mut dyn Write) -> Result<()> {
        if self.rows.is_empty() {
            return Ok(());
        }
        writeln!(out, "{}{};", self.head, self.rows.join(",\n"))?;
        self.rows.clear();
        self.bytes = 0;
        Ok(())
    }
}

/// Remove `DEFINER=...` from a `CREATE VIEW/TRIGGER/PROCEDURE/EVENT` statement so it can be
/// restored by a user other than the original definer.
pub fn strip_definer(sql: &str) -> String {
    let Some(start) = sql.find("DEFINER=") else { return sql.to_string() };
    let b = sql.as_bytes();
    let mut i = start + "DEFINER=".len();
    let mut in_quote: Option<u8> = None;
    while i < b.len() {
        match (in_quote, b[i]) {
            (None, b'`' | b'\'' | b'"') => in_quote = Some(b[i]),
            (Some(q), c) if c == q => in_quote = None,
            (None, c) if c.is_ascii_whitespace() => break,
            _ => {}
        }
        i += 1;
    }
    let mut out = sql[..start].trim_end_matches(' ').to_string();
    out.push(' ');
    out.push_str(sql[i..].trim_start());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_quote_or_not() {
        assert_eq!(sql_value(Dialect::Pg, Some("12"), true), "12");
        assert_eq!(sql_value(Dialect::Pg, Some("12"), false), "'12'");
        assert_eq!(sql_value(Dialect::Pg, Some("NaN"), true), "'NaN'");
        assert_eq!(sql_value(Dialect::Pg, None, true), "NULL");
        assert_eq!(sql_value(Dialect::My, Some("a'b"), false), "'a\\'b'");
    }

    #[test]
    fn insert_writer_batches() {
        let mut w = InsertWriter::new(Dialect::Pg, "\"public\".\"t\"", &["a".into(), "b".into()]);
        let mut out = Vec::new();
        w.push(&["1".into(), "'x'".into()], &mut out).unwrap();
        w.push(&["2".into(), "NULL".into()], &mut out).unwrap();
        w.flush(&mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "INSERT INTO \"public\".\"t\" (\"a\", \"b\") VALUES\n(1, 'x'),\n(2, NULL);\n");
        assert_eq!(w.total, 2);
    }

    #[test]
    fn definer_is_removed() {
        assert_eq!(strip_definer("CREATE DEFINER=`root`@`localhost` PROCEDURE p() BEGIN END"), "CREATE PROCEDURE p() BEGIN END");
        assert_eq!(strip_definer("CREATE ALGORITHM=UNDEFINED DEFINER=`a b`@`%` SQL SECURITY DEFINER VIEW v AS SELECT 1"), "CREATE ALGORITHM=UNDEFINED SQL SECURITY DEFINER VIEW v AS SELECT 1");
        assert_eq!(strip_definer("CREATE TABLE t (a int)"), "CREATE TABLE t (a int)");
    }

    #[test]
    fn binary_and_numeric_types() {
        assert!(is_numeric("int(11)") && is_numeric("decimal(10,2)") && !is_numeric("varchar(5)") && !is_numeric("tinyint(1)"));
        assert!(is_binary("longblob") && is_binary("varbinary(16)") && !is_binary("text"));
        assert_eq!(hex_literal(&[0xde, 0xad]), "0xDEAD");
    }
}
