//! SQL snippets for the query editor. Placeholders use `{{name}}`, so running one prompts for values.

use dboard_core::model::{Table, TableKind};
use dboard_core::sql::Dialect;

fn qualified(d: Dialect, t: &Table) -> String {
    if t.schema.is_empty() {
        d.quote(&t.name)
    } else {
        format!("{}.{}", d.quote(&t.schema), d.quote(&t.name))
    }
}

/// The table a snippet is written for: the open one, else the first real table.
fn pick<'a>(tables: &'a [Table], active: Option<&str>) -> Option<&'a Table> {
    active
        .and_then(|a| tables.iter().find(|t| t.name == a && t.kind == TableKind::Table))
        .or_else(|| tables.iter().find(|t| t.kind == TableKind::Table))
}

pub fn sql_snippet(kind: &str, d: Dialect, tables: &[Table], active: Option<&str>) -> Option<String> {
    let t = pick(tables, active)?;
    let q = qualified(d, t);
    let first_col = t.columns.iter().find(|c| !c.is_primary_key).or(t.columns.first()).map(|c| c.name.clone()).unwrap_or_else(|| "column".into());
    let key = t.columns.iter().find(|c| c.is_primary_key).map(|c| c.name.clone()).unwrap_or_else(|| t.columns.first().map(|c| c.name.clone()).unwrap_or_else(|| "id".into()));
    Some(match kind {
        "select" => format!("SELECT *\nFROM {q}\nLIMIT 100;"),
        "count" => format!("SELECT COUNT(*) FROM {q};"),
        "group" => format!("SELECT {c}, COUNT(*) AS n\nFROM {q}\nGROUP BY {c}\nORDER BY n DESC;", c = d.quote(&first_col)),
        "join" => {
            // Prefer a foreign key of the open table, then any foreign key in the database.
            let fks = |tb: &Table| -> Option<(String, String, String)> {
                tb.columns.iter().find_map(|c| {
                    let fk = c.fk.as_ref()?;
                    let (target, col) = fk.strip_suffix(')')?.rsplit_once('(')?;
                    Some((c.name.clone(), target.to_string(), col.to_string()))
                })
            };
            let (base, (col, target, refcol)) = fks(t).map(|f| (t, f)).or_else(|| tables.iter().filter(|x| x.kind == TableKind::Table).find_map(|x| fks(x).map(|f| (x, f))))?;
            let (ts, tn) = target.split_once('.').unwrap_or(("", target.as_str()));
            let rq = if ts.is_empty() { d.quote(tn) } else { format!("{}.{}", d.quote(ts), d.quote(tn)) };
            format!("SELECT a.*, b.*\nFROM {} a\nJOIN {rq} b ON a.{} = b.{}\nLIMIT 100;", qualified(d, base), d.quote(&col), d.quote(&refcol))
        }
        "insert" => {
            let cols: Vec<&dboard_core::model::Column> = t.columns.iter().filter(|c| c.default.is_none()).collect();
            let names = cols.iter().map(|c| d.quote(&c.name)).collect::<Vec<_>>().join(", ");
            let vals = cols.iter().map(|c| format!("'{{{{{}}}}}'", c.name)).collect::<Vec<_>>().join(", ");
            format!("INSERT INTO {q} ({names})\nVALUES ({vals});")
        }
        "update" => format!("UPDATE {q}\nSET {} = '{{{{value}}}}'\nWHERE {} = '{{{{id}}}}';", d.quote(&first_col), d.quote(&key)),
        "delete" => format!("DELETE FROM {q}\nWHERE {} = '{{{{id}}}}';", d.quote(&key)),
        "cte" => format!("WITH recent AS (\n  SELECT *\n  FROM {q}\n  LIMIT 100\n)\nSELECT * FROM recent;"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dboard_core::model::Column;

    fn col(name: &str, pk: bool, fk: Option<&str>, default: bool) -> Column {
        Column { name: name.into(), type_name: "int".into(), nullable: false, is_primary_key: pk, default: default.then(|| "x".into()), fk: fk.map(String::from) }
    }
    fn table(name: &str, cols: Vec<Column>) -> Table {
        Table { schema: "public".into(), name: name.into(), kind: TableKind::Table, columns: cols, estimated_rows: None, size_bytes: None, indexes: vec![], keyless_edit: false }
    }
    fn tables() -> Vec<Table> {
        vec![
            table("users", vec![col("id", true, None, true), col("name", false, None, false)]),
            table("orders", vec![col("id", true, None, true), col("user_id", false, Some("public.users(id)"), false), col("total", false, None, false)]),
        ]
    }

    #[test]
    fn join_follows_the_foreign_key() {
        let s = sql_snippet("join", Dialect::Pg, &tables(), Some("orders")).unwrap();
        assert!(s.contains("FROM \"public\".\"orders\" a"));
        assert!(s.contains("JOIN \"public\".\"users\" b ON a.\"user_id\" = b.\"id\""));
        // from a table without its own FK it still finds one elsewhere
        assert!(sql_snippet("join", Dialect::Pg, &tables(), Some("users")).unwrap().contains("orders"));
    }

    #[test]
    fn insert_skips_defaulted_columns_and_uses_variables() {
        let s = sql_snippet("insert", Dialect::Pg, &tables(), Some("orders")).unwrap();
        assert!(s.contains("(\"user_id\", \"total\")"));
        assert!(s.contains("'{{user_id}}', '{{total}}'"));
        assert!(!s.contains("\"id\""));
    }

    #[test]
    fn update_and_delete_target_the_primary_key() {
        let s = sql_snippet("delete", Dialect::My, &tables(), Some("users")).unwrap();
        assert!(s.contains("WHERE `id` = '{{id}}'"));
        assert!(sql_snippet("update", Dialect::Pg, &tables(), Some("users")).unwrap().contains("SET \"name\" = '{{value}}'"));
    }

    #[test]
    fn unknown_kind_or_no_tables_gives_nothing() {
        assert!(sql_snippet("nope", Dialect::Pg, &tables(), None).is_none());
        assert!(sql_snippet("select", Dialect::Pg, &[], None).is_none());
    }
}
