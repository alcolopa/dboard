//! `{{name}}` / `{{name=default}}` placeholders in saved queries.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Var {
    pub name: String,
    pub default: String,
}

fn spans(text: &str) -> Vec<(usize, usize, Var)> {
    let mut out = Vec::new();
    let mut rest = 0;
    while let Some(open) = text[rest..].find("{{").map(|i| i + rest) {
        let Some(close) = text[open + 2..].find("}}").map(|i| i + open + 2) else { break };
        let inner = &text[open + 2..close];
        let (name, default) = inner.split_once('=').unwrap_or((inner, ""));
        let name = name.trim();
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
            out.push((open, close + 2, Var { name: name.to_string(), default: default.trim().to_string() }));
            rest = close + 2;
        } else {
            rest = open + 2;
        }
    }
    out
}

/// Distinct variables in order of first appearance (the first default wins).
pub fn find(text: &str) -> Vec<Var> {
    let mut seen: Vec<Var> = Vec::new();
    for (_, _, v) in spans(text) {
        if !seen.iter().any(|s| s.name == v.name) {
            seen.push(v);
        }
    }
    seen
}

pub fn substitute(text: &str, values: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut last = 0;
    for (start, end, v) in spans(text) {
        out.push_str(&text[last..start]);
        out.push_str(values.get(&v.name).map(String::as_str).unwrap_or(&v.default));
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_distinct_variables_with_defaults() {
        let v = find("select * from t where a = {{id}} and b = '{{ name = bob }}' or c = {{id}}");
        assert_eq!(v, vec![Var { name: "id".into(), default: "".into() }, Var { name: "name".into(), default: "bob".into() }]);
    }

    #[test]
    fn substitutes_values_then_defaults() {
        let mut m = HashMap::new();
        m.insert("id".to_string(), "7".to_string());
        assert_eq!(substitute("a={{id}} b={{n=x}} c={{id}}", &m), "a=7 b=x c=7");
    }

    #[test]
    fn leaves_other_braces_alone() {
        assert!(find("select '{{ not valid }}', {a: 1}, {{}}").is_empty());
        assert_eq!(substitute("{\"a\": {\"b\": 1}}", &HashMap::new()), "{\"a\": {\"b\": 1}}");
    }
}
