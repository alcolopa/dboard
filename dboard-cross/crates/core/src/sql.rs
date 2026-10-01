//! Pure SQL builders (no I/O) so they're unit-testable. Two dialects: Postgres and MySQL.

use crate::model::{Page, Table};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Pg,
    My,
}

impl Dialect {
    pub fn quote(self, s: &str) -> String {
        match self {
            Dialect::Pg => format!("\"{}\"", s.replace('"', "\"\"")),
            Dialect::My => format!("`{}`", s.replace('`', "``")),
        }
    }

    /// Positional placeholder, 1-based.
    fn ph(self, n: usize, ty: &str) -> String {
        match self {
            Dialect::Pg => format!("${n}::text::{ty}"),
            Dialect::My => "?".into(),
        }
    }

    pub fn qualified(self, t: &Table) -> String {
        if t.schema.is_empty() {
            self.quote(&t.name)
        } else {
            format!("{}.{}", self.quote(&t.schema), self.quote(&t.name))
        }
    }
}

pub fn quote_ident(s: &str) -> String {
    Dialect::Pg.quote(s)
}

/// SQL string literal. PostgreSQL assumes `standard_conforming_strings = on` (the default and
/// what dumps set explicitly); MySQL escapes backslashes the way its default mode reads them.
pub fn literal(d: Dialect, s: &str) -> String {
    match d {
        Dialect::Pg => format!("'{}'", s.replace('\'', "''")),
        Dialect::My => {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('\'');
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\'' => out.push_str("\\'"),
                    '\0' => out.push_str("\\0"),
                    '\x1a' => out.push_str("\\Z"),
                    c => out.push(c),
                }
            }
            out.push('\'');
            out
        }
    }
}

/// `SELECT "a"::text, ... FROM ... WHERE ... ORDER BY ... LIMIT n OFFSET m`
pub fn select_page(d: Dialect, t: &Table, p: &Page) -> String {
    let cols = t
        .columns
        .iter()
        .map(|c| match d {
            Dialect::Pg => format!("{}::text", d.quote(&c.name)),
            Dialect::My => d.quote(&c.name),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!("SELECT {cols} FROM {}", d.qualified(t));
    if let Some(f) = p.filter.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        sql.push_str(&format!(" WHERE {f}"));
    }
    // PostgreSQL selects every column as text, and an unqualified ORDER BY name would resolve to
    // that output column (sorting 10 before 2). Qualifying with the table forces the real column.
    let order_col = |c: &str| match d {
        Dialect::Pg => format!("{}.{}", d.qualified(t), d.quote(c)),
        Dialect::My => d.quote(c),
    };
    if let Some(c) = &p.sort_column {
        sql.push_str(&format!(" ORDER BY {} {}", order_col(c), if p.sort_ascending { "ASC" } else { "DESC" }));
    } else if t.has_primary_key() && matches!(t.kind, crate::model::TableKind::Table) {
        // Stable default order: without it Postgres returns an edited row at the end of the table.
        let pks = t.primary_keys().iter().map(|c| order_col(&c.name)).collect::<Vec<_>>().join(", ");
        sql.push_str(&format!(" ORDER BY {pks}"));
    }
    sql.push_str(&format!(" LIMIT {} OFFSET {}", p.limit.max(1), p.offset.max(0)));
    sql
}

/// Number of key parameters a statement built with [`key_where`] expects.
pub fn key_len(t: &Table) -> usize {
    let n = t.primary_keys().len();
    if n > 0 { n } else { t.columns.len() }
}

/// WHERE clause that addresses exactly one row. Primary key columns when there are any;
/// otherwise (PostgreSQL only) the physical row id of the first row equal to the whole row.
fn key_where(d: Dialect, t: &Table, mut idx: usize) -> Option<String> {
    let pks = t.primary_keys();
    if pks.is_empty() {
        if d != Dialect::Pg || !t.keyless_edit || t.columns.is_empty() {
            return None;
        }
        let eq = t
            .columns
            .iter()
            .map(|c| {
                let s = format!("{}::text IS NOT DISTINCT FROM ${idx}::text", d.quote(&c.name));
                idx += 1;
                s
            })
            .collect::<Vec<_>>()
            .join(" AND ");
        return Some(format!("ctid = (SELECT ctid FROM {} WHERE {eq} LIMIT 1)", d.qualified(t)));
    }
    Some(
        pks.iter()
            .map(|pk| {
                let s = format!("{} = {}", d.quote(&pk.name), d.ph(idx, &pk.type_name));
                idx += 1;
                s
            })
            .collect::<Vec<_>>()
            .join(" AND "),
    )
}

/// Parameterised single-cell update. Params: new value (unless NULL), then one per PK column.
/// Returns `None` if the table has no primary key or the column is unknown.
pub fn update_cell(d: Dialect, t: &Table, column: &str, set_null: bool) -> Option<String> {
    let col = t.column(column)?;
    let (set, next) = if set_null {
        (format!("{} = NULL", d.quote(&col.name)), 1)
    } else {
        (format!("{} = {}", d.quote(&col.name), d.ph(1, &col.type_name)), 2)
    };
    Some(format!("UPDATE {} SET {set} WHERE {}", d.qualified(t), key_where(d, t, next)?))
}

/// `INSERT` for the given columns (omitted columns take their defaults).
/// One param per column, in order.
pub fn insert_row(d: Dialect, t: &Table, columns: &[&str]) -> Option<String> {
    if columns.is_empty() {
        return Some(match d {
            Dialect::Pg => format!("INSERT INTO {} DEFAULT VALUES", d.qualified(t)),
            Dialect::My => format!("INSERT INTO {} () VALUES ()", d.qualified(t)),
        });
    }
    let mut ph = Vec::new();
    for (i, c) in columns.iter().enumerate() {
        ph.push(d.ph(i + 1, &t.column(c)?.type_name));
    }
    Some(format!(
        "INSERT INTO {} ({}) VALUES ({})",
        d.qualified(t),
        columns.iter().map(|c| d.quote(c)).collect::<Vec<_>>().join(", "),
        ph.join(", ")
    ))
}

/// `INSERT` that can also set columns to NULL explicitly (used to restore a deleted row).
/// Params: one per non-NULL column, in order.
pub fn insert_row_with_nulls(d: Dialect, t: &Table, columns: &[(&str, bool)]) -> Option<String> {
    if columns.is_empty() {
        return insert_row(d, t, &[]);
    }
    let mut n = 0;
    let mut ph = Vec::new();
    for (c, is_null) in columns {
        let col = t.column(c)?;
        if *is_null {
            ph.push("NULL".to_string());
        } else {
            n += 1;
            ph.push(d.ph(n, &col.type_name));
        }
    }
    Some(format!(
        "INSERT INTO {} ({}) VALUES ({})",
        d.qualified(t),
        columns.iter().map(|(c, _)| d.quote(c)).collect::<Vec<_>>().join(", "),
        ph.join(", ")
    ))
}

/// Delete by primary key. One param per PK column.
pub fn delete_row(d: Dialect, t: &Table) -> Option<String> {
    Some(format!("DELETE FROM {} WHERE {}", d.qualified(t), key_where(d, t, 1)?))
}

pub fn truncate(d: Dialect, t: &Table) -> String {
    format!("TRUNCATE TABLE {}", d.qualified(t))
}

pub fn drop_table(d: Dialect, t: &Table) -> String {
    let what = match t.kind {
        crate::model::TableKind::View => "VIEW",
        crate::model::TableKind::MaterializedView => "MATERIALIZED VIEW",
        _ => "TABLE",
    };
    format!("DROP {what} {}", d.qualified(t))
}

pub fn count_estimate_pg(schema: &str, table: &str) -> String {
    format!(
        "SELECT coalesce(c.reltuples::bigint, 0) FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = '{}' AND c.relname = '{}'",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    )
}

/// Best-effort CREATE TABLE for Postgres (columns, defaults, NOT NULL, primary key).
pub fn pg_ddl(t: &Table) -> String {
    let d = Dialect::Pg;
    let mut lines: Vec<String> = t
        .columns
        .iter()
        .map(|c| {
            let mut l = format!("  {} {}", d.quote(&c.name), c.type_name);
            if let Some(def) = &c.default {
                l.push_str(&format!(" DEFAULT {def}"));
            }
            if !c.nullable {
                l.push_str(" NOT NULL");
            }
            l
        })
        .collect();
    let pks: Vec<String> = t.primary_keys().iter().map(|c| d.quote(&c.name)).collect();
    if !pks.is_empty() {
        lines.push(format!("  PRIMARY KEY ({})", pks.join(", ")));
    }
    for c in &t.columns {
        if let Some(fk) = &c.fk {
            lines.push(format!("  -- {} references {fk}", d.quote(&c.name)));
        }
    }
    format!("CREATE TABLE {} (\n{}\n);", d.qualified(t), lines.join(",\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, TableKind};

    fn table() -> Table {
        let col = |n: &str, t: &str, pk: bool| Column {
            name: n.into(),
            type_name: t.into(),
            nullable: !pk,
            is_primary_key: pk,
            default: None,
            fk: None,
        };
        Table {
            schema: "public".into(),
            name: "us\"ers".into(),
            kind: TableKind::Table,
            columns: vec![col("id", "int4", true), col("name", "text", false)],
            estimated_rows: None,
            size_bytes: None,
            indexes: vec![],
            keyless_edit: false,
        }
    }

    #[test]
    fn literals_are_escaped_per_dialect() {
        assert_eq!(literal(Dialect::Pg, "it's \\ ok"), "'it''s \\ ok'");
        assert_eq!(literal(Dialect::My, "it's \\ ok"), "'it\\'s \\\\ ok'");
        assert_eq!(literal(Dialect::My, "a\0b"), "'a\\0b'");
    }

    #[test]
    fn quotes_identifiers() {
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(Dialect::My.quote("a`b"), "`a``b`");
    }

    #[test]
    fn select_builds_sort_filter_page() {
        let p = Page { limit: 50, offset: 100, sort_column: Some("name".into()), sort_ascending: false, filter: Some(" id > 3 ".into()) };
        assert_eq!(
            select_page(Dialect::Pg, &table(), &p),
            "SELECT \"id\"::text, \"name\"::text FROM \"public\".\"us\"\"ers\" WHERE id > 3 ORDER BY \"public\".\"us\"\"ers\".\"name\" DESC LIMIT 50 OFFSET 100"
        );
        assert!(select_page(Dialect::My, &table(), &p).starts_with("SELECT `id`, `name` FROM `public`.`us\"ers`"));
    }

    #[test]
    fn default_order_is_primary_key() {
        let p = Page { limit: 10, offset: 0, sort_column: None, sort_ascending: true, filter: None };
        assert!(select_page(Dialect::Pg, &table(), &p).ends_with("ORDER BY \"public\".\"us\"\"ers\".\"id\" LIMIT 10 OFFSET 0"));
        let mut t = table();
        t.columns[0].is_primary_key = false;
        assert!(!select_page(Dialect::Pg, &t, &p).contains("ORDER BY"));
    }

    #[test]
    fn update_is_parameterised() {
        assert_eq!(
            update_cell(Dialect::Pg, &table(), "name", false).unwrap(),
            "UPDATE \"public\".\"us\"\"ers\" SET \"name\" = $1::text::text WHERE \"id\" = $2::text::int4"
        );
        assert_eq!(
            update_cell(Dialect::Pg, &table(), "name", true).unwrap(),
            "UPDATE \"public\".\"us\"\"ers\" SET \"name\" = NULL WHERE \"id\" = $1::text::int4"
        );
        assert_eq!(
            update_cell(Dialect::My, &table(), "name", false).unwrap(),
            "UPDATE `public`.`us\"ers` SET `name` = ? WHERE `id` = ?"
        );
    }

    #[test]
    fn update_refused_without_pk() {
        let mut t = table();
        t.columns[0].is_primary_key = false;
        assert!(update_cell(Dialect::Pg, &t, "name", false).is_none());
        assert!(delete_row(Dialect::Pg, &t).is_none());
    }

    #[test]
    fn keyless_postgres_tables_are_addressed_by_ctid() {
        let mut t = table();
        t.columns[0].is_primary_key = false;
        assert!(update_cell(Dialect::Pg, &t, "name", false).is_none());
        t.keyless_edit = true;
        assert_eq!(key_len(&t), 2);
        assert_eq!(
            update_cell(Dialect::Pg, &t, "name", false).unwrap(),
            "UPDATE \"public\".\"us\"\"ers\" SET \"name\" = $1::text::text WHERE ctid = (SELECT ctid FROM \"public\".\"us\"\"ers\" \
             WHERE \"id\"::text IS NOT DISTINCT FROM $2::text AND \"name\"::text IS NOT DISTINCT FROM $3::text LIMIT 1)"
        );
        assert!(delete_row(Dialect::Pg, &t).unwrap().starts_with("DELETE FROM \"public\".\"us\"\"ers\" WHERE ctid = ("));
        assert!(delete_row(Dialect::My, &t).is_none(), "MySQL has no exact row id, so it stays read-only");
    }

    #[test]
    fn insert_with_explicit_nulls() {
        let t = table();
        assert_eq!(
            insert_row_with_nulls(Dialect::Pg, &t, &[("id", false), ("name", true)]).unwrap(),
            "INSERT INTO \"public\".\"us\"\"ers\" (\"id\", \"name\") VALUES ($1::text::int4, NULL)"
        );
        assert_eq!(
            insert_row_with_nulls(Dialect::My, &t, &[("id", true), ("name", false)]).unwrap(),
            "INSERT INTO `public`.`us\"ers` (`id`, `name`) VALUES (NULL, ?)"
        );
    }

    #[test]
    fn insert_and_delete() {
        let t = table();
        assert_eq!(
            insert_row(Dialect::Pg, &t, &["name"]).unwrap(),
            "INSERT INTO \"public\".\"us\"\"ers\" (\"name\") VALUES ($1::text::text)"
        );
        assert_eq!(insert_row(Dialect::Pg, &t, &[]).unwrap(), "INSERT INTO \"public\".\"us\"\"ers\" DEFAULT VALUES");
        assert_eq!(delete_row(Dialect::My, &t).unwrap(), "DELETE FROM `public`.`us\"ers` WHERE `id` = ?");
        assert!(insert_row(Dialect::Pg, &t, &["nope"]).is_none());
    }

    #[test]
    fn ddl_contains_pk() {
        let ddl = pg_ddl(&table());
        assert!(ddl.contains("PRIMARY KEY (\"id\")") && ddl.contains("NOT NULL"));
    }
}
