//! Streaming SQL script splitter for imports. It understands what a real dump contains: quoted
//! strings and identifiers, comments (including MySQL `/*! ... */` conditional ones), PostgreSQL
//! dollar quoting, `COPY ... FROM stdin` data blocks, `DELIMITER` lines and psql `\` commands.

use crate::sql::Dialect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Sql(String),
    /// `COPY ... FROM stdin;` plus the tab-separated rows that followed it (without the `\.` line).
    Copy { head: String, data: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Normal,
    Single { backslash: bool },
    Double,
    Backtick,
    Block { depth: u32, keep: bool },
    Dollar(String),
}

pub struct Splitter {
    dialect: Dialect,
    delimiter: Vec<char>,
    buf: String,
    state: State,
    copy: Option<(String, String)>,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Splitter {
    pub fn new(dialect: Dialect) -> Self {
        Self { dialect, delimiter: vec![';'], buf: String::new(), state: State::Normal, copy: None }
    }

    fn emit(&mut self, out: &mut Vec<Stmt>) {
        let text = std::mem::take(&mut self.buf);
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let up = text.to_uppercase();
        if self.dialect == Dialect::Pg && up.starts_with("COPY ") && up.contains(" FROM STDIN") {
            self.copy = Some((text.to_string(), String::new()));
        } else {
            out.push(Stmt::Sql(text.to_string()));
        }
    }

    pub fn feed_line(&mut self, line: &str, out: &mut Vec<Stmt>) {
        if let Some((head, data)) = &mut self.copy {
            if line.trim_end() == "\\." {
                out.push(Stmt::Copy { head: std::mem::take(head), data: std::mem::take(data) });
                self.copy = None;
            } else {
                data.push_str(line);
                if !line.ends_with('\n') {
                    data.push('\n');
                }
            }
            return;
        }
        if self.state == State::Normal && self.buf.trim().is_empty() {
            let t = line.trim_start();
            if t.starts_with('\\') {
                return; // psql meta command (\connect, \restrict, ...)
            }
            if self.dialect == Dialect::My && t.len() > 9 && t[..9].eq_ignore_ascii_case("delimiter") && t[9..].starts_with(char::is_whitespace) {
                let d = t[9..].trim();
                if !d.is_empty() {
                    self.delimiter = d.chars().collect();
                }
                return;
            }
        }
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();
            match self.state.clone() {
                State::Normal => {
                    if chars[i..].starts_with(&self.delimiter) {
                        self.emit(out);
                        i += self.delimiter.len();
                        if self.copy.is_some() {
                            // The rest of this line is not part of the statement; data starts on the next line.
                            return;
                        }
                        continue;
                    }
                    let line_comment = (c == '-' && next == Some('-') && (self.dialect == Dialect::Pg || chars.get(i + 2).is_none_or(|x| x.is_whitespace())))
                        || (c == '#' && self.dialect == Dialect::My);
                    if line_comment {
                        self.buf.push('\n');
                        return;
                    }
                    if c == '/' && next == Some('*') {
                        let keep = self.dialect == Dialect::My && matches!(chars.get(i + 2), Some('!') | Some('+'));
                        self.state = State::Block { depth: 1, keep };
                        if keep {
                            self.buf.push_str("/*");
                        }
                        i += 2;
                        continue;
                    }
                    match c {
                        '\'' => {
                            let backslash = match self.dialect {
                                Dialect::My => true,
                                Dialect::Pg => {
                                    let mut it = self.buf.chars().rev();
                                    matches!(it.next(), Some('e' | 'E')) && !it.next().is_some_and(is_ident)
                                }
                            };
                            self.state = State::Single { backslash };
                        }
                        '"' => self.state = State::Double,
                        '`' if self.dialect == Dialect::My => self.state = State::Backtick,
                        '$' if self.dialect == Dialect::Pg && !self.buf.chars().last().is_some_and(is_ident) => {
                            let mut j = i + 1;
                            while j < chars.len() && is_ident(chars[j]) {
                                j += 1;
                            }
                            let tag_ok = chars.get(j) == Some(&'$') && !chars.get(i + 1).is_some_and(|d| d.is_ascii_digit());
                            if tag_ok {
                                let tag: String = chars[i..=j].iter().collect();
                                self.buf.push_str(&tag);
                                self.state = State::Dollar(tag);
                                i = j + 1;
                                continue;
                            }
                        }
                        _ => {}
                    }
                    self.buf.push(c);
                }
                State::Single { backslash } => {
                    self.buf.push(c);
                    if backslash && c == '\\' {
                        if let Some(n) = next {
                            self.buf.push(n);
                            i += 1;
                        }
                    } else if c == '\'' {
                        if next == Some('\'') {
                            self.buf.push('\'');
                            i += 1;
                        } else {
                            self.state = State::Normal;
                        }
                    }
                }
                State::Double | State::Backtick => {
                    let q = if self.state == State::Double { '"' } else { '`' };
                    self.buf.push(c);
                    if c == q {
                        if next == Some(q) {
                            self.buf.push(q);
                            i += 1;
                        } else {
                            self.state = State::Normal;
                        }
                    }
                }
                State::Block { depth, keep } => {
                    if c == '*' && next == Some('/') {
                        if keep {
                            self.buf.push_str("*/");
                        }
                        self.state = if depth <= 1 { State::Normal } else { State::Block { depth: depth - 1, keep } };
                        i += 2;
                        continue;
                    }
                    if c == '/' && next == Some('*') && self.dialect == Dialect::Pg {
                        self.state = State::Block { depth: depth + 1, keep };
                        i += 2;
                        continue;
                    }
                    if keep {
                        self.buf.push(c);
                    }
                }
                State::Dollar(tag) => {
                    let t: Vec<char> = tag.chars().collect();
                    if chars[i..].starts_with(&t) {
                        self.buf.push_str(&tag);
                        self.state = State::Normal;
                        i += t.len();
                        continue;
                    }
                    self.buf.push(c);
                }
            }
            i += 1;
        }
    }

    /// Flush a final statement that has no terminating delimiter.
    pub fn finish(&mut self, out: &mut Vec<Stmt>) {
        if let Some((head, data)) = self.copy.take() {
            out.push(Stmt::Copy { head, data });
        }
        self.emit(out);
    }
}

pub fn split_all(d: Dialect, text: &str) -> Vec<Stmt> {
    let mut s = Splitter::new(d);
    let mut out = Vec::new();
    for l in text.split_inclusive('\n') {
        s.feed_line(l, &mut out);
    }
    s.finish(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sql(v: &[Stmt]) -> Vec<&str> {
        v.iter().map(|s| if let Stmt::Sql(t) = s { t.as_str() } else { "<copy>" }).collect()
    }

    #[test]
    fn splits_on_semicolons_outside_quotes_and_comments() {
        let v = split_all(Dialect::Pg, "-- hi;\nSELECT 'a;b'; /* x; y */ SELECT \"c;d\";\nSELECT 'it''s;'");
        assert_eq!(sql(&v), ["SELECT 'a;b'", "SELECT \"c;d\"", "SELECT 'it''s;'"]);
    }

    #[test]
    fn dollar_quoted_bodies_stay_whole() {
        let src = "CREATE FUNCTION f() RETURNS int AS $body$\nBEGIN\n  RETURN 1; -- ;\nEND;\n$body$ LANGUAGE plpgsql;\nSELECT $1;";
        let v = split_all(Dialect::Pg, src);
        assert_eq!(v.len(), 2);
        assert!(sql(&v)[0].ends_with("LANGUAGE plpgsql"));
        assert!(sql(&v)[0].contains("RETURN 1;"));
        assert_eq!(sql(&v)[1], "SELECT $1");
    }

    #[test]
    fn copy_blocks_carry_their_rows() {
        let v = split_all(Dialect::Pg, "COPY public.t (a, b) FROM stdin;\n1\tx\n2\t\\N\n\\.\nSELECT 1;\n");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], Stmt::Copy { head: "COPY public.t (a, b) FROM stdin".into(), data: "1\tx\n2\t\\N\n".into() });
    }

    #[test]
    fn psql_meta_lines_are_skipped() {
        let v = split_all(Dialect::Pg, "\\connect foo\nSELECT 1;\n\\restrict abc\n");
        assert_eq!(sql(&v), ["SELECT 1"]);
    }

    #[test]
    fn escaped_strings_honour_backslashes() {
        let v = split_all(Dialect::Pg, "SELECT E'a\\';b'; SELECT 'a\\'; SELECT 2;");
        assert_eq!(sql(&v), ["SELECT E'a\\';b'", "SELECT 'a\\'", "SELECT 2"]);
    }

    #[test]
    fn mysql_delimiters_backslashes_and_conditional_comments() {
        let src = "/*!40101 SET NAMES utf8 */;\nINSERT INTO t VALUES ('a\\';b');\nDELIMITER ;;\nCREATE TRIGGER x BEFORE INSERT ON t FOR EACH ROW BEGIN SET NEW.a = 1; END;;\nDELIMITER ;\nSELECT 1;\n# note;\nSELECT 2;";
        let v = split_all(Dialect::My, src);
        let s = sql(&v);
        assert_eq!(s[0], "/*!40101 SET NAMES utf8 */");
        assert_eq!(s[1], "INSERT INTO t VALUES ('a\\';b')");
        assert!(s[2].starts_with("CREATE TRIGGER") && s[2].ends_with("END"));
        assert_eq!(&s[3..], ["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn mysql_double_dash_needs_a_space() {
        let v = split_all(Dialect::My, "SELECT 1--1;\nSELECT 2 -- c;\n;");
        assert_eq!(sql(&v), ["SELECT 1--1", "SELECT 2"]);
    }
}
