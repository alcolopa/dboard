//! Pure formatters for "Export data": CSV, JSON and SQL INSERT statements.

use dboard_core::model::Cell;
use dboard_core::sql::Dialect;

pub struct ExportCol {
    pub name: String,
    pub type_name: String,
}


/// Tab-separated text as spreadsheets expect it: fields holding a tab, newline or quote are quoted.
pub fn tsv(cols: &[ExportCol], rows: &[Vec<Cell>], headers: bool) -> String {
    fn field(s: &str) -> String {
        if s.contains(['\t', '\n', '\r', '"']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
    }
    let mut lines: Vec<String> = Vec::with_capacity(rows.len() + 1);
    if headers {
        lines.push(cols.iter().map(|c| field(&c.name)).collect::<Vec<_>>().join("\t"));
    }
    for r in rows {
        lines.push(r.iter().map(|c| c.as_deref().map(field).unwrap_or_default()).collect::<Vec<_>>().join("\t"));
    }
    lines.join("\n")
}

/// Parse clipboard text copied from a spreadsheet, a terminal or this app (TSV, with optional
/// quoted fields). A trailing newline does not create an empty last row.
pub fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    parse_delimited(text, '\t')
}

/// RFC 4180-style parser shared by clipboard paste (tab) and CSV import (comma).
pub fn parse_delimited(text: &str, sep: char) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut was_quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        any = true;
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !was_quoted => {
                quoted = true;
                was_quoted = true;
            }
            c if c == sep => {
                row.push(std::mem::take(&mut field));
                was_quoted = false;
            }
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                was_quoted = false;
            }
            c => field.push(c),
        }
    }
    if any && (!field.is_empty() || !row.is_empty() || was_quoted) {
        row.push(field);
        rows.push(row);
    }
    rows
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
    fn tsv_round_trips_through_the_parser() {
        let r = vec![vec![Some("a\tb".into()), Some("say \"hi\"\nnow".into()), None], vec![Some("1".into()), Some("".into()), Some("x".into())]];
        let text = tsv(&cols(), &r, true);
        assert!(text.starts_with("id\tnote\tok\n"));
        let back = parse_tsv(&text);
        assert_eq!(back[0], ["id", "note", "ok"]);
        assert_eq!(back[1], ["a\tb", "say \"hi\"\nnow", ""]);
        assert_eq!(back[2], ["1", "", "x"]);
    }

    #[test]
    fn parser_handles_spreadsheet_and_csv_text() {
        assert_eq!(parse_tsv("a\tb\r\nc\td\r\n"), vec![vec!["a", "b"], vec!["c", "d"]]);
        assert_eq!(parse_tsv("single"), vec![vec!["single"]]);
        assert!(parse_tsv("").is_empty());
        assert_eq!(parse_delimited("h1,h2\n\"x,1\",\"q\"\"\"\n,\n", ','), vec![vec!["h1", "h2"], vec!["x,1", "q\""], vec!["", ""]]);
        assert_eq!(parse_delimited("\u{feff}a,b", ','), vec![vec!["a", "b"]]);
        assert_eq!(parse_tsv("\"\"\n"), vec![vec![""]]);
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
