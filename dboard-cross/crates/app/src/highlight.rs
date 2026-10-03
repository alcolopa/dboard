//! Syntax colouring and bracket matching for the query editor. Works on plain text, one token
//! per run of same-kind characters, so the UI can draw each run at (column x char width, line x line height).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain = 0,
    Keyword = 1,
    Str = 2,
    Number = 3,
    Comment = 4,
    Punct = 5,
    Func = 6,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tok {
    pub line: usize,
    pub col: usize,
    pub text: String,
    pub kind: Kind,
}

const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "GROUP", "BY", "ORDER", "LIMIT", "OFFSET", "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "FULL", "CROSS", "ON", "AS",
    "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "ALTER", "DROP", "TABLE", "INDEX", "VIEW", "DATABASE", "SCHEMA", "TRUNCATE",
    "DISTINCT", "HAVING", "UNION", "ALL", "AND", "OR", "NOT", "NULL", "IS", "LIKE", "ILIKE", "IN", "BETWEEN", "EXISTS", "CASE", "WHEN", "THEN",
    "ELSE", "END", "RETURNING", "WITH", "EXPLAIN", "ANALYZE", "ASC", "DESC", "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "DEFAULT", "UNIQUE",
    "CONSTRAINT", "BEGIN", "COMMIT", "ROLLBACK", "TRUE", "FALSE", "USING", "OVER", "PARTITION", "CAST", "INTERVAL", "GRANT", "REVOKE", "SHOW",
    "DESCRIBE", "USE", "IF", "CASCADE", "ADD", "COLUMN", "RENAME", "TO", "FETCH", "FIRST", "NEXT", "ROWS", "ONLY", "FOR", "LATERAL", "RECURSIVE",
    "DB", "FIND", "AGGREGATE",
];

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Per-character kind for the whole text (so strings and comments can span lines).
fn classify(chars: &[char]) -> Vec<Kind> {
    let n = chars.len();
    let mut k = vec![Kind::Plain; n];
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '-' && next == Some('-') || c == '/' && next == Some('/') || c == '#' {
            while i < n && chars[i] != '\n' {
                k[i] = Kind::Comment;
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let start = i;
            i += 2;
            while i < n && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(n);
            for x in &mut k[start..i] {
                *x = Kind::Comment;
            }
        } else if c == '\'' || c == '"' || c == '`' {
            let q = c;
            let start = i;
            i += 1;
            while i < n {
                if chars[i] == '\\' && q != '`' {
                    i += 2;
                    continue;
                }
                if chars[i] == q {
                    if chars.get(i + 1) == Some(&q) {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            let end = i.min(n);
            for x in &mut k[start..end] {
                *x = Kind::Str;
            }
        } else if c.is_ascii_digit() && (i == 0 || !is_word(chars[i - 1])) {
            while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                k[i] = Kind::Number;
                i += 1;
            }
        } else if is_word(c) {
            let start = i;
            while i < n && is_word(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let kind = if KEYWORDS.contains(&word.to_uppercase().as_str()) {
                Kind::Keyword
            } else if chars[i..].iter().find(|c| !c.is_whitespace() || **c == '\n') == Some(&'(') {
                Kind::Func
            } else {
                Kind::Plain
            };
            for x in &mut k[start..i] {
                *x = kind;
            }
        } else {
            if !c.is_whitespace() {
                k[i] = Kind::Punct;
            }
            i += 1;
        }
    }
    k
}

pub fn tokens(text: &str) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let kinds = classify(&chars);
    let mut out: Vec<Tok> = Vec::new();
    let (mut line, mut col) = (0usize, 0usize);
    let mut cur: Option<Tok> = None;
    for (i, &c) in chars.iter().enumerate() {
        if c == '\n' {
            out.extend(cur.take());
            line += 1;
            col = 0;
            continue;
        }
        if c.is_whitespace() {
            out.extend(cur.take());
            col += if c == '\t' { 4 } else { 1 };
            continue;
        }
        let kind = kinds[i];
        // Strings and comments keep their inner spaces; everything else breaks at whitespace.
        match &mut cur {
            Some(t) if t.kind == kind && t.line == line && t.col + t.text.chars().count() == col => t.text.push(c),
            _ => {
                out.extend(cur.take());
                cur = Some(Tok { line, col, text: c.to_string(), kind });
            }
        }
        col += 1;
    }
    out.extend(cur.take());
    // Re-join the pieces of strings / comments that were split at spaces so spacing is preserved
    // by position (each piece keeps its own column, so nothing else is needed).
    out
}

/// Positions `(line, col)` of the bracket next to `byte_offset` and its partner, if any.
pub fn match_bracket(text: &str, byte_offset: usize) -> Option<[(usize, usize); 2]> {
    let chars: Vec<char> = text.chars().collect();
    let kinds = classify(&chars);
    // char index for the byte offset
    let idx = text.char_indices().position(|(b, _)| b >= byte_offset).unwrap_or(chars.len());
    let is_open = |c: char| matches!(c, '(' | '[' | '{');
    let is_close = |c: char| matches!(c, ')' | ']' | '}');
    let candidates = [idx.checked_sub(1), Some(idx)];
    for at in candidates.into_iter().flatten() {
        let Some(&c) = chars.get(at) else { continue };
        if kinds[at] == Kind::Str || kinds[at] == Kind::Comment || !(is_open(c) || is_close(c)) {
            continue;
        }
        let want = match c {
            '(' => ')',
            ')' => '(',
            '[' => ']',
            ']' => '[',
            '{' => '}',
            _ => '{',
        };
        let mut depth = 0i32;
        let found = if is_open(c) {
            (at + 1..chars.len()).find(|&j| {
                if kinds[j] == Kind::Str || kinds[j] == Kind::Comment {
                    return false;
                }
                if chars[j] == c {
                    depth += 1;
                } else if chars[j] == want {
                    if depth == 0 {
                        return true;
                    }
                    depth -= 1;
                }
                false
            })
        } else {
            (0..at).rev().find(|&j| {
                if kinds[j] == Kind::Str || kinds[j] == Kind::Comment {
                    return false;
                }
                if chars[j] == c {
                    depth += 1;
                } else if chars[j] == want {
                    if depth == 0 {
                        return true;
                    }
                    depth -= 1;
                }
                false
            })
        };
        if let Some(j) = found {
            let pos = |p: usize| {
                let before = &chars[..p];
                let line = before.iter().filter(|c| **c == '\n').count();
                let col = before.iter().rev().take_while(|c| **c != '\n').count();
                (line, col)
            };
            return Some([pos(at), pos(j)]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(String, Kind)> {
        tokens(text).into_iter().map(|t| (t.text, t.kind)).collect()
    }

    #[test]
    fn colours_sql() {
        let k = kinds("SELECT count(*), 'a b' FROM t -- hi\nWHERE id = 42");
        assert!(k.contains(&("SELECT".into(), Kind::Keyword)));
        assert!(k.contains(&("count".into(), Kind::Func)));
        assert!(k.iter().any(|(t, kind)| t == "'a" && *kind == Kind::Str));
        assert!(k.iter().any(|(t, kind)| t == "b'" && *kind == Kind::Str));
        assert!(k.contains(&("42".into(), Kind::Number)));
        assert!(k.iter().any(|(t, kind)| t == "--" && *kind == Kind::Comment));
        assert!(k.contains(&("t".into(), Kind::Plain)));
    }

    #[test]
    fn positions_follow_lines_and_columns() {
        let t = tokens("a\n  bc");
        assert_eq!((t[0].line, t[0].col), (0, 0));
        assert_eq!((t[1].line, t[1].col, t[1].text.as_str()), (1, 2, "bc"));
    }

    #[test]
    fn block_comments_span_lines() {
        let k = kinds("/* a\nb */ x");
        assert!(k.iter().filter(|(_, kind)| *kind == Kind::Comment).count() >= 3);
        assert!(k.contains(&("x".into(), Kind::Plain)));
    }

    #[test]
    fn matches_brackets_skipping_strings() {
        let text = "f(a, ')', (b))";
        assert_eq!(match_bracket(text, 2), Some([(0, 1), (0, 13)]));
        assert_eq!(match_bracket(text, 14), Some([(0, 13), (0, 1)]));
        assert_eq!(match_bracket("abc", 1), None);
    }
}
