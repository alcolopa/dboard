//! Read-only inspection of MySQL scripts. The generated-column check follows dboard's
//! CREATE TABLE and explicit-column INSERT output, including legacy exports.

use crate::split::{Splitter, Stmt};
use crate::sql::Dialect;
use crate::{Error, Result};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};

pub struct MyRestore {
    pub summary: String,
    pub(crate) script: File,
    pub(crate) database: Option<String>,
    pub(crate) connection_id: u32,
    pub(crate) session: u64,
    pub(crate) count: usize,
}

struct Statements<'a> {
    reader: &'a mut dyn BufRead,
    splitter: Splitter,
    raw: Vec<u8>,
    batch: Vec<Stmt>,
    eof: bool,
}

impl<'a> Statements<'a> {
    fn new(reader: &'a mut dyn BufRead) -> Self {
        Self {
            reader,
            splitter: Splitter::new(Dialect::My),
            raw: Vec::new(),
            batch: Vec::new(),
            eof: false,
        }
    }

    fn next(&mut self) -> Result<Option<String>> {
        loop {
            while let Some(stmt) = self.batch.pop() {
                if let Stmt::Sql(sql) = stmt {
                    return Ok(Some(sql));
                }
            }
            if self.eof {
                return Ok(None);
            }
            self.raw.clear();
            if self.reader.read_until(b'\n', &mut self.raw)? == 0 {
                self.eof = true;
                self.splitter.finish(&mut self.batch);
            } else {
                self.splitter.feed_line(&String::from_utf8_lossy(&self.raw), &mut self.batch);
            }
            self.batch.reverse();
        }
    }
}

pub(crate) fn read_statement(reader: &mut dyn std::io::Read) -> Result<String> {
    let mut length = [0; 8];
    reader.read_exact(&mut length)?;
    let mut bytes = vec![0; u64::from_le_bytes(length) as usize];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|e| Error::Db(format!("Could not read the prepared statement: {e}")))
}

type Generated = HashMap<String, HashSet<String>>;

pub(crate) async fn inspect(conn: &mut mysql_async::Conn, path: &std::path::Path, database: (Option<String>, u8), session: u64) -> Result<MyRestore> {
    use std::fmt::Write as _;
    use std::io::{BufWriter, Seek};

    let mut reader = BufReader::new(File::open(path).map_err(|e| Error::Db(format!("Cannot read {}: {e}", path.display())))?);
    if reader.fill_buf()?.starts_with(b"\xef\xbb\xbf") {
        reader.consume(3);
    }
    let dboard = reader.buffer().starts_with(b"-- dboard dump") || reader.buffer().starts_with(b"-- dboard table export");
    let mut script = BufWriter::new(tempfile::tempfile()?);
    let mut statements = Statements::new(&mut reader);
    let lowercase = database.1 != 0;
    let database = database.0;
    let mut current = database.clone();
    let mut catalogs = HashMap::new();
    let mut definitions = HashMap::new();
    let mut summary = format!("Current database: {}\n", database.as_deref().unwrap_or("(none selected)"));
    if let Some(db) = &current {
        inspect_target(conn, db, lowercase, &mut catalogs, &mut summary).await?;
    }
    let mut count = 0;
    while let Some(sql) = statements.next()? {
        count += 1;
        // Save the exact statements inspected so execution need not reopen or split the input.
        script.write_all(&(sql.len() as u64).to_le_bytes())?;
        script.write_all(sql.as_bytes())?;
        let mut tokens = Tokens(&sql);
        if tokens.keyword("USE") {
            current = tokens.name().map(Cow::into_owned);
            if let Some(db) = &current {
                inspect_target(conn, db, lowercase, &mut catalogs, &mut summary).await?;
            }
            continue;
        }

        let create_keyword = tokens.keyword("CREATE");
        if create_keyword && (tokens.keyword("DATABASE") || tokens.keyword("SCHEMA")) {
            if tokens.keyword("IF") {
                tokens.keyword("NOT");
                tokens.keyword("EXISTS");
            }
            if let Some(db) = tokens.name() {
                inspect_target(conn, &db, lowercase, &mut catalogs, &mut summary).await?;
            }
            continue;
        }
        let create = create_keyword && tokens.keyword("TABLE");
        let insert = !create_keyword && tokens.keyword("INSERT") && tokens.keyword("INTO");
        if !create && !insert {
            continue;
        }
        if create && tokens.keyword("IF") {
            tokens.keyword("NOT");
            tokens.keyword("EXISTS");
        }
        let Some(first) = tokens.name() else { continue };
        let (db, table) = if tokens.eat('.') {
            let Some(table) = tokens.name() else { continue };
            (Some(first.as_ref()), table)
        } else {
            (current.as_deref(), first)
        };
        let Some(db) = db else { continue };
        if !catalogs.contains_key(db) {
            inspect_target(conn, db, lowercase, &mut catalogs, &mut summary).await?;
        }
        if !dboard || !tokens.eat('(') {
            continue;
        }
        let table_key = if !lowercase { table } else { Cow::Owned(table.to_lowercase()) };
        if create {
            let generated = tokens.generated_columns();
            if !generated.is_empty() {
                definitions.entry(db.to_owned()).or_insert_with(Generated::new).insert(table_key.into_owned(), generated);
            }
        } else {
            // A dump can include just data. Check both its definitions and the live target.
            let from_dump = definitions.get(db).and_then(|schema| schema.get(table_key.as_ref()));
            let from_target = catalogs[db].get(table_key.as_ref());
            if from_dump.is_none() && from_target.is_none() {
                continue;
            }
            while let Some(column) = tokens.name() {
                let key = column.to_lowercase();
                if [from_dump, from_target].into_iter().flatten().any(|cols| cols.contains(&key)) {
                    return Err(Error::Db(format!(
                        "Restore blocked before execution. Statement {count} assigns a value to generated column {}.{}.{}. Re-export the database with the current version of dboard.\n\n{summary}",
                        Dialect::My.quote(db), Dialect::My.quote(&table_key), Dialect::My.quote(&column)
                    )));
                }
                if !tokens.eat(',') {
                    break;
                }
            }
        }
    }
    if count == 0 {
        return Err(Error::Db("The script contains no SQL statements.".into()));
    }
    if catalogs.is_empty() {
        return Err(Error::Db("Select a target database before restoring this script.".into()));
    }
    writeln!(summary, "\n{count} SQL statement(s). MySQL can keep changes made before a failure.").unwrap();
    if !dboard {
        summary.push_str("Generated-column checks are available for dboard SQL exports. Review the script for other targets or writes.");
    }
    script.flush()?;
    let mut script = script.into_inner().map_err(|e| e.into_error())?;
    script.rewind()?;
    Ok(MyRestore {
        summary,
        script,
        database,
        connection_id: conn.id(),
        session,
        count,
    })
}

async fn inspect_target(conn: &mut mysql_async::Conn, db: &str, lowercase: bool, catalogs: &mut HashMap<String, Generated>, summary: &mut String) -> Result<()> {
    use mysql_async::prelude::Queryable;
    use std::collections::hash_map::Entry;
    use std::fmt::Write as _;

    match catalogs.entry(db.to_owned()) {
        Entry::Occupied(_) => Ok(()),
        Entry::Vacant(entry) => {
            let tables: Vec<(String, Option<String>)> = conn
                .exec(
                    "SELECT t.TABLE_NAME, c.COLUMN_NAME FROM information_schema.TABLES t \
                 LEFT JOIN information_schema.COLUMNS c ON c.TABLE_SCHEMA = t.TABLE_SCHEMA \
                 AND c.TABLE_NAME = t.TABLE_NAME AND c.GENERATION_EXPRESSION <> '' \
                 WHERE t.TABLE_SCHEMA = ? AND t.TABLE_TYPE = 'BASE TABLE' ORDER BY BINARY t.TABLE_NAME",
                    (db,),
                )
                .await?;
            let mut names = Vec::new();
            let mut generated = Generated::new();
            for (table, column) in tables {
                if column.is_none() || names.last() != Some(&table) {
                    names.push(table.clone());
                }
                if let Some(column) = column {
                    let table = if lowercase { table.to_lowercase() } else { table };
                    generated.entry(table).or_default().insert(column.to_lowercase());
                }
            }
            const SHOWN: usize = 5;
            write!(summary, "\n{} · {} existing table(s)", Dialect::My.quote(db), names.len()).unwrap();
            if !names.is_empty() {
                write!(summary, " ({})", names.iter().take(SHOWN).map(|n| Dialect::My.quote(n)).collect::<Vec<_>>().join(", ")).unwrap();
                if names.len() > SHOWN {
                    write!(summary, " and {} more", names.len() - SHOWN).unwrap();
                }
                summary.push_str(". Use an empty target for a clean restore.");
            }
            summary.push('\n');
            entry.insert(generated);
            Ok(())
        }
    }
}

struct Tokens<'a>(&'a str);

impl<'a> Tokens<'a> {
    fn eat(&mut self, ch: char) -> bool {
        if let Some(rest) = self.0.trim_start().strip_prefix(ch) {
            self.0 = rest;
            true
        } else {
            false
        }
    }

    fn word(&mut self) -> &'a str {
        let input = self.0.trim_start();
        let end = input.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '$'))).unwrap_or(input.len());
        self.0 = &input[end..];
        &input[..end]
    }

    fn keyword(&mut self, keyword: &str) -> bool {
        let original = self.0;
        if self.word().eq_ignore_ascii_case(keyword) {
            true
        } else {
            self.0 = original;
            false
        }
    }

    fn name(&mut self) -> Option<Cow<'a, str>> {
        if self.eat('`') {
            let start = self.0;
            self.skip_quote('`');
            let quoted = &start[..start.len() - self.0.len()];
            let name = quoted.strip_suffix('`')?;
            return Some(if name.contains("``") {
                Cow::Owned(name.replace("``", "`"))
            } else {
                Cow::Borrowed(name)
            });
        }
        let word = self.word();
        (!word.is_empty()).then_some(Cow::Borrowed(word))
    }

    fn skip_quote(&mut self, quote: char) {
        while let Some(c) = self.0.chars().next() {
            self.0 = &self.0[c.len_utf8()..];
            if c == '\\' && quote != '`' {
                if let Some(next) = self.0.chars().next() {
                    self.0 = &self.0[next.len_utf8()..];
                }
            } else if c == quote {
                if let Some(rest) = self.0.strip_prefix(quote) {
                    self.0 = rest;
                } else {
                    break;
                }
            }
        }
    }

    fn generated_columns(&mut self) -> HashSet<String> {
        let mut columns = HashSet::new();
        loop {
            let name = self.name();
            let mut depth = 0;
            while let Some(c) = self.0.trim_start().chars().next() {
                self.0 = self.0.trim_start();
                if c.is_alphanumeric() || matches!(c, '_' | '$') {
                    let word = self.word();
                    if depth == 0 && word.eq_ignore_ascii_case("AS") && self.0.trim_start().starts_with('(') {
                        if let Some(name) = &name {
                            columns.insert(name.to_lowercase());
                        }
                    }
                    continue;
                }
                self.0 = &self.0[c.len_utf8()..];
                match c {
                    '(' => depth += 1,
                    ')' if depth == 0 => return columns,
                    ')' => depth -= 1,
                    ',' if depth == 0 => break,
                    '\'' | '"' | '`' => self.skip_quote(c),
                    _ => {}
                }
            }
            if self.0.is_empty() {
                return columns;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_generated_columns_with_quoted_and_unquoted_names() {
        let mut definition =
            Tokens("n int, plain varchar(40) DEFAULT 'AS (ignored)', doubled int AS (n * 2), `with``quote` int GENERATED ALWAYS AS (n + 1) STORED, CHECK (n >= 0))");
        assert_eq!(definition.generated_columns(), HashSet::from(["doubled".into(), "with`quote".into()]));
    }
}
