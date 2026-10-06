use crate::admin;
use crate::dump::{Progress, hex_literal, is_binary, is_numeric, sql_value, strip_definer, DumpOptions, DumpStats, ImportOptions, ImportStats, InsertWriter};
use crate::model::*;
use crate::split::{Splitter, Stmt};
use crate::sql::{self, Dialect};
use crate::{Error, Result};
use mysql_async::prelude::*;
use mysql_async::{Conn, OptsBuilder, Params, Row, SslOpts, Value};
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

const D: Dialect = Dialect::My;

pub struct My {
    conn: Conn,
    opts: mysql_async::Opts,
    is_mariadb: bool,
    /// Only this database is listed (None = every non-system database).
    scope: Option<String>,
}

impl From<mysql_async::Error> for Error {
    fn from(e: mysql_async::Error) -> Self {
        match &e {
            mysql_async::Error::Server(s) => Error::Db(humanize(s.code, &s.message)),
            _ => Error::Db(e.to_string()),
        }
    }
}

fn humanize(code: u16, message: &str) -> String {
    match code {
        1062 => "A record with this unique value already exists in the table.".into(),
        1451 | 1452 => "This change violates a foreign key constraint.".into(),
        1048 => "A required (NOT NULL) column cannot be empty.".into(),
        1264 | 1366 | 1292 | 1265 => format!("Invalid value for this column type: {message}"),
        1044 | 1045 | 1142 | 1143 => "Permission denied for this operation.".into(),
        _ => message.to_string(),
    }
}

pub fn value_to_cell(v: &Value) -> Cell {
    Some(match v {
        Value::NULL => return None,
        Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Date(y, mo, d, h, mi, s, us) => {
            if *h == 0 && *mi == 0 && *s == 0 && *us == 0 {
                format!("{y:04}-{mo:02}-{d:02}")
            } else if *us == 0 {
                format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
            } else {
                format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{us:06}")
            }
        }
        Value::Time(neg, days, h, mi, s, _) => {
            format!("{}{:02}:{mi:02}:{s:02}", if *neg { "-" } else { "" }, u32::from(*days) * 24 + u32::from(*h))
        }
    })
}

fn rows_to_cells(rows: &[Row], width: usize) -> Vec<Vec<Cell>> {
    rows.iter().map(|r| (0..width).map(|i| r.as_ref(i).and_then(value_to_cell)).collect()).collect()
}

impl My {
    pub async fn connect(c: &ConnectionConfig, password: &str) -> Result<Self> {
        let base = OptsBuilder::default()
            .ip_or_hostname(c.host.clone())
            .tcp_port(c.port)
            .user(Some(c.username.clone()))
            .pass(Some(password.to_string()))
            .db_name(Some(c.database.clone()).filter(|d| !d.is_empty()));
        let with_ssl = |b: OptsBuilder| match c.ssl {
            SslMode::Disable => b,
            SslMode::Prefer | SslMode::Require if !c.ssl_ca.trim().is_empty() => b.ssl_opts(Some(ca_opts(SslOpts::default().with_danger_skip_domain_validation(true), &c.ssl_ca))),
            SslMode::Prefer | SslMode::Require => b.ssl_opts(Some(
                SslOpts::default().with_danger_accept_invalid_certs(true).with_danger_skip_domain_validation(true),
            )),
            SslMode::VerifyFull => b.ssl_opts(Some(ca_opts(SslOpts::default(), &c.ssl_ca))),
        };
        let connect = |b: OptsBuilder| async move {
            tokio::time::timeout(Duration::from_secs(10), Conn::new(b))
                .await
                .map_err(|_| Error::Db("connection timed out".into()))?
                .map_err(Error::from)
        };
        let conn = match c.ssl {
            SslMode::Prefer => match connect(with_ssl(base.clone())).await {
                Ok(c) => c,
                Err(_) => connect(base).await?,
            },
            _ => connect(with_ssl(base)).await?,
        };
        let mut conn = conn;
        if c.read_only {
            conn.query_drop("SET SESSION TRANSACTION READ ONLY").await?;
        }
        Ok(Self { opts: conn.opts().clone(), conn, is_mariadb: false, scope: Some(c.database.clone()).filter(|d| !d.is_empty()) })
    }

    pub fn canceller(&self) -> crate::driver::Canceller {
        crate::driver::Canceller::My { opts: self.opts.clone(), id: self.conn.id() }
    }

    pub async fn set_timeout(&mut self, ms: u64) -> Result<()> {
        if self.is_mariadb {
            self.conn.query_drop(format!("SET SESSION max_statement_time = {}", ms as f64 / 1000.0)).await?;
        } else {
            // Applies to SELECT statements (MySQL has no general statement timeout).
            self.conn.query_drop(format!("SET SESSION max_execution_time = {ms}")).await?;
        }
        Ok(())
    }

    pub async fn version(&mut self) -> Result<String> {
        let v: Option<String> = self.conn.query_first("SELECT VERSION()").await?;
        let v = v.unwrap_or_default();
        self.is_mariadb = v.to_lowercase().contains("mariadb");
        Ok(v)
    }

    /// `col` restricted to the selected database, or to every user database.
    fn scoped(&self, col: &str) -> String {
        match &self.scope {
            Some(d) => format!("{col} = {}", sql::literal(D, d)),
            None => format!("{col} NOT IN ('mysql','information_schema','performance_schema','sys')"),
        }
    }

    pub async fn metadata(&mut self) -> Result<Metadata> {
        let tables: Vec<Row> = self
            .conn
            .query(format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE, TABLE_ROWS, COALESCE(DATA_LENGTH,0)+COALESCE(INDEX_LENGTH,0) \
                 FROM information_schema.TABLES WHERE {} ORDER BY 1, 2",
                self.scoped("TABLE_SCHEMA")
            ))
            .await?;
        let cols: Vec<Row> = self
            .conn
            .query(format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT \
                 FROM information_schema.COLUMNS WHERE {} ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION",
                self.scoped("TABLE_SCHEMA")
            ))
            .await?;
        let fks: Vec<Row> = self
            .conn
            .query(format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
                 FROM information_schema.KEY_COLUMN_USAGE WHERE REFERENCED_TABLE_NAME IS NOT NULL AND {}",
                self.scoped("TABLE_SCHEMA")
            ))
            .await?;
        let idx: Vec<Row> = self
            .conn
            .query(format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, INDEX_NAME, NON_UNIQUE, GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX) \
                 FROM information_schema.STATISTICS WHERE {} GROUP BY 1, 2, 3, 4",
                self.scoped("TABLE_SCHEMA")
            ))
            .await?;
        let routines: Vec<Row> = self
            .conn
            .query(format!(
                "SELECT ROUTINE_SCHEMA, ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES WHERE {} ORDER BY 1, 2",
                self.scoped("ROUTINE_SCHEMA")
            ))
            .await?;
        let triggers: Vec<Row> = self
            .conn
            .query(format!(
                "SELECT TRIGGER_SCHEMA, TRIGGER_NAME, EVENT_OBJECT_TABLE FROM information_schema.TRIGGERS WHERE {} ORDER BY 1, 2",
                self.scoped("TRIGGER_SCHEMA")
            ))
            .await?;
        // MariaDB and MySQL both have EVENTS, but the scheduler may be unavailable to this user.
        let events: Vec<Row> = self
            .conn
            .query(format!("SELECT EVENT_SCHEMA, EVENT_NAME FROM information_schema.EVENTS WHERE {} ORDER BY 1, 2", self.scoped("EVENT_SCHEMA")))
            .await
            .unwrap_or_default();

        let s = |r: &Row, i: usize| r.as_ref(i).and_then(value_to_cell).unwrap_or_default();
        let mut out: Vec<Table> = tables
            .iter()
            .map(|r| Table {
                schema: s(r, 0),
                name: s(r, 1),
                kind: if s(r, 2).contains("VIEW") { TableKind::View } else { TableKind::Table },
                columns: Vec::new(),
                estimated_rows: s(r, 3).parse().ok(),
                size_bytes: s(r, 4).parse().ok(),
                indexes: Vec::new(),
                keyless_edit: false,
            })
            .collect();
        let pos: std::collections::HashMap<(String, String), usize> =
            out.iter().enumerate().map(|(i, t)| ((t.schema.clone(), t.name.clone()), i)).collect();
        let fk_map: std::collections::HashMap<(String, String, String), String> = fks
            .iter()
            .map(|r| ((s(r, 0), s(r, 1), s(r, 2)), format!("{}.{}({})", s(r, 3), s(r, 4), s(r, 5))))
            .collect();
        for r in &cols {
            let (sc, t, name) = (s(r, 0), s(r, 1), s(r, 2));
            if let Some(&i) = pos.get(&(sc.clone(), t.clone())) {
                let fk = fk_map.get(&(sc, t, name.clone())).cloned();
                out[i].columns.push(Column {
                    name,
                    type_name: s(r, 3),
                    nullable: s(r, 4) == "YES",
                    is_primary_key: s(r, 5) == "PRI",
                    default: r.as_ref(6).and_then(value_to_cell),
                    fk,
                });
            }
        }
        for r in &idx {
            if let Some(&i) = pos.get(&(s(r, 0), s(r, 1))) {
                let unique = if s(r, 3) == "0" { "UNIQUE " } else { "" };
                out[i].indexes.push(format!("{unique}INDEX {} ({})", s(r, 2), s(r, 4)));
            }
        }
        let mut objects: Vec<DbObject> = routines
            .iter()
            .map(|r| DbObject {
                schema: s(r, 0),
                name: s(r, 1),
                kind: if s(r, 2) == "PROCEDURE" { ObjectKind::Procedure } else { ObjectKind::Function },
                detail: String::new(),
            })
            .collect();
        objects.extend(triggers.iter().map(|r| DbObject { schema: s(r, 0), name: s(r, 1), kind: ObjectKind::Trigger, detail: s(r, 2) }));
        objects.extend(events.iter().map(|r| DbObject { schema: s(r, 0), name: s(r, 1), kind: ObjectKind::Event, detail: String::new() }));
        // Indexes: the detail is the owning table; the definition is rebuilt on demand.
        for t in &out {
            for ix in &t.indexes {
                let name = ix.split_whitespace().find(|w| *w != "UNIQUE" && *w != "INDEX").unwrap_or_default().to_string();
                objects.push(DbObject { schema: t.schema.clone(), name, kind: ObjectKind::Index, detail: t.name.clone() });
            }
        }
        Ok(Metadata { tables: out, objects })
    }

    pub async fn fetch(&mut self, t: &Table, p: &Page) -> Result<Rows> {
        let started = Instant::now();
        let rows: Vec<Row> = self.conn.query(sql::select_page(D, t, p)).await?;
        Ok(Rows {
            columns: t.columns.iter().map(|c| c.name.clone()).collect(),
            rows: rows_to_cells(&rows, t.columns.len()),
            duration_ms: started.elapsed().as_secs_f64() * 1000.0,
            total_estimate: t.estimated_rows,
        })
    }

    pub async fn query(&mut self, sql_text: &str) -> Result<Rows> {
        let started = Instant::now();
        let mut res = self.conn.query_iter(sql_text).await?;
        let columns: Vec<String> =
            res.columns().map(|c| c.iter().map(|c| c.name_str().to_string()).collect()).unwrap_or_default();
        let rows: Vec<Row> = res.collect().await?;
        let affected = res.affected_rows();
        drop(res);
        let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
        if columns.is_empty() {
            return Ok(Rows { columns: vec!["rows affected".into()], rows: vec![vec![Some(affected.to_string())]], duration_ms, total_estimate: None });
        }
        let w = columns.len();
        Ok(Rows { columns, rows: rows_to_cells(&rows, w), duration_ms, total_estimate: None })
    }

    async fn exec(&mut self, stmt: &str, params: Vec<Value>) -> Result<u64> {
        self.conn.exec_drop(stmt, Params::Positional(params)).await?;
        Ok(self.conn.affected_rows())
    }

    pub async fn update(&mut self, t: &Table, column: &str, key: &[Cell], new: Option<&str>) -> Result<()> {
        let stmt = sql::update_cell(D, t, column, new.is_none()).ok_or_else(|| Error::Db(format!("unknown column {column}")))?;
        let mut params: Vec<Value> = Vec::new();
        if let Some(v) = new {
            params.push(Value::from(v));
        }
        params.extend(key.iter().map(|k| k.as_deref().map_or(Value::NULL, Value::from)));
        // MySQL reports 0 affected rows when the value is unchanged, so 0 is not an error here.
        self.exec(&stmt, params).await?;
        Ok(())
    }

    pub async fn insert(&mut self, t: &Table, vals: &[(String, String)]) -> Result<()> {
        let cols: Vec<&str> = vals.iter().map(|v| v.0.as_str()).collect();
        let stmt = sql::insert_row(D, t, &cols).ok_or_else(|| Error::Db("unknown column".into()))?;
        self.exec(&stmt, vals.iter().map(|v| Value::from(v.1.as_str())).collect()).await?;
        Ok(())
    }

    /// Insert a full row, with explicit NULLs (used to restore a deleted row).
    pub async fn insert_nullable(&mut self, t: &Table, vals: &[(String, Cell)]) -> Result<()> {
        let cols: Vec<(&str, bool)> = vals.iter().map(|v| (v.0.as_str(), v.1.is_none())).collect();
        let stmt = sql::insert_row_with_nulls(D, t, &cols).ok_or_else(|| Error::Db("unknown column".into()))?;
        self.exec(&stmt, vals.iter().filter_map(|v| v.1.as_deref().map(Value::from)).collect()).await?;
        Ok(())
    }

    pub async fn delete(&mut self, t: &Table, key: &[Cell]) -> Result<()> {
        let stmt = sql::delete_row(D, t).ok_or_else(|| Error::Unsafe("This table has no primary key.".into()))?;
        if self.exec(&stmt, key.iter().map(|k| k.as_deref().map_or(Value::NULL, Value::from)).collect()).await? == 0 {
            return Err(Error::Db("No row matched; it may have been changed or deleted.".into()));
        }
        Ok(())
    }

    pub async fn truncate(&mut self, t: &Table) -> Result<()> {
        self.conn.query_drop(sql::truncate(D, t)).await?;
        Ok(())
    }

    pub async fn drop_table(&mut self, t: &Table) -> Result<()> {
        self.conn.query_drop(sql::drop_table(D, t)).await?;
        Ok(())
    }

    pub async fn ddl(&mut self, t: &Table) -> Result<String> {
        let what = if t.kind == TableKind::View { "VIEW" } else { "TABLE" };
        let rows: Vec<Row> = self.conn.query(format!("SHOW CREATE {what} {}", D.qualified(t))).await?;
        Ok(rows.first().and_then(|r| r.as_ref(1)).and_then(value_to_cell).unwrap_or_default())
    }

    pub async fn explain(&mut self, sql_text: &str, analyze: bool) -> Result<Rows> {
        let body = sql_text.trim().trim_end_matches(';');
        let stmt = match (analyze, self.is_mariadb) {
            (false, _) => format!("EXPLAIN {body}"),
            (true, false) => format!("EXPLAIN ANALYZE {body}"),
            (true, true) => format!("ANALYZE {body}"),
        };
        self.query(&stmt).await
    }

    pub async fn object_def(&mut self, o: &DbObject) -> Result<String> {
        let q = |s: &str| D.quote(s);
        let (kind, col) = match o.kind {
            ObjectKind::Procedure => ("PROCEDURE", 2),
            ObjectKind::Function => ("FUNCTION", 2),
            ObjectKind::Trigger => ("TRIGGER", 2),
            ObjectKind::Event => ("EVENT", 3),
            ObjectKind::Index => {
                let rows: Vec<Row> = self
                    .conn
                    .query(format!(
                        "SELECT NON_UNIQUE, GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX) FROM information_schema.STATISTICS \
                         WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} AND INDEX_NAME = {} GROUP BY NON_UNIQUE",
                        sql::literal(D, &o.schema), sql::literal(D, &o.detail), sql::literal(D, &o.name)
                    ))
                    .await?;
                let Some(r) = rows.first() else { return Ok(String::new()) };
                let unique = r.as_ref(0).and_then(value_to_cell).as_deref() == Some("0");
                let cols = r.as_ref(1).and_then(value_to_cell).unwrap_or_default();
                return Ok(if o.name == "PRIMARY" {
                    format!("ALTER TABLE {}.{} ADD PRIMARY KEY ({cols});", q(&o.schema), q(&o.detail))
                } else {
                    format!("CREATE {}INDEX {} ON {}.{} ({cols});", if unique { "UNIQUE " } else { "" }, q(&o.name), q(&o.schema), q(&o.detail))
                });
            }
            ObjectKind::Sequence | ObjectKind::Type | ObjectKind::Extension => return Ok(String::new()),
        };
        let rows: Vec<Row> = self.conn.query(format!("SHOW CREATE {kind} {}.{}", q(&o.schema), q(&o.name))).await?;
        Ok(rows.first().and_then(|r| r.as_ref(col)).and_then(value_to_cell).map(|d| strip_definer(&d)).unwrap_or_default())
    }

    // ---- databases, users -------------------------------------------------------------------

    pub async fn list_databases(&mut self) -> Result<Vec<String>> {
        let rows: Vec<String> = self.conn.query("SHOW DATABASES").await?;
        Ok(rows.into_iter().filter(|d| !matches!(d.as_str(), "mysql" | "information_schema" | "performance_schema" | "sys")).collect())
    }

    /// Narrow (or widen, with `None`) the databases this connection lists, and `USE` it.
    pub async fn use_database(&mut self, db: Option<&str>) -> Result<()> {
        if let Some(d) = db {
            self.conn.query_drop(format!("USE {}", D.quote(d))).await?;
        }
        self.scope = db.map(str::to_string);
        Ok(())
    }

    pub async fn list_users(&mut self) -> Result<Vec<UserInfo>> {
        let rows: Vec<Row> = match self.conn.query("SELECT User, Host, Super_priv FROM mysql.user ORDER BY User, Host").await {
            Ok(r) => r,
            Err(_) => self.conn.query("SELECT User, Host, 'N' FROM mysql.user ORDER BY User, Host").await.map_err(|_| {
                Error::Db("This account is not allowed to read the user list (mysql.user). Connect as an administrator.".into())
            })?,
        };
        let s = |r: &Row, i: usize| r.as_ref(i).and_then(value_to_cell).unwrap_or_default();
        Ok(rows
            .iter()
            .map(|r| UserInfo { name: s(r, 0), origin: s(r, 1), summary: if s(r, 2) == "Y" { "superuser".into() } else { String::new() } })
            .collect())
    }

    pub async fn user_grants(&mut self, u: &UserInfo) -> Result<Vec<String>> {
        let rows: Vec<Row> = self.conn.query(format!("SHOW GRANTS FOR {}", admin::mysql_account(&u.name, &u.origin))).await?;
        Ok(rows.iter().filter_map(|r| r.as_ref(0).and_then(value_to_cell)).collect())
    }

    pub async fn set_access(&mut self, u: &UserInfo, level: AccessLevel) -> Result<()> {
        let (revoke, grant) = admin::my_set_access(self.scope.as_deref(), &u.name, &u.origin, level);
        let _ = self.conn.query_drop(revoke).await; // fails when nothing was granted
        if let Some(g) = grant {
            self.conn.query_drop(g).await?;
        }
        Ok(())
    }

    pub async fn create_user(&mut self, n: &NewUser) -> Result<()> {
        self.conn.query_drop(admin::my_create_user(n)).await?;
        let acct = UserInfo { name: n.name.clone(), origin: if n.host.trim().is_empty() { "%".into() } else { n.host.trim().into() }, summary: String::new() };
        let r = if n.admin { self.conn.query_drop(admin::my_grant_admin(&acct.name, &acct.origin)).await.map_err(Error::from) } else { self.set_access(&acct, n.access).await };
        r.map_err(|e| Error::Db(format!("The user was created, but access could not be set: {e}")))
    }

    pub async fn set_password(&mut self, u: &UserInfo, password: &str) -> Result<()> {
        self.conn.query_drop(admin::my_set_password(&u.name, &u.origin, password)).await?;
        Ok(())
    }

    pub async fn drop_user(&mut self, u: &UserInfo) -> Result<()> {
        self.conn.query_drop(admin::my_drop_user(&u.name, &u.origin)).await?;
        Ok(())
    }

    // ---- export / import --------------------------------------------------------------------

    pub async fn dump(&mut self, opts: &DumpOptions, tables: &[Table], objects: &[DbObject], out: &mut dyn Write, progress: &mut dyn FnMut(Progress)) -> Result<DumpStats> {
        let mut st = DumpStats::default();
        let ver = self.version().await.unwrap_or_default();
        write!(
            out,
            "-- dboard dump\n-- MySQL/MariaDB {ver}\n\n/*!40101 SET NAMES utf8mb4 */;\nSET FOREIGN_KEY_CHECKS = 0;\nSET UNIQUE_CHECKS = 0;\nSET SQL_MODE = 'NO_AUTO_VALUE_ON_ZERO';\n"
        )?;
        let mut schemas: Vec<String> = tables.iter().map(|t| t.schema.clone()).chain(objects.iter().map(|o| o.schema.clone())).collect();
        schemas.sort();
        schemas.dedup();
        let multi = schemas.len() > 1;
        let total_tables = tables.iter().filter(|t| t.kind == TableKind::Table).count();
        let mut done_tables = 0usize;
        for sch in &schemas {
            // A single-database dump must restore into any database name, so drop the source
            // database qualifier (`db`.) from definitions; multi-database dumps USE each one.
            let strip = |sql: String| if multi { sql } else { sql.replace(&format!("{}.", D.quote(sch)), "") };
            let unq = |t: &Table| D.quote(&t.name);
            if multi {
                writeln!(out, "\nCREATE DATABASE IF NOT EXISTS {};\nUSE {};", D.quote(sch), D.quote(sch))?;
            }
            let in_schema: Vec<&Table> = tables.iter().filter(|t| &t.schema == sch).collect();
            let base: Vec<&&Table> = in_schema.iter().filter(|t| t.kind == TableKind::Table).collect();
            if opts.schema {
                for t in &base {
                    let rows: Vec<Row> = self.conn.query(format!("SHOW CREATE TABLE {}", D.qualified(t))).await?;
                    let ddl = rows.first().and_then(|r| r.as_ref(1)).and_then(value_to_cell).unwrap_or_default();
                    writeln!(out, "\n{};", strip(ddl))?;
                }
            }
            st.tables += base.len();
            if opts.data {
                for t in &base {
                    if t.columns.is_empty() {
                        continue;
                    }
                    let names: Vec<String> = t.columns.iter().map(|c| c.name.clone()).collect();
                    let select = names.iter().map(|n| D.quote(n)).collect::<Vec<_>>().join(", ");
                    let numeric: Vec<bool> = t.columns.iter().map(|c| is_numeric(&c.type_name)).collect();
                    let binary: Vec<bool> = t.columns.iter().map(|c| is_binary(&c.type_name)).collect();
                    let mut w = InsertWriter::new(D, &unq(t), &names);
                    writeln!(out, "\n-- Data for {}", unq(t))?;
                    {
                        let mut res = self.conn.query_iter(format!("SELECT {select} FROM {}", D.qualified(t))).await?;
                        while let Some(row) = res.next().await? {
                            let vals: Vec<String> = (0..names.len())
                                .map(|i| match row.as_ref(i) {
                                    Some(Value::Bytes(b)) if binary[i] => hex_literal(b),
                                    Some(v) => sql_value(D, value_to_cell(v).as_deref(), numeric[i]),
                                    None => "NULL".into(),
                                })
                                .collect();
                            w.push(&vals, out)?;
                        }
                    }
                    w.flush(out)?;
                    st.rows += w.total;
                    done_tables += 1;
                    progress(Progress::at(format!("Exported {} ({} rows)", D.qualified(t), w.total), done_tables, total_tables));
                }
            }
            if opts.schema {
                for t in in_schema.iter().filter(|t| t.kind == TableKind::View) {
                    let rows: Vec<Row> = self.conn.query(format!("SHOW CREATE VIEW {}", D.qualified(t))).await?;
                    let ddl = rows.first().and_then(|r| r.as_ref(1)).and_then(value_to_cell).unwrap_or_default();
                    writeln!(out, "\n{};", strip(strip_definer(&ddl)))?;
                    st.objects += 1;
                }
                for o in objects.iter().filter(|o| &o.schema == sch && matches!(o.kind, ObjectKind::Function | ObjectKind::Procedure | ObjectKind::Trigger | ObjectKind::Event)) {
                    let def = self.object_def(o).await?;
                    if def.is_empty() {
                        continue;
                    }
                    writeln!(out, "\nDELIMITER ;;\n{};;\nDELIMITER ;", strip(def))?;
                    st.objects += 1;
                }
            }
        }
        writeln!(out, "\nSET FOREIGN_KEY_CHECKS = 1;\nSET UNIQUE_CHECKS = 1;")?;
        out.flush()?;
        Ok(st)
    }

    pub async fn import(&mut self, reader: &mut dyn BufRead, opts: &ImportOptions, progress: &mut dyn FnMut(Progress)) -> Result<ImportStats> {
        let mut stats = ImportStats::default();
        let mut sp = Splitter::new(D);
        let mut pending: Vec<Stmt> = Vec::new();
        // Views may be listed before the views they read from; retry those at the end.
        let mut deferred: Vec<String> = Vec::new();
        let mut raw = Vec::new();
        let mut last_report = Instant::now();
        let mut eof = false;
        while !eof {
            raw.clear();
            if reader.read_until(b'\n', &mut raw)? == 0 {
                eof = true;
                sp.finish(&mut pending);
            } else {
                sp.feed_line(&String::from_utf8_lossy(&raw), &mut pending);
            }
            for stmt in pending.drain(..) {
                let Stmt::Sql(sql) = stmt else { continue };
                stats.statements += 1;
                if let Err(e) = self.conn.query_drop(sql.as_str()).await {
                    let is_view = sql.to_uppercase().contains(" VIEW ");
                    let missing = matches!(&e, mysql_async::Error::Server(s) if s.code == 1146 || s.code == 1356);
                    if is_view && missing {
                        deferred.push(sql);
                        continue;
                    }
                    let snippet: String = sql.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect();
                    let msg = format!("Statement {} failed: {}\n  {snippet}", stats.statements, Error::from(e));
                    if opts.stop_on_error {
                        return Err(Error::Db(format!("{msg}\nStatements before this one were already applied (MySQL cannot roll back schema changes).")));
                    }
                    if stats.errors.len() < 50 {
                        stats.errors.push(msg);
                    }
                }
                if last_report.elapsed().as_millis() > 400 {
                    progress(format!("{} statements run…", stats.statements).into());
                    last_report = Instant::now();
                }
            }
        }
        for _ in 0..4 {
            if deferred.is_empty() {
                break;
            }
            let before = deferred.len();
            let mut still = Vec::new();
            let mut last_err = String::new();
            for sql in deferred.drain(..) {
                if let Err(e) = self.conn.query_drop(sql.as_str()).await {
                    last_err = Error::from(e).to_string();
                    still.push(sql);
                }
            }
            deferred = still;
            if deferred.len() == before {
                for sql in &deferred {
                    let msg = format!("View could not be created: {last_err}\n  {}", sql.chars().take(120).collect::<String>());
                    if opts.stop_on_error {
                        return Err(Error::Db(msg));
                    }
                    stats.errors.push(msg);
                }
                break;
            }
        }
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_values() {
        assert_eq!(value_to_cell(&Value::NULL), None);
        assert_eq!(value_to_cell(&Value::Int(-4)).as_deref(), Some("-4"));
        assert_eq!(value_to_cell(&Value::Bytes(b"hi".to_vec())).as_deref(), Some("hi"));
        assert_eq!(value_to_cell(&Value::Date(2024, 1, 2, 0, 0, 0, 0)).as_deref(), Some("2024-01-02"));
        assert_eq!(value_to_cell(&Value::Date(2024, 1, 2, 3, 4, 5, 0)).as_deref(), Some("2024-01-02 03:04:05"));
        assert_eq!(value_to_cell(&Value::Time(true, 1, 2, 3, 4, 0)).as_deref(), Some("-26:03:04"));
    }

    #[test]
    fn humanizes_errors() {
        assert!(humanize(1062, "x").contains("unique"));
        assert_eq!(humanize(9999, "raw"), "raw");
    }
}

/// Trust the user's CA bundle (PEM path) when one is configured.
fn ca_opts(o: SslOpts, ca: &str) -> SslOpts {
    if ca.trim().is_empty() {
        o
    } else {
        o.with_root_certs(vec![std::path::PathBuf::from(ca.trim()).into()])
    }
}
