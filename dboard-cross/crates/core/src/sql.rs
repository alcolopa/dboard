//! Pure SQL builders (no I/O) so they're unit-testable.

use crate::model::{Page, Table};

/// Quote an identifier for Postgres.
pub fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

pub fn qualified(schema: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(schema), quote_ident(table))
}

/// `SELECT "a"::text, "b"::text FROM ... ORDER BY ... LIMIT n OFFSET m`
pub fn select_page(t: &Table, p: &Page) -> String {
    let cols = t
        .columns
        .iter()
        .map(|c| format!("{}::text", quote_ident(&c.name)))
        .collect::<Vec<_>>()
        .join(", ");
    let mut sql = format!("SELECT {cols} FROM {}", qualified(&t.schema, &t.name));
    if let Some(f) = p.filter.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        sql.push_str(&format!(" WHERE {f}"));
    }
    if let Some(c) = &p.sort_column {
        sql.push_str(&format!(
            " ORDER BY {} {}",
            quote_ident(c),
            if p.sort_ascending { "ASC" } else { "DESC" }
        ));
    }
    sql.push_str(&format!(" LIMIT {} OFFSET {}", p.limit.max(1), p.offset.max(0)));
    sql
}

/// Parameterised single-cell update. Values are bound as text and cast
/// server-side, so keys keep using their indexes.
///
/// Params: `$1` = new value (unless NULL), then one per primary-key column.
/// Returns `None` if the table has no primary key or the column is unknown.
pub fn update_cell(t: &Table, column: &str, set_null: bool) -> Option<String> {
    let col = t.column(column)?;
    let pks = t.primary_keys();
    if pks.is_empty() {
        return None;
    }
    let mut idx = 1;
    let set = if set_null {
        format!("{} = NULL", quote_ident(&col.name))
    } else {
        idx += 1;
        format!("{} = $1::text::{}", quote_ident(&col.name), col.type_name)
    };
    let wh = pks
        .iter()
        .map(|pk| {
            let s = format!("{} = ${idx}::text::{}", quote_ident(&pk.name), pk.type_name);
            idx += 1;
            s
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    Some(format!(
        "UPDATE {} SET {set} WHERE {wh}",
        qualified(&t.schema, &t.name)
    ))
}

pub fn count_estimate(schema: &str, table: &str) -> String {
    format!(
        "SELECT coalesce(c.reltuples::bigint, 0) FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = '{}' AND c.relname = '{}'",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    )
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
        };
        Table {
            schema: "public".into(),
            name: "us\"ers".into(),
            kind: TableKind::Table,
            columns: vec![col("id", "int4", true), col("name", "text", false)],
        }
    }

    #[test]
    fn quotes_identifiers() {
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn select_builds_sort_filter_page() {
        let p = Page {
            limit: 50,
            offset: 100,
            sort_column: Some("name".into()),
            sort_ascending: false,
            filter: Some(" id > 3 ".into()),
        };
        assert_eq!(
            select_page(&table(), &p),
            "SELECT \"id\"::text, \"name\"::text FROM \"public\".\"us\"\"ers\" WHERE id > 3 ORDER BY \"name\" DESC LIMIT 50 OFFSET 100"
        );
    }

    #[test]
    fn update_is_parameterised() {
        assert_eq!(
            update_cell(&table(), "name", false).unwrap(),
            "UPDATE \"public\".\"us\"\"ers\" SET \"name\" = $1::text::text WHERE \"id\" = $2::text::int4"
        );
        assert_eq!(
            update_cell(&table(), "name", true).unwrap(),
            "UPDATE \"public\".\"us\"\"ers\" SET \"name\" = NULL WHERE \"id\" = $1::text::int4"
        );
    }

    #[test]
    fn update_refused_without_pk() {
        let mut t = table();
        t.columns[0].is_primary_key = false;
        assert!(update_cell(&t, "name", false).is_none());
    }
}
