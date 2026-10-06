//! SQLite driver (a local file). `database` holds the file path; there are no users or servers.

use crate::dump::{Progress, DumpOptions, DumpStats, ImportOptions, ImportStats};
use crate::model::*;
use crate::split::{Splitter, Stmt};
use crate::sql::{literal, Dialect};
use crate::{Error, Result};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::io::{BufRead, Write};
use std::time::Instant;

const D: Dialect = Dialect::Pg; // identifier quoting and string literals are the same as PostgreSQL's

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Db(match &e {
            rusqlite::Error::SqliteFailure(f, Some(msg)) => match f.code {
                rusqlite::ErrorCode::ConstraintViolation => format!("Constraint failed: {msg}"),
                rusqlite::ErrorCode::ReadOnly => "This database is read-only.".to_string(),
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => "The database file is locked by another program.".to_string(),
                _ => msg.clone(),
            },
            other => other.to_string(),
        })
    }
}

pub struct Sqlite {
    conn: Connection,
    path: String,
}

fn q(s: &str) -> String {
    D.quote(s)
}

fn cell(v: ValueRef<'_>) -> Cell {
    match v {
        ValueRef::Null => None,
        ValueRef::Integer(i) => Some(i.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Some(format!("<blob {} bytes>", b.len())),
    }
}

fn lit(v: &Cell) -> String {
    match v {
        None => "NULL".into(),
        Some(s) => literal(D, s),
    }
}

impl Sqlite {
    pub async fn connect(c: &ConnectionConfig, _password: &str) -> Result<Self> {
        let path = c.database.trim().to_string();
        if path.is_empty() {
            return Err(Error::Db("Enter the path of the SQLite file.".into()));
        }
        if path != ":memory:" && !std::path::Path::new(&path).exists() {
            return Err(Error::Db(format!("No file at {path}. (A new database is created from a query with ATTACH, or by opening an empty file.)")));
        }
        let flags = if c.read_only { OpenFlags::SQLITE_OPEN_READ_ONLY } else { OpenFlags::SQLITE_OPEN_READ_WRITE } | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(&path, flags)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(Self { conn, path })
    }

    pub async fn version(&mut self) -> Result<String> {
        let v: String = self.conn.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
        Ok(format!("SQLite {v}"))
    }

    pub async fn metadata(&mut self) -> Result<Metadata> {
        let mut tables: Vec<Table> = Vec::new();
        let mut objects: Vec<DbObject> = Vec::new();
        let master: Vec<(String, String, String, Option<String>)> = {
            let mut st = self.conn.prepare("SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        for (kind, name, tbl, sql) in &master {
            match kind.as_str() {
                "table" | "view" => {
                    let mut columns: Vec<Column> = Vec::new();
                    // A view whose underlying table is gone makes this PRAGMA fail; keep it listed, without columns.
                    let infos: Vec<(String, String, i64, Option<String>, i64)> = match self.conn.prepare(&format!("PRAGMA table_info({})", literal(D, name))) {
                        Ok(mut st) => match st.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, i64>(5)?))) {
                            Ok(rows) => rows.collect::<std::result::Result<_, _>>().unwrap_or_default(),
                            Err(_) if kind == "view" => Vec::new(),
                            Err(e) => return Err(e.into()),
                        },
                        Err(_) if kind == "view" => Vec::new(),
                        Err(e) => return Err(e.into()),
                    };
                    for (cname, ty, notnull, dflt, pk) in infos {
                        columns.push(Column {
                            name: cname,
                            type_name: if ty.is_empty() { "any".into() } else { ty.to_lowercase() },
                            nullable: notnull == 0 && pk == 0,
                            is_primary_key: pk > 0,
                            default: dflt,
                            fk: None,
                        });
                    }
                    if kind == "table" {
                        let mut st = self.conn.prepare(&format!("PRAGMA foreign_key_list({})", literal(D, name)))?;
                        let fks: Vec<(String, String, Option<String>)> = st
                            .query_map([], |r| Ok((r.get::<_, String>(3)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(4)?)))?
                            .collect::<std::result::Result<_, _>>()?;
                        for (from, to_table, to_col) in fks {
                            if let Some(c) = columns.iter_mut().find(|c| c.name == from) {
                                c.fk = Some(format!("main.{to_table}({})", to_col.unwrap_or_else(|| "rowid".into())));
                            }
                        }
                    }
                    tables.push(Table {
                        schema: "main".into(),
                        name: name.clone(),
                        kind: if kind == "view" { TableKind::View } else { TableKind::Table },
                        columns,
                        estimated_rows: None,
                        size_bytes: None,
                        indexes: Vec::new(),
                        keyless_edit: false,
                    });
                }
                "index" => {
                    if let Some(sql) = sql {
                        if let Some(t) = tables.iter_mut().find(|t| t.name == *tbl) {
                            t.indexes.push(sql.clone());
                        } else {
                            objects.push(DbObject { schema: "main".into(), name: name.clone(), kind: ObjectKind::Index, detail: tbl.clone() });
                        }
                    }
                }
                "trigger" => objects.push(DbObject { schema: "main".into(), name: name.clone(), kind: ObjectKind::Trigger, detail: tbl.clone() }),
                _ => {}
            }
        }
        // Indexes were seen before their table in name order when the index sorts first; attach them now.
        for (kind, name, tbl, sql) in &master {
            if kind == "index" {
                if let (Some(sql), Some(t)) = (sql, tables.iter_mut().find(|t| t.name == *tbl)) {
                    if !t.indexes.contains(sql) {
                        t.indexes.push(sql.clone());
                    }
                    objects.retain(|o| !(o.kind == ObjectKind::Index && o.name == *name));
                }
            }
        }
        Ok(Metadata { tables, objects })
    }

    fn run(&self, sql: &str) -> Result<Rows> {
        let started = Instant::now();
        let mut st = self.conn.prepare(sql)?;
        let ncols = st.column_count();
        let columns: Vec<String> = st.column_names().into_iter().map(String::from).collect();
        if ncols == 0 {
            let n = st.execute([])?;
            return Ok(Rows { columns: vec!["Result".into()], rows: vec![vec![Some(format!("{n} row(s) affected"))]], duration_ms: started.elapsed().as_secs_f64() * 1000.0, total_estimate: None });
        }
        let mut out: Vec<Vec<Cell>> = Vec::new();
        let mut rows = st.query([])?;
        while let Some(r) = rows.next()? {
            out.push((0..ncols).map(|i| r.get_ref(i).map(cell)).collect::<std::result::Result<_, _>>()?);
        }
        Ok(Rows { columns, rows: out, duration_ms: started.elapsed().as_secs_f64() * 1000.0, total_estimate: None })
    }

    pub async fn query(&mut self, sql_text: &str) -> Result<Rows> {
        self.run(sql_text)
    }

    pub async fn fetch(&mut self, t: &Table, p: &Page) -> Result<Rows> {
        let mut sql = format!("SELECT * FROM {}", q(&t.name));
        if let Some(f) = p.filter.as_deref().filter(|f| !f.trim().is_empty()) {
            sql.push_str(&format!(" WHERE {f}"));
        }
        if let Some(col) = &p.sort_column {
            sql.push_str(&format!(" ORDER BY {} {}", q(col), if p.sort_ascending { "ASC" } else { "DESC" }));
        }
        sql.push_str(&format!(" LIMIT {} OFFSET {}", p.limit, p.offset));
        let mut r = self.run(&sql)?;
        // total for the "of N" in the footer
        let mut count = format!("SELECT COUNT(*) FROM {}", q(&t.name));
        if let Some(f) = p.filter.as_deref().filter(|f| !f.trim().is_empty()) {
            count.push_str(&format!(" WHERE {f}"));
        }
        r.total_estimate = self.conn.query_row(&count, [], |x| x.get::<_, i64>(0)).ok();
        Ok(r)
    }

    fn where_key(t: &Table, key: &[Cell]) -> Result<String> {
        let pks = t.primary_keys();
        if pks.is_empty() {
            return Err(Error::Unsafe("This table has no primary key, so rows cannot be addressed safely.".into()));
        }
        Ok(pks
            .iter()
            .zip(key)
            .map(|(c, v)| match v {
                None => format!("{} IS NULL", q(&c.name)),
                Some(_) => format!("{} = {}", q(&c.name), lit(v)),
            })
            .collect::<Vec<_>>()
            .join(" AND "))
    }

    fn affected(&self, sql: &str, expect_one: bool) -> Result<()> {
        let n = self.conn.execute(sql, [])?;
        if expect_one && n != 1 {
            return Err(Error::Db(format!("Expected to change 1 row but {n} changed; nothing was kept.")));
        }
        Ok(())
    }

    pub async fn update(&mut self, t: &Table, column: &str, key: &[Cell], new: Option<&str>) -> Result<()> {
        let w = Self::where_key(t, key)?;
        let v = lit(&new.map(String::from));
        self.affected(&format!("UPDATE {} SET {} = {v} WHERE {w}", q(&t.name), q(column)), true)
    }

    pub async fn insert(&mut self, t: &Table, vals: &[(String, String)]) -> Result<()> {
        let v: Vec<(String, Cell)> = vals.iter().map(|(k, x)| (k.clone(), Some(x.clone()))).collect();
        self.insert_nullable(t, &v).await
    }

    pub async fn insert_nullable(&mut self, t: &Table, vals: &[(String, Cell)]) -> Result<()> {
        let sql = if vals.is_empty() {
            format!("INSERT INTO {} DEFAULT VALUES", q(&t.name))
        } else {
            format!("INSERT INTO {} ({}) VALUES ({})", q(&t.name), vals.iter().map(|(c, _)| q(c)).collect::<Vec<_>>().join(", "), vals.iter().map(|(_, v)| lit(v)).collect::<Vec<_>>().join(", "))
        };
        self.affected(&sql, false)
    }

    pub async fn delete(&mut self, t: &Table, key: &[Cell]) -> Result<()> {
        let w = Self::where_key(t, key)?;
        self.affected(&format!("DELETE FROM {} WHERE {w}", q(&t.name)), true)
    }

    pub async fn truncate(&mut self, t: &Table) -> Result<()> {
        self.affected(&format!("DELETE FROM {}", q(&t.name)), false)
    }

    pub async fn drop_table(&mut self, t: &Table) -> Result<()> {
        let what = if t.kind == TableKind::View { "VIEW" } else { "TABLE" };
        self.affected(&format!("DROP {what} {}", q(&t.name)), false)
    }

    pub async fn ddl(&mut self, t: &Table) -> Result<String> {
        let mut parts: Vec<String> = Vec::new();
        let main: Option<String> = self.conn.query_row("SELECT sql FROM sqlite_master WHERE name = ?1", [&t.name], |r| r.get(0)).ok();
        parts.extend(main.map(|s| format!("{s};")));
        parts.extend(t.indexes.iter().map(|i| format!("{i};")));
        Ok(parts.join("\n"))
    }

    /// EXPLAIN QUERY PLAN as an indented tree.
    pub async fn explain(&mut self, sql_text: &str, _analyze: bool) -> Result<Rows> {
        let started = Instant::now();
        let mut st = self.conn.prepare(&format!("EXPLAIN QUERY PLAN {}", sql_text.trim().trim_end_matches(';')))?;
        let items: Vec<(i64, i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(3)?)))?.collect::<std::result::Result<_, _>>()?;
        let depth_of = |parent: i64| -> usize {
            let mut d = 0;
            let mut p = parent;
            while p != 0 {
                d += 1;
                p = items.iter().find(|(id, _, _)| *id == p).map(|(_, par, _)| *par).unwrap_or(0);
            }
            d
        };
        let rows = items.iter().map(|(_, parent, detail)| vec![Some(format!("{}{}{detail}", "   ".repeat(depth_of(*parent).saturating_sub(0)), if *parent == 0 { "" } else { "↳ " }))]).collect();
        Ok(Rows { columns: vec!["Plan".into()], rows, duration_ms: started.elapsed().as_secs_f64() * 1000.0, total_estimate: None })
    }

    pub async fn object_def(&mut self, o: &DbObject) -> Result<String> {
        let sql: Option<String> = self.conn.query_row("SELECT sql FROM sqlite_master WHERE name = ?1", [&o.name], |r| r.get(0)).ok();
        Ok(sql.map(|s| format!("{s};")).unwrap_or_else(|| format!("-- no definition found for {}", o.name)))
    }

    pub async fn list_databases(&mut self) -> Result<Vec<String>> {
        Ok(vec![self.path.rsplit(['/', '\\']).next().unwrap_or("main").to_string()])
    }

    pub async fn current_database(&mut self) -> Result<String> {
        Ok(self.path.rsplit(['/', '\\']).next().unwrap_or("main").to_string())
    }

    pub async fn dump(&mut self, opts: &DumpOptions, out: &mut dyn Write, progress: &mut dyn FnMut(Progress)) -> Result<DumpStats> {
        let meta = self.metadata().await?;
        let mut stats = DumpStats { tables: 0, rows: 0, objects: 0 };
        writeln!(out, "-- dboard dump\n-- SQLite database {}\n\nPRAGMA foreign_keys = OFF;\nBEGIN TRANSACTION;\n", self.path)?;
        let total = meta.tables.iter().filter(|t| t.kind == TableKind::Table).count();
        for (done, t) in meta.tables.iter().filter(|t| t.kind == TableKind::Table).enumerate() {
            progress(Progress::at(format!("Writing {}…", t.name), done, total));
            if opts.schema {
                writeln!(out, "{}\n", self.ddl_text(&t.name)?)?;
            }
            stats.tables += 1;
            if opts.data {
                let mut st = self.conn.prepare(&format!("SELECT * FROM {}", q(&t.name)))?;
                let n = st.column_count();
                let mut rows = st.query([])?;
                while let Some(r) = rows.next()? {
                    let vals: Vec<String> = (0..n)
                        .map(|i| match r.get_ref(i) {
                            Ok(ValueRef::Null) | Err(_) => "NULL".into(),
                            Ok(ValueRef::Integer(v)) => v.to_string(),
                            Ok(ValueRef::Real(v)) => v.to_string(),
                            Ok(ValueRef::Text(v)) => literal(D, &String::from_utf8_lossy(v)),
                            Ok(ValueRef::Blob(b)) => format!("X'{}'", b.iter().map(|x| format!("{x:02x}")).collect::<String>()),
                        })
                        .collect();
                    writeln!(out, "INSERT INTO {} VALUES ({});", q(&t.name), vals.join(", "))?;
                    stats.rows += 1;
                }
                writeln!(out)?;
            }
        }
        if opts.schema {
            let mut st = self.conn.prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL AND type IN ('index','view','trigger') AND name NOT LIKE 'sqlite_%' ORDER BY type, name")?;
            let defs: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
            for d in defs {
                writeln!(out, "{d};")?;
                stats.objects += 1;
            }
        }
        writeln!(out, "\nCOMMIT;\nPRAGMA foreign_keys = ON;")?;
        Ok(stats)
    }

    fn ddl_text(&self, name: &str) -> Result<String> {
        let sql: String = self.conn.query_row("SELECT sql FROM sqlite_master WHERE name = ?1", [name], |r| r.get(0))?;
        Ok(format!("{sql};"))
    }

    pub async fn import(&mut self, reader: &mut dyn BufRead, opts: &ImportOptions, progress: &mut dyn FnMut(Progress)) -> Result<ImportStats> {
        let mut stats = ImportStats { statements: 0, rows_copied: 0, errors: Vec::new() };
        let mut splitter = Splitter::new(D);
        let mut pending: Vec<Stmt> = Vec::new();
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            splitter.feed_line(line.trim_end_matches(['\n', '\r']), &mut pending);
        }
        splitter.finish(&mut pending);
        for s in pending {
            let Stmt::Sql(sql) = s else { continue };
            if sql.trim().is_empty() {
                continue;
            }
            match self.conn.execute_batch(&sql) {
                Ok(()) => stats.statements += 1,
                Err(e) => {
                    let msg = format!("{e}: {}", sql.chars().take(80).collect::<String>());
                    if opts.stop_on_error {
                        let _ = self.conn.execute_batch("ROLLBACK");
                        return Err(Error::Db(msg));
                    }
                    stats.errors.push(msg);
                }
            }
            if stats.statements % 200 == 0 {
                progress(format!("Ran {} statement(s)…", stats.statements).into());
            }
        }
        Ok(stats)
    }
}

impl Sqlite {
    pub async fn list_users(&mut self) -> Result<Vec<UserInfo>> {
        Ok(Vec::new())
    }
    pub async fn user_grants(&mut self, _u: &UserInfo) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
    pub async fn create_user(&mut self, _n: &NewUser) -> Result<()> {
        Err(Error::Db("SQLite is a file and has no users; control access with file permissions.".into()))
    }
    pub async fn set_password(&mut self, _u: &UserInfo, _password: &str) -> Result<()> {
        Err(Error::Db("SQLite has no users.".into()))
    }
    pub async fn drop_user(&mut self, _u: &UserInfo) -> Result<()> {
        Err(Error::Db("SQLite has no users.".into()))
    }
    pub async fn set_access(&mut self, _u: &UserInfo, _level: AccessLevel) -> Result<()> {
        Err(Error::Db("SQLite has no users.".into()))
    }
}
