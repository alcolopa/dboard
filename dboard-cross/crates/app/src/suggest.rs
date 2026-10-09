//! Autocomplete suggestions for the query editor and fuzzy matching for the palette.

use dboard_core::model::Table;

const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "GROUP BY", "ORDER BY", "LIMIT", "OFFSET", "JOIN", "LEFT JOIN", "INNER JOIN", "ON", "AS",
    "INSERT INTO", "VALUES", "UPDATE", "SET", "DELETE FROM", "CREATE TABLE", "ALTER TABLE", "DROP TABLE", "DISTINCT",
    "COUNT(*)", "HAVING", "UNION", "AND", "OR", "NOT", "NULL", "IS NULL", "IS NOT NULL", "LIKE", "ILIKE", "IN", "BETWEEN",
    "EXISTS", "CASE", "WHEN", "THEN", "ELSE", "END", "RETURNING", "WITH", "EXPLAIN", "ASC", "DESC",
];

const MONGO: &[&str] = &["find", "aggregate", "countDocuments", "limit", "sort", "$match", "$group", "$sort", "$project", "$limit", "$lookup", "$unwind"];

/// The identifier-ish word at the end of `text` (letters, digits, `_`, `.`, `$`).
pub fn trailing_word(text: &str) -> &str {
    let start = text.rfind(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '$'))).map(|i| i + text[i..].chars().next().map_or(1, |c| c.len_utf8())).unwrap_or(0);
    &text[start..]
}

pub fn suggest(text: &str, tables: &[Table], mongo: bool) -> Vec<String> {
    let word = trailing_word(text);
    if word.chars().take(2).count() < 2 {
        return Vec::new();
    }
    let lw = word.to_lowercase();
    let lower_text = text.to_lowercase();
    let mut out: Vec<String> = Vec::with_capacity(8);
    let mut push = |s: &str| {
        let lower = s.to_lowercase();
        if lower != lw && lower.starts_with(&lw) && !out.iter().any(|v| v == s) {
            out.push(s.to_string());
        }
        out.len() == 8
    };
    // Columns of tables already mentioned in the statement, then table names, then keywords.
    for t in tables {
        if lower_text.contains(&t.name.to_lowercase()) {
            for c in &t.columns {
                if push(&c.name) { return out; }
                if push(&format!("{}.{}", t.name, c.name)) { return out; }
            }
        }
    }
    for t in tables {
        if push(&t.name) { return out; }
    }
    if mongo {
        for t in tables {
            if push(&format!("db.{}", t.name)) { return out; }
        }
        for k in MONGO {
            if push(k) { return out; }
        }
    } else {
        for k in KEYWORDS {
            if push(k) { return out; }
        }
    }
    out
}

/// Replace the trailing word with `choice`.
pub fn apply(text: &str, choice: &str) -> String {
    let w = trailing_word(text);
    format!("{}{}", &text[..text.len() - w.len()], choice)
}

/// Subsequence match score for the palette (higher is better; `None` = no match).
#[cfg(test)]
pub fn fuzzy(query: &str, target: &str) -> Option<i32> {
    fuzzy_lower(&query.to_lowercase(), target)
}

/// Palette queries are normalized once, rather than once per table or column.
pub fn fuzzy_lower(query: &str, target: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let t = target.to_lowercase();
    if let Some(pos) = t.find(query) {
        return Some(1000 - pos as i32 - (t.len() - query.len()) as i32);
    }
    let mut it = t.chars();
    let mut score = 0;
    for qc in query.chars() {
        let mut gap = 0;
        loop {
            match it.next() {
                Some(c) if c == qc => break,
                Some(_) => gap += 1,
                None => return None,
            }
        }
        score -= gap;
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dboard_core::model::{Column, TableKind};

    fn table() -> Table {
        Table {
            schema: "public".into(),
            name: "users".into(),
            kind: TableKind::Table,
            columns: vec![Column { name: "email".into(), type_name: "text".into(), nullable: true, is_primary_key: false, default: None, fk: None }],
            estimated_rows: None,
            size_bytes: None,
            indexes: vec![],
            keyless_edit: false,
        }
    }

    #[test]
    fn trailing_word_handles_punctuation() {
        assert_eq!(trailing_word("select * from us"), "us");
        assert_eq!(trailing_word("select users.em"), "users.em");
        assert_eq!(trailing_word("select "), "");
    }

    #[test]
    fn suggests_columns_tables_keywords() {
        let t = [table()];
        assert!(suggest("select em", &t, false).is_empty()); // table not mentioned yet
        assert_eq!(suggest("select * from users where em", &t, false)[0], "email");
        assert!(suggest("select * from us", &t, false).contains(&"users".to_string()));
        assert!(suggest("sel", &t, false).contains(&"SELECT".to_string()));
        assert!(suggest("db.us", &t, true).contains(&"db.users".to_string()));
        assert!(suggest("x", &t, false).is_empty());
    }

    #[test]
    fn apply_replaces_word() {
        assert_eq!(apply("select * from us", "users"), "select * from users");
        assert_eq!(apply("sel", "SELECT"), "SELECT");
    }

    #[test]
    fn suggestions_keep_priority_uniqueness_and_the_eight_item_limit() {
        let mut t = table();
        t.columns = (0..2000).map(|i| Column { name: format!("field{i}"), ..t.columns[0].clone() }).collect();
        let s = suggest("select users fi", &[t.clone(), t], false);
        assert_eq!(s, (0..8).map(|i| format!("field{i}")).collect::<Vec<_>>());
        assert_eq!(suggest("se", &[], false).first().map(String::as_str), Some("SELECT"));
    }

    #[test]
    fn fuzzy_ranks() {
        assert!(fuzzy("usr", "users").is_some());
        assert!(fuzzy("zzz", "users").is_none());
        assert!(fuzzy("user", "users").unwrap() > fuzzy("usr", "users").unwrap());
        assert_eq!(fuzzy("", "x"), Some(0));
    }
}
