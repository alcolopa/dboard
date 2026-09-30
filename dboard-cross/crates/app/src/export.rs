//! Pure formatters for "Export data": CSV, JSON and SQL INSERT statements.

use dboard_core::model::Cell;
use dboard_core::sql::Dialect;

pub struct ExportCol {
    pub name: String,
    pub type_name: String,
}


pub fn extension(format: usize) -> &'static str {
    match format {
        0 => "csv",
        1 => "json",
        _ => "sql",
    }
}

pub fn format(format: usize, cols: &[ExportCol], rows: &[Vec<Cell>], headers: bool, table: &str, d: Dialect) -> String {
    match format {
        0 => csv(cols, rows, headers),
        1 => json(cols, rows),
        _ => sql_inserts(cols, rows, table, d),
    }
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn csv(cols: &[ExportCol], rows: &[Vec<Cell>], headers: bool) -> String {
    let mut out = String::new();
    if headers {
        out.push_str(&cols.iter().map(|c| csv_field(&c.name)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    for r in rows {
        out.push_str(&r.iter().map(|c| c.as_deref().map(csv_field).unwrap_or_default()).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

fn is_numeric(ty: &str) -> bool {
    let t = ty.to_lowercase();
    ["int", "numeric", "decimal", "float", "double", "real", "serial", "money"].iter().any(|k| t.contains(k)) && !t.contains("interval") && t != "tinyint(1)"
}

fn is_bool(ty: &str) -> bool {
    matches!(ty.to_lowercase().as_str(), "boolean" | "bool" | "tinyint(1)")
}

fn json_value(v: &str, ty: &str) -> serde_json::Value {
    use serde_json::Value;
    let t = ty.to_lowercase();
    if is_numeric(&t) {
        if let Ok(n) = v.parse::<i64>() {
            return Value::from(n);
        }
        if let Some(n) = v.parse::<f64>().ok().and_then(serde_json::Number::from_f64) {
            return Value::Number(n);
        }
    }
    if is_bool(&t) {
        match v {
            "true" | "t" | "1" => return Value::Bool(true),
            "false" | "f" | "0" => return Value::Bool(false),
            _ => {}
        }
    }
    if matches!(t.as_str(), "json" | "jsonb" | "object" | "array") {
        if let Ok(x) = serde_json::from_str(v) {
            return x;
        }
    }
    Value::String(v.to_string())
}

pub fn json(cols: &[ExportCol], rows: &[Vec<Cell>]) -> String {
    let arr: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let mut m = serde_json::Map::new();
            for (c, v) in cols.iter().zip(r) {
                m.insert(c.name.clone(), v.as_deref().map(|s| json_value(s, &c.type_name)).unwrap_or(serde_json::Value::Null));
            }
            serde_json::Value::Object(m)
        })
        .collect();
    serde_json::to_string_pretty(&arr).unwrap_or_default()
}

pub fn sql_inserts(cols: &[ExportCol], rows: &[Vec<Cell>], table: &str, d: Dialect) -> String {
    let names = cols.iter().map(|c| d.quote(&c.name)).collect::<Vec<_>>().join(", ");
    let mut out = String::new();
    for r in rows {
        let vals = cols
            .iter()
            .zip(r)
            .map(|(c, v)| match v {
                None => "NULL".to_string(),
                Some(s) if is_numeric(&c.type_name) && s.parse::<f64>().is_ok() => s.clone(),
                Some(s) => format!("'{}'", s.replace('\'', "''")),
            })
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("INSERT INTO {table} ({names}) VALUES ({vals});\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols() -> Vec<ExportCol> {
        vec![
            ExportCol { name: "id".into(), type_name: "integer".into() },
            ExportCol { name: "note".into(), type_name: "text".into() },
            ExportCol { name: "ok".into(), type_name: "boolean".into() },
        ]
    }
    fn rows() -> Vec<Vec<Cell>> {
        vec![
            vec![Some("1".into()), Some("a,\"b\"".into()), Some("true".into())],
            vec![Some("2".into()), None, Some("f".into())],
        ]
    }

    #[test]
    fn csv_quotes_and_nulls() {
        assert_eq!(csv(&cols(), &rows(), true), "id,note,ok\r\n1,\"a,\"\"b\"\"\",true\r\n2,,f\r\n");
        assert!(!csv(&cols(), &rows(), false).starts_with("id"));
    }

    #[test]
    fn json_types() {
        let v: serde_json::Value = serde_json::from_str(&json(&cols(), &rows())).unwrap();
        assert_eq!(v[0]["id"], 1);
        assert_eq!(v[0]["ok"], true);
        assert_eq!(v[1]["note"], serde_json::Value::Null);
        assert_eq!(v[1]["ok"], false);
    }

    #[test]
    fn sql_inserts_escape() {
        let s = sql_inserts(&cols(), &rows(), "\"public\".\"t\"", Dialect::Pg);
        assert!(s.contains("VALUES (1, 'a,\"b\"', 'true');"));
        assert!(s.contains("VALUES (2, NULL, 'f');"));
        let q = sql_inserts(&cols()[1..2], &[vec![Some("it's".into())]], "t", Dialect::My);
        assert!(q.contains("'it''s'") && q.contains("`note`"));
    }
}
