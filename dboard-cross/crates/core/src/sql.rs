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
    if let Some(c) = &p.sort_column {
        sql.push_str(&format!(" ORDER BY {} {}", d.quote(c), if p.sort_ascending { "ASC" } else { "DESC" }));
    } else if t.has_primary_key() && matches!(t.kind, crate::model::TableKind::Table) {
        // Stable default order: without it Postgres returns an edited row at the end of the table.
        let pks = t.primary_keys().iter().map(|c| d.quote(&c.name)).collect::<Vec<_>>().join(", ");
        sql.push_str(&format!(" ORDER BY {pks}"));
    }
    sql.push_str(&format!(" LIMIT {} OFFSET {}", p.limit.max(1), p.offset.max(0)));
    sql
}

fn key_where(d: Dialect, t: &Table, mut idx: usize) -> Option<String> {
    let pks = t.primary_keys();
    if pks.is_empty() {
        return None;
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
        }
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
            "SELECT \"id\"::text, \"name\"::text FROM \"public\".\"us\"\"ers\" WHERE id > 3 ORDER BY \"name\" DESC LIMIT 50 OFFSET 100"
        );
        assert!(select_page(Dialect::My, &table(), &p).starts_with("SELECT `id`, `name` FROM `public`.`us\"ers`"));
    }

    #[test]
    fn default_order_is_primary_key() {
        let p = Page { limit: 10, offset: 0, sort_column: None, sort_ascending: true, filter: None };
        assert!(select_page(Dialect::Pg, &table(), &p).ends_with("ORDER BY \"id\" LIMIT 10 OFFSET 0"));
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
