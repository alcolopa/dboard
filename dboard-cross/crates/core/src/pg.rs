use crate::admin;
use crate::dump::{is_numeric, sql_value, DumpOptions, DumpStats, ImportOptions, ImportStats, InsertWriter};
use crate::model::*;
use crate::split::{Splitter, Stmt};
use crate::sql::{self, literal, quote_ident, Dialect};
use crate::{tls, Error, Result};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::time::Instant;
use tokio_postgres::{types::ToSql, Client, Config, NoTls};

const D: Dialect = Dialect::Pg;

pub struct Pg {
    client: Client,
}

impl Pg {
    pub async fn connect(c: &ConnectionConfig, password: &str) -> Result<Self> {
        let mut cfg = Config::new();
        cfg.host(&c.host)
            .port(c.port)
            .user(&c.username)
            .password(password)
            .connect_timeout(std::time::Duration::from_secs(10));
        if !c.database.is_empty() {
            cfg.dbname(&c.database);
        }
        let client = match c.ssl {
            SslMode::Disable => Self::plain(&cfg).await?,
            SslMode::Prefer => match Self::secure(&cfg, c).await {
                Ok(cl) => cl,
                Err(_) => Self::plain(&cfg).await?,
            },
            _ => Self::secure(&cfg, c).await?,
        };
        Ok(Self { client })
    }

    async fn plain(cfg: &Config) -> Result<Client> {
        let (client, conn) = cfg.connect(NoTls).await?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        Ok(client)
    }

    async fn secure(cfg: &Config, c: &ConnectionConfig) -> Result<Client> {
        let mut cfg = cfg.clone();
        cfg.ssl_mode(tokio_postgres::config::SslMode::Require);
        let connector = tokio_postgres_rustls::MakeRustlsConnect::new(tls::client_config(c)?);
        let (client, conn) = cfg.connect(connector).await?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        Ok(client)
    }

    pub async fn version(&mut self) -> Result<String> {
        Ok(self.client.query_one("SHOW server_version", &[]).await?.get(0))
    }

    pub async fn metadata(&mut self) -> Result<Metadata> {
        const SYS: &str = "n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' AND n.nspname NOT LIKE 'pg_temp%'";
        let tables = self
            .client
            .query(
                format!(
                    "SELECT n.nspname, c.relname, c.relkind::text, c.reltuples::bigint, pg_total_relation_size(c.oid) \
                     FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE c.relkind IN ('r','p','v','m') AND {SYS} ORDER BY n.nspname, c.relname"
                )
                .as_str(),
                &[],
            )
            .await?;
        let cols = self
            .client
            .query(
                format!(
                    "SELECT n.nspname, c.relname, a.attname, format_type(a.atttypid, NULL), NOT a.attnotnull, \
                            coalesce(a.attnum = ANY (i.indkey), false), pg_get_expr(d.adbin, d.adrelid) \
                     FROM pg_attribute a \
                     JOIN pg_class c ON c.oid = a.attrelid \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     LEFT JOIN pg_index i ON i.indrelid = c.oid AND i.indisprimary \
                     LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                     WHERE a.attnum > 0 AND NOT a.attisdropped AND c.relkind IN ('r','p','v','m') AND {SYS} \
                     ORDER BY n.nspname, c.relname, a.attnum"
                )
                .as_str(),
                &[],
            )
            .await?;
        let fks = self
            .client
            .query(
                "SELECT n.nspname, c.relname, a.attname, fn.nspname || '.' || fc.relname || '(' || fa.attname || ')' \
                 FROM pg_constraint k \
                 JOIN pg_class c ON c.oid = k.conrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
                 JOIN pg_class fc ON fc.oid = k.confrelid JOIN pg_namespace fn ON fn.oid = fc.relnamespace \
                 CROSS JOIN LATERAL unnest(k.conkey, k.confkey) AS u(ck, fk) \
                 JOIN pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = u.ck \
                 JOIN pg_attribute fa ON fa.attrelid = k.confrelid AND fa.attnum = u.fk \
                 WHERE k.contype = 'f'",
                &[],
            )
            .await?;
        let idx = self
            .client
            .query("SELECT schemaname, tablename, indexdef FROM pg_indexes WHERE schemaname NOT IN ('pg_catalog','information_schema')", &[])
            .await?;
        let objs = self
            .client
            .query(
                format!(
                    "SELECT n.nspname, p.proname, p.prokind::text, pg_get_function_identity_arguments(p.oid) \
                     FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                     WHERE p.prokind IN ('f','p') AND {SYS} \
                       AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = p.oid AND d.deptype = 'e') \
                     UNION ALL \
                     SELECT n.nspname, c.relname, 'S', '' FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE c.relkind = 'S' AND {SYS} \
                       AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = c.oid AND d.deptype = 'i') \
                     UNION ALL \
                     SELECT n.nspname, t.tgname, 'T', c.relname FROM pg_trigger t \
                     JOIN pg_class c ON c.oid = t.tgrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE NOT t.tgisinternal AND {SYS} \
                     UNION ALL \
                     SELECT n.nspname, t.typname, 'Y', t.typtype::text FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace \
                     WHERE t.typtype IN ('e','d') AND {SYS} \
                     UNION ALL \
                     SELECT schemaname, indexname, 'I', tablename FROM pg_indexes \
                     WHERE schemaname NOT IN ('pg_catalog','information_schema') AND schemaname NOT LIKE 'pg_toast%' \
                     UNION ALL \
                     SELECT n.nspname, e.extname, 'X', e.extversion FROM pg_extension e JOIN pg_namespace n ON n.oid = e.extnamespace \
                     WHERE {SYS} \
                     ORDER BY 1, 2"
                )
                .as_str(),
                &[],
            )
            .await?;

        let mut out: Vec<Table> = tables
            .iter()
            .map(|r| Table {
                schema: r.get(0),
                name: r.get(1),
                kind: match r.get::<_, String>(2).as_str() {
                    "v" => TableKind::View,
                    "m" => TableKind::MaterializedView,
                    _ => TableKind::Table,
                },
                columns: Vec::new(),
                estimated_rows: Some(r.get::<_, i64>(3)).filter(|n| *n >= 0),
                size_bytes: Some(r.get(4)),
                indexes: Vec::new(),
                keyless_edit: false,
            })
            .collect();
        let pos: HashMap<(String, String), usize> =
            out.iter().enumerate().map(|(i, t)| ((t.schema.clone(), t.name.clone()), i)).collect();
        let fk_map: HashMap<(String, String, String), String> =
            fks.iter().map(|r| ((r.get(0), r.get(1), r.get(2)), r.get(3))).collect();
        for r in &cols {
            let (s, t, name): (String, String, String) = (r.get(0), r.get(1), r.get(2));
            if let Some(&i) = pos.get(&(s.clone(), t.clone())) {
                let fk = fk_map.get(&(s, t, name.clone())).cloned();
                out[i].columns.push(Column {
                    name,
                    // format_type output is valid in a cast (e.g. `character varying(20)`, `integer[]`).
                    type_name: r.get(3),
                    nullable: r.get(4),
                    is_primary_key: r.get(5),
                    default: r.get(6),
                    fk,
                });
            }
        }
        for r in &idx {
            let (s, t): (String, String) = (r.get(0), r.get(1));
            if let Some(&i) = pos.get(&(s, t)) {
                out[i].indexes.push(r.get(2));
            }
        }
        // Without a primary key a row is still addressable through its physical row id.
        for t in out.iter_mut() {
            t.keyless_edit = t.kind == TableKind::Table && !t.has_primary_key() && !t.columns.is_empty();
        }
        let objects = objs
            .iter()
            .map(|r| DbObject {
                schema: r.get(0),
                name: r.get(1),
                kind: match r.get::<_, String>(2).as_str() {
                    "p" => ObjectKind::Procedure,
                    "S" => ObjectKind::Sequence,
                    "T" => ObjectKind::Trigger,
                    "Y" => ObjectKind::Type,
                    "I" => ObjectKind::Index,
                    "X" => ObjectKind::Extension,
                    _ => ObjectKind::Function,
                },
                detail: r.get(3),
            })
            .collect();
        Ok(Metadata { tables: out, objects })
    }

    pub async fn fetch(&mut self, t: &Table, p: &Page) -> Result<Rows> {
        let started = Instant::now();
        let rows = self.client.query(sql::select_page(D, t, p).as_str(), &[]).await?;
        let data = rows.iter().map(|r| (0..t.columns.len()).map(|i| r.get::<_, Option<String>>(i)).collect()).collect();
        let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
        let est = self
            .client
            .query_opt(sql::count_estimate_pg(&t.schema, &t.name).as_str(), &[])
            .await
            .ok()
            .flatten()
            .map(|r| r.get::<_, i64>(0));
        Ok(Rows { columns: t.columns.iter().map(|c| c.name.clone()).collect(), rows: data, duration_ms, total_estimate: est })
    }

    /// Run arbitrary SQL via the simple-query protocol: every value comes back as text, and
    /// `SHOW`, `EXPLAIN`, `RETURNING` and multi-statement input all work. Returns the last
    /// result set, or the affected-row count if none returned rows.
    pub async fn query(&mut self, sql_text: &str) -> Result<Rows> {
        use tokio_postgres::SimpleQueryMessage as M;
        let started = Instant::now();
        let msgs = self.client.simple_query(sql_text).await?;
        let mut columns: Vec<String> = Vec::new();
        let mut rows: Vec<Vec<Cell>> = Vec::new();
        let mut affected: Option<u64> = None;
        for m in msgs {
            match m {
                M::RowDescription(d) => {
                    columns = d.iter().map(|c| c.name().to_string()).collect();
                    rows.clear();
                }
                M::Row(r) => {
                    if columns.is_empty() {
                        columns = r.columns().iter().map(|c| c.name().to_string()).collect();
                    }
                    rows.push((0..r.len()).map(|i| r.get(i).map(str::to_string)).collect());
                }
                M::CommandComplete(n) => affected = Some(n),
                _ => {}
            }
        }
        let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
        if columns.is_empty() {
            return Ok(Rows {
                columns: vec!["rows affected".into()],
                rows: vec![vec![Some(affected.unwrap_or(0).to_string())]],
                duration_ms,
                total_estimate: None,
            });
        }
        Ok(Rows { columns, rows, duration_ms, total_estimate: None })
    }

    async fn run(&self, stmt: &str, params: &[Cell], first: Option<&str>) -> Result<u64> {
        let mut p: Vec<&(dyn ToSql + Sync)> = Vec::with_capacity(params.len() + 1);
        if let Some(v) = first.as_ref() {
            p.push(v);
        }
        for k in params {
            p.push(k);
        }
        Ok(self.client.execute(stmt, &p).await?)
    }

    pub async fn update(&mut self, t: &Table, column: &str, key: &[Cell], new: Option<&str>) -> Result<()> {
        let stmt = sql::update_cell(D, t, column, new.is_none()).ok_or_else(|| Error::Db(format!("unknown column {column}")))?;
        if self.run(&stmt, key, new).await? == 0 {
            return Err(Error::Db("No row matched; it may have been changed or deleted.".into()));
        }
        Ok(())
    }

    pub async fn insert(&mut self, t: &Table, vals: &[(String, String)]) -> Result<()> {
        let cols: Vec<&str> = vals.iter().map(|v| v.0.as_str()).collect();
        let stmt = sql::insert_row(D, t, &cols).ok_or_else(|| Error::Db("unknown column".into()))?;
        let params: Vec<Cell> = vals.iter().map(|v| Some(v.1.clone())).collect();
        self.run(&stmt, &params, None).await?;
        Ok(())
    }

    /// Insert a full row, with explicit NULLs (used to restore a deleted row).
    pub async fn insert_nullable(&mut self, t: &Table, vals: &[(String, Cell)]) -> Result<()> {
        let cols: Vec<(&str, bool)> = vals.iter().map(|v| (v.0.as_str(), v.1.is_none())).collect();
        let stmt = sql::insert_row_with_nulls(D, t, &cols).ok_or_else(|| Error::Db("unknown column".into()))?;
        let params: Vec<Cell> = vals.iter().filter(|v| v.1.is_some()).map(|v| v.1.clone()).collect();
        self.run(&stmt, &params, None).await?;
        Ok(())
    }

    pub async fn delete(&mut self, t: &Table, key: &[Cell]) -> Result<()> {
        let stmt = sql::delete_row(D, t).ok_or_else(|| Error::Unsafe("This table has no primary key.".into()))?;
        if self.run(&stmt, key, None).await? == 0 {
            return Err(Error::Db("No row matched; it may have been changed or deleted.".into()));
        }
        Ok(())
    }

    pub async fn truncate(&mut self, t: &Table) -> Result<()> {
        self.client.batch_execute(&sql::truncate(D, t)).await?;
        Ok(())
    }

    pub async fn drop_table(&mut self, t: &Table) -> Result<()> {
        self.client.batch_execute(&sql::drop_table(D, t)).await?;
        Ok(())
    }

    pub async fn ddl(&mut self, t: &Table) -> Result<String> {
        let mut out = sql::pg_ddl(t);
        for i in &t.indexes {
            if !i.contains("UNIQUE") || !t.has_primary_key() {
                out.push_str(&format!("\n{i};"));
            }
        }
        Ok(out)
    }

    pub async fn explain(&mut self, sql_text: &str, analyze: bool) -> Result<Rows> {
        let prefix = if analyze { "EXPLAIN (ANALYZE, FORMAT JSON) " } else { "EXPLAIN (FORMAT JSON) " };
        let started = Instant::now();
        let r = self.query(&format!("{prefix}{}", sql_text.trim().trim_end_matches(';'))).await?;
        let json = r.rows.first().and_then(|row| row.first()).cloned().flatten().unwrap_or_default();
        let mut out = explain_rows_from_json(&json).map_err(Error::Db)?;
        out.duration_ms = started.elapsed().as_secs_f64() * 1000.0;
        Ok(out)
    }

    pub async fn object_def(&mut self, o: &DbObject) -> Result<String> {
        let (sch, name, det) = (literal(D, &o.schema), literal(D, &o.name), literal(D, &o.detail));
        let first = |r: Rows| r.rows.first().and_then(|x| x.first()).cloned().flatten().unwrap_or_default();
        match o.kind {
            ObjectKind::Sequence => {
                let r = self
                    .query(&format!(
                        "SELECT start_value, minimum_value, maximum_value, increment FROM information_schema.sequences \
                         WHERE sequence_schema = {sch} AND sequence_name = {name}"
                    ))
                    .await?;
                let row = r.rows.first().cloned().unwrap_or_default();
                let g = |i: usize| row.get(i).cloned().flatten().unwrap_or_default();
                Ok(format!(
                    "CREATE SEQUENCE {}.{}\n  START {}\n  MINVALUE {}\n  MAXVALUE {}\n  INCREMENT {};",
                    quote_ident(&o.schema), quote_ident(&o.name), g(0), g(1), g(2), g(3)
                ))
            }
            ObjectKind::Function | ObjectKind::Procedure => Ok(first(
                self.query(&format!(
                    "SELECT pg_get_functiondef(p.oid) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                     WHERE n.nspname = {sch} AND p.proname = {name} AND pg_get_function_identity_arguments(p.oid) = {det} LIMIT 1"
                ))
                .await?,
            )),
            ObjectKind::Trigger => Ok(first(
                self.query(&format!(
                    "SELECT pg_get_triggerdef(t.oid, true) || ';' FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = {sch} AND t.tgname = {name} AND c.relname = {det} AND NOT t.tgisinternal LIMIT 1"
                ))
                .await?,
            )),
            ObjectKind::Index => Ok(first(
                self.query(&format!("SELECT indexdef || ';' FROM pg_indexes WHERE schemaname = {sch} AND indexname = {name} LIMIT 1")).await?,
            )),
            ObjectKind::Type => {
                let q = if o.detail == "d" {
                    format!(
                        "SELECT 'CREATE DOMAIN ' || quote_ident(n.nspname) || '.' || quote_ident(t.typname) || ' AS ' || format_type(t.typbasetype, t.typtypmod) \
                         || coalesce(' DEFAULT ' || t.typdefault, '') || CASE WHEN t.typnotnull THEN ' NOT NULL' ELSE '' END || ';' \
                         FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace WHERE n.nspname = {sch} AND t.typname = {name}"
                    )
                } else {
                    format!(
                        "SELECT 'CREATE TYPE ' || quote_ident(n.nspname) || '.' || quote_ident(t.typname) || ' AS ENUM (' \
                         || coalesce((SELECT string_agg(quote_literal(e.enumlabel), ', ' ORDER BY e.enumsortorder) FROM pg_enum e WHERE e.enumtypid = t.oid), '') || ');' \
                         FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace WHERE n.nspname = {sch} AND t.typname = {name}"
                    )
                };
                Ok(first(self.query(&q).await?))
            }
            ObjectKind::Extension => Ok(format!(
                "CREATE EXTENSION IF NOT EXISTS {} WITH SCHEMA {} VERSION {};",
                quote_ident(&o.name),
                quote_ident(&o.schema),
                literal(D, &o.detail)
            )),
            ObjectKind::Event => Ok(String::new()),
        }
    }

    // ---- databases, users -------------------------------------------------------------------

    pub async fn list_databases(&mut self) -> Result<Vec<String>> {
        let rows = self.client.query("SELECT datname FROM pg_database WHERE datallowconn AND NOT datistemplate ORDER BY datname", &[]).await?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    pub async fn current_database(&mut self) -> Result<String> {
        Ok(self.client.query_one("SELECT current_database()", &[]).await?.get(0))
    }

    pub async fn list_users(&mut self) -> Result<Vec<UserInfo>> {
        let rows = self
            .client
            .query(
                "SELECT rolname, rolcanlogin, rolsuper, rolcreatedb, rolcreaterole, rolreplication, coalesce(rolvaliduntil::text, '') \
                 FROM pg_roles WHERE rolname NOT LIKE 'pg\\_%' ORDER BY rolcanlogin DESC, rolname",
                &[],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let mut tags: Vec<String> = vec![if r.get::<_, bool>(1) { "can log in".into() } else { "group role".into() }];
                for (i, label) in [(2, "superuser"), (3, "can create databases"), (4, "can create roles"), (5, "replication")] {
                    if r.get::<_, bool>(i) {
                        tags.push(label.into());
                    }
                }
                let until: String = r.get(6);
                if !until.is_empty() {
                    tags.push(format!("expires {until}"));
                }
                UserInfo { name: r.get(0), origin: String::new(), summary: tags.join(" · ") }
            })
            .collect())
    }

    pub async fn user_grants(&mut self, u: &UserInfo) -> Result<Vec<String>> {
        let name = &u.name;
        let mut out = Vec::new();
        let member = self
            .client
            .query(
                "SELECT r.rolname FROM pg_auth_members m JOIN pg_roles r ON r.oid = m.roleid JOIN pg_roles u ON u.oid = m.member WHERE u.rolname = $1 ORDER BY 1",
                &[name],
            )
            .await?;
        if !member.is_empty() {
            out.push(format!("Member of: {}", member.iter().map(|r| r.get::<_, String>(0)).collect::<Vec<_>>().join(", ")));
        }
        let db = self
            .client
            .query_one("SELECT current_database(), has_database_privilege($1, current_database(), 'CONNECT'), has_database_privilege($1, current_database(), 'CREATE')", &[name])
            .await?;
        out.push(format!(
            "Database {}: {}{}",
            db.get::<_, String>(0),
            if db.get::<_, bool>(1) { "connect" } else { "no connect" },
            if db.get::<_, bool>(2) { ", create schemas" } else { "" }
        ));
        let rows = self
            .client
            .query(
                "SELECT n.nspname || '.' || c.relname, string_agg(a.privilege_type, ', ' ORDER BY a.privilege_type) \
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 CROSS JOIN LATERAL aclexplode(c.relacl) a JOIN pg_roles r ON r.oid = a.grantee \
                 WHERE r.rolname = $1 AND c.relkind IN ('r','p','v','m','S') AND n.nspname NOT IN ('pg_catalog','information_schema') \
                 GROUP BY 1 ORDER BY 1",
                &[name],
            )
            .await?;
        for r in &rows {
            out.push(format!("{}: {}", r.get::<_, String>(0), r.get::<_, String>(1)));
        }
        if rows.is_empty() {
            out.push("No table or sequence privileges granted directly.".into());
        }
        Ok(out)
    }

    async fn user_schemas(&mut self) -> Result<Vec<String>> {
        let rows = self
            .client
            .query("SELECT nspname FROM pg_namespace WHERE nspname NOT IN ('pg_catalog','information_schema') AND nspname NOT LIKE 'pg_toast%' AND nspname NOT LIKE 'pg_temp%' ORDER BY 1", &[])
            .await?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    pub async fn set_access(&mut self, user: &str, level: AccessLevel) -> Result<()> {
        let db = self.current_database().await?;
        let schemas = self.user_schemas().await?;
        self.client.batch_execute(&admin::pg_set_access(&db, user, level, &schemas).join(";\n")).await?;
        Ok(())
    }

    pub async fn create_user(&mut self, n: &NewUser) -> Result<()> {
        self.client.batch_execute(&admin::pg_create_user(n)).await?;
        if !n.admin {
            if let Err(e) = self.set_access(&n.name, n.access).await {
                return Err(Error::Db(format!("The user was created, but access could not be set: {e}")));
            }
        }
        Ok(())
    }

    pub async fn set_password(&mut self, u: &UserInfo, password: &str) -> Result<()> {
        self.client.batch_execute(&admin::pg_set_password(&u.name, password)).await?;
        Ok(())
    }

    pub async fn drop_user(&mut self, u: &UserInfo) -> Result<()> {
        // Privileges in this database would block DROP ROLE, so remove them first.
        let _ = self.set_access(&u.name, AccessLevel::None).await;
        self.client.batch_execute(&format!("DROP ROLE {}", quote_ident(&u.name))).await.map_err(|e| {
            let e: Error = e.into();
            Error::Db(format!("{e}\nIf this role owns objects or has rights in other databases, reassign or drop those first."))
        })
    }

    // ---- export / import --------------------------------------------------------------------

    pub async fn dump(&mut self, opts: &DumpOptions, out: &mut dyn Write, progress: &mut dyn FnMut(String)) -> Result<DumpStats> {
        const SYS: &str = "n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' AND n.nspname NOT LIKE 'pg_temp%'";
        let mut st = DumpStats::default();
        let db = self.current_database().await?;
        let ver = self.version().await.unwrap_or_default();
        write!(
            out,
            "-- dboard dump\n-- PostgreSQL {ver} - database {db}\n\nSET client_encoding = 'UTF8';\nSET standard_conforming_strings = on;\nSET check_function_bodies = false;\n\n"
        )?;
        let mut post: Vec<String> = Vec::new(); // run after all data: setvals, refreshes

        if opts.schema {
            progress("Writing schemas, types and sequences…".into());
            for r in self.client.query(&format!("SELECT nspname FROM pg_namespace n WHERE {SYS} AND nspname <> 'public' ORDER BY 1"), &[]).await? {
                writeln!(out, "CREATE SCHEMA IF NOT EXISTS {};", quote_ident(&r.get::<_, String>(0)))?;
            }
            for r in self
                .client
                .query("SELECT e.extname, n.nspname FROM pg_extension e JOIN pg_namespace n ON n.oid = e.extnamespace WHERE e.extname <> 'plpgsql' ORDER BY 1", &[])
                .await?
            {
                let (ext, sch): (String, String) = (r.get(0), r.get(1));
                if sch == "pg_catalog" {
                    writeln!(out, "CREATE EXTENSION IF NOT EXISTS {};", quote_ident(&ext))?;
                } else {
                    writeln!(out, "CREATE EXTENSION IF NOT EXISTS {} WITH SCHEMA {};", quote_ident(&ext), quote_ident(&sch))?;
                }
                st.objects += 1;
            }
            for r in self
                .client
                .query(
                    &format!(
                        "SELECT n.nspname, t.typname, t.typtype::text FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace \
                         WHERE t.typtype IN ('e','d') AND {SYS} ORDER BY t.oid"
                    ),
                    &[],
                )
                .await?
            {
                let o = DbObject { schema: r.get(0), name: r.get(1), kind: ObjectKind::Type, detail: r.get(2) };
                writeln!(out, "{}", self.object_def(&o).await?)?;
                st.objects += 1;
            }
            let seqs = self
                .client
                .query(
                    "SELECT s.schemaname, s.sequencename, s.data_type::text, s.start_value::text, s.min_value::text, s.max_value::text, \
                            s.increment_by::text, s.cycle, s.cache_size::text, s.last_value::text \
                     FROM pg_sequences s JOIN pg_class c ON c.relname = s.sequencename \
                       AND c.relnamespace = (SELECT oid FROM pg_namespace WHERE nspname = s.schemaname) \
                     WHERE s.schemaname NOT IN ('pg_catalog','information_schema') \
                       AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = c.oid AND d.deptype = 'i') \
                     ORDER BY 1, 2",
                    &[],
                )
                .await?;
            for r in &seqs {
                let q = format!("{}.{}", quote_ident(&r.get::<_, String>(0)), quote_ident(&r.get::<_, String>(1)));
                writeln!(
                    out,
                    "CREATE SEQUENCE {q} AS {} INCREMENT BY {} MINVALUE {} MAXVALUE {} START WITH {} CACHE {}{};",
                    r.get::<_, String>(2), r.get::<_, String>(6), r.get::<_, String>(4), r.get::<_, String>(5), r.get::<_, String>(3), r.get::<_, String>(8),
                    if r.get::<_, bool>(7) { " CYCLE" } else { "" }
                )?;
                if let Some(last) = r.get::<_, Option<String>>(9) {
                    post.push(format!("SELECT pg_catalog.setval({}, {last}, true);", literal(D, &q)));
                }
                st.objects += 1;
            }
        }

        // Tables and views in creation order (a good approximation of dependency order).
        let rels = self
            .client
            .query(
                &format!(
                    "SELECT n.nspname, c.relname, c.relkind::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE c.relkind IN ('r','p','v','m') AND NOT c.relispartition AND {SYS} ORDER BY c.oid"
                ),
                &[],
            )
            .await?;
        let tables: Vec<(String, String)> = rels.iter().filter(|r| matches!(r.get::<_, String>(2).as_str(), "r" | "p")).map(|r| (r.get(0), r.get(1))).collect();

        #[derive(Default, Clone)]
        struct ColDef {
            name: String,
            ty: String,
            not_null: bool,
            identity: bool,
            generated: bool,
            default: Option<String>,
        }
        let mut cols: HashMap<(String, String), Vec<ColDef>> = HashMap::new();
        for r in self
            .client
            .query(
                &format!(
                    "SELECT n.nspname, c.relname, a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull, a.attidentity::text, a.attgenerated::text, \
                            pg_get_expr(d.adbin, d.adrelid) \
                     FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
                     LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                     WHERE a.attnum > 0 AND NOT a.attisdropped AND c.relkind IN ('r','p') AND {SYS} ORDER BY n.nspname, c.relname, a.attnum"
                ),
                &[],
            )
            .await?
        {
            cols.entry((r.get(0), r.get(1))).or_default().push(ColDef {
                name: r.get(2),
                ty: r.get(3),
                not_null: r.get(4),
                identity: !r.get::<_, String>(5).is_empty(),
                generated: !r.get::<_, String>(6).is_empty(),
                default: r.get(7),
            });
        }
        // (constraint name, pg_constraint.contype, definition) per table
        type Constraint = (String, String, String);
        let mut constraints: HashMap<(String, String), Vec<Constraint>> = HashMap::new();
        for r in self
            .client
            .query(
                &format!(
                    "SELECT n.nspname, c.relname, k.conname, k.contype::text, pg_get_constraintdef(k.oid, true) \
                     FROM pg_constraint k JOIN pg_class c ON c.oid = k.conrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE c.relkind IN ('r','p') AND k.contype IN ('p','u','c','x','f') AND {SYS} ORDER BY n.nspname, c.relname, k.contype <> 'p', k.conname"
                ),
                &[],
            )
            .await?
        {
            constraints.entry((r.get(0), r.get(1))).or_default().push((r.get(2), r.get(3), r.get(4)));
        }

        if opts.schema {
            for (sch, name) in &tables {
                let key = (sch.clone(), name.clone());
                let mut lines: Vec<String> = Vec::new();
                for c in cols.get(&key).map(Vec::as_slice).unwrap_or_default() {
                    let mut l = format!("    {} {}", quote_ident(&c.name), c.ty);
                    if c.identity {
                        l.push_str(" GENERATED BY DEFAULT AS IDENTITY");
                    } else if c.generated {
                        l.push_str(&format!(" GENERATED ALWAYS AS ({}) STORED", c.default.clone().unwrap_or_default()));
                    } else if let Some(d) = &c.default {
                        l.push_str(&format!(" DEFAULT {d}"));
                    }
                    if c.not_null && !c.identity {
                        l.push_str(" NOT NULL");
                    }
                    lines.push(l);
                }
                for (cname, ctype, def) in constraints.get(&key).map(Vec::as_slice).unwrap_or_default() {
                    if ctype != "f" {
                        lines.push(format!("    CONSTRAINT {} {def}", quote_ident(cname)));
                    }
                }
                writeln!(out, "\nCREATE TABLE {}.{} (\n{}\n);", quote_ident(sch), quote_ident(name), lines.join(",\n"))?;
            }
        }
        st.tables = tables.len();

        if opts.data {
            for (sch, name) in &tables {
                let key = (sch.clone(), name.clone());
                let defs: Vec<ColDef> = cols.get(&key).cloned().unwrap_or_default().into_iter().filter(|c| !c.generated).collect();
                if defs.is_empty() {
                    continue;
                }
                let q = format!("{}.{}", quote_ident(sch), quote_ident(name));
                let select = defs.iter().map(|c| format!("{}::text", quote_ident(&c.name))).collect::<Vec<_>>().join(", ");
                let numeric: Vec<bool> = defs.iter().map(|c| is_numeric(&c.ty)).collect();
                let names: Vec<String> = defs.iter().map(|c| c.name.clone()).collect();
                let mut w = InsertWriter::new(D, &q, &names);
                self.client.batch_execute(&format!("BEGIN; DECLARE dboard_dump NO SCROLL CURSOR FOR SELECT {select} FROM {q}")).await?;
                writeln!(out, "\n-- Data for {q}")?;
                let res: Result<()> = async {
                    loop {
                        let msgs = self.client.simple_query("FETCH FORWARD 500 FROM dboard_dump").await?;
                        let mut got = 0;
                        for m in msgs {
                            if let tokio_postgres::SimpleQueryMessage::Row(r) = m {
                                got += 1;
                                let vals: Vec<String> = (0..defs.len()).map(|i| sql_value(D, r.get(i), numeric[i])).collect();
                                w.push(&vals, out)?;
                            }
                        }
                        if got == 0 {
                            break;
                        }
                    }
                    Ok(())
                }
                .await;
                let _ = self.client.batch_execute("CLOSE dboard_dump; COMMIT").await;
                res?;
                w.flush(out)?;
                st.rows += w.total;
                progress(format!("Exported {q} ({} rows)", w.total));
                if defs.iter().any(|c| c.identity) {
                    for c in defs.iter().filter(|c| c.identity) {
                        let (qc, qs) = (quote_ident(&c.name), literal(D, &q));
                        post.push(format!(
                            "SELECT pg_catalog.setval(pg_get_serial_sequence({qs}, {}), coalesce((SELECT max({qc}) FROM {q}), 1), (SELECT max({qc}) IS NOT NULL FROM {q}));",
                            literal(D, &c.name)
                        ));
                    }
                }
            }
        }

        if opts.schema {
            progress("Writing functions, views, indexes and constraints…".into());
            for r in self
                .client
                .query(
                    &format!(
                        "SELECT pg_get_functiondef(p.oid) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                         WHERE p.prokind IN ('f','p') AND {SYS} AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = p.oid AND d.deptype = 'e') ORDER BY p.oid"
                    ),
                    &[],
                )
                .await?
            {
                writeln!(out, "\n{};", r.get::<_, String>(0).trim_end().trim_end_matches(';'))?;
                st.objects += 1;
            }
            for r in &rels {
                let kind: String = r.get(2);
                if kind != "v" && kind != "m" {
                    continue;
                }
                let (sch, name): (String, String) = (r.get(0), r.get(1));
                let q = format!("{}.{}", quote_ident(&sch), quote_ident(&name));
                let def = self.client.query_one("SELECT pg_get_viewdef($1::text::regclass, true)", &[&q]).await?.get::<_, String>(0);
                let def = def.trim_end().trim_end_matches(';');
                if kind == "m" {
                    writeln!(out, "\nCREATE MATERIALIZED VIEW {q} AS\n{def}\nWITH NO DATA;")?;
                    post.push(format!("REFRESH MATERIALIZED VIEW {q};"));
                } else {
                    writeln!(out, "\nCREATE VIEW {q} AS\n{def};")?;
                }
                st.objects += 1;
            }
            for r in self
                .client
                .query(
                    &format!(
                        "SELECT pg_get_indexdef(i.indexrelid) FROM pg_index i JOIN pg_class tc ON tc.oid = i.indrelid JOIN pg_namespace n ON n.oid = tc.relnamespace \
                         WHERE NOT EXISTS (SELECT 1 FROM pg_constraint k WHERE k.conindid = i.indexrelid) AND tc.relkind IN ('r','p','m') AND NOT tc.relispartition AND {SYS} \
                         ORDER BY i.indexrelid"
                    ),
                    &[],
                )
                .await?
            {
                writeln!(out, "{};", r.get::<_, String>(0))?;
                st.objects += 1;
            }
            for r in self
                .client
                .query(
                    &format!(
                        "SELECT pg_get_triggerdef(t.oid, true) FROM pg_trigger t JOIN pg_class c ON c.oid = t.tgrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
                         WHERE NOT t.tgisinternal AND NOT c.relispartition AND {SYS} ORDER BY t.oid"
                    ),
                    &[],
                )
                .await?
            {
                writeln!(out, "{};", r.get::<_, String>(0))?;
                st.objects += 1;
            }
            for (sch, name) in &tables {
                for (cname, ctype, def) in constraints.get(&(sch.clone(), name.clone())).map(Vec::as_slice).unwrap_or_default() {
                    if ctype == "f" {
                        writeln!(out, "ALTER TABLE ONLY {}.{} ADD CONSTRAINT {} {def};", quote_ident(sch), quote_ident(name), quote_ident(cname))?;
                    }
                }
            }
        }
        if !post.is_empty() {
            writeln!(out)?;
            for l in post {
                writeln!(out, "{l}")?;
            }
        }
        out.flush()?;
        Ok(st)
    }

    pub async fn import(&mut self, reader: &mut dyn BufRead, opts: &ImportOptions, progress: &mut dyn FnMut(String)) -> Result<ImportStats> {
        use futures_util::SinkExt;
        let mut stats = ImportStats::default();
        let mut sp = Splitter::new(D);
        let mut pending: Vec<Stmt> = Vec::new();
        let mut raw = Vec::new();
        let mut in_txn = false;
        if opts.stop_on_error {
            self.client.batch_execute("BEGIN").await?;
            in_txn = true;
        }
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
                stats.statements += 1;
                let (label, res) = match &stmt {
                    Stmt::Sql(sql) => (sql.clone(), self.client.batch_execute(sql).await.map_err(Error::from)),
                    Stmt::Copy { head, data } => {
                        let r: Result<u64> = async {
                            let sink = self.client.copy_in::<_, bytes::Bytes>(head.as_str()).await?;
                            futures_util::pin_mut!(sink);
                            sink.as_mut().send(bytes::Bytes::from(data.clone())).await?;
                            Ok(sink.finish().await?)
                        }
                        .await;
                        if let Ok(n) = &r {
                            stats.rows_copied += n;
                        }
                        (head.clone(), r.map(|_| ()))
                    }
                };
                if let Err(e) = res {
                    let snippet: String = label.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect();
                    let msg = format!("Statement {} failed: {e}\n  {snippet}", stats.statements);
                    if opts.stop_on_error {
                        if in_txn {
                            let _ = self.client.batch_execute("ROLLBACK").await;
                        }
                        return Err(Error::Db(format!("{msg}\nNothing was imported (the whole import was rolled back).")));
                    }
                    if stats.errors.len() < 50 {
                        stats.errors.push(msg);
                    }
                }
                if last_report.elapsed().as_millis() > 400 {
                    progress(format!("{} statements run…", stats.statements));
                    last_report = Instant::now();
                }
            }
        }
        if in_txn {
            self.client.batch_execute("COMMIT").await?;
        }
        Ok(stats)
    }
}

/// Flatten `EXPLAIN (FORMAT JSON)` output into indented rows.
pub fn explain_rows_from_json(json: &str) -> std::result::Result<Rows, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("could not parse plan: {e}"))?;
    let plan = v.get(0).and_then(|p| p.get("Plan")).ok_or("no plan in EXPLAIN output")?;
    let mut rows = Vec::new();
    fn walk(n: &serde_json::Value, depth: usize, rows: &mut Vec<Vec<Cell>>) {
        let s = |k: &str| n.get(k).and_then(|x| x.as_str()).map(str::to_string);
        let f = |k: &str| n.get(k).and_then(|x| x.as_f64());
        let mut label = s("Node Type").unwrap_or_default();
        if let Some(r) = s("Relation Name") {
            label.push_str(&format!(" on {r}"));
        }
        if let Some(i) = s("Index Name") {
            label.push_str(&format!(" using {i}"));
        }
        let indent = if depth == 0 { String::new() } else { format!("{}↳ ", "   ".repeat(depth - 1)) };
        rows.push(vec![
            Some(format!("{indent}{label}")),
            f("Total Cost").map(|c| format!("{c:.2}")),
            f("Plan Rows").map(|c| format!("{c:.0}")),
            f("Actual Total Time").map(|c| format!("{c:.3}")),
            f("Actual Rows").map(|c| format!("{c:.0}")),
        ]);
        if let Some(children) = n.get("Plans").and_then(|p| p.as_array()) {
            for c in children {
                walk(c, depth + 1, rows);
            }
        }
    }
    walk(plan, 0, &mut rows);
    let mut extra = Vec::new();
    for (k, label) in [("Planning Time", "Planning ms"), ("Execution Time", "Execution ms")] {
        if let Some(t) = v.get(0).and_then(|p| p.get(k)).and_then(|x| x.as_f64()) {
            extra.push(vec![Some(label.to_string()), None, None, Some(format!("{t:.3}")), None]);
        }
    }
    rows.extend(extra);
    Ok(Rows {
        columns: vec!["Plan".into(), "Cost".into(), "Est. rows".into(), "Actual ms".into(), "Actual rows".into()],
        rows,
        duration_ms: 0.0,
        total_estimate: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_explain_plan() {
        let j = r#"[{"Plan":{"Node Type":"Hash Join","Total Cost":12.5,"Plan Rows":10,"Plans":[
            {"Node Type":"Seq Scan","Relation Name":"a","Total Cost":1.0,"Plan Rows":5},
            {"Node Type":"Index Scan","Index Name":"b_pkey","Relation Name":"b","Total Cost":2.0,"Plan Rows":5}]},
            "Execution Time": 1.25}]"#;
        let r = explain_rows_from_json(j).unwrap();
        assert_eq!(r.rows[0][0].as_deref(), Some("Hash Join"));
        assert_eq!(r.rows[1][0].as_deref(), Some("↳ Seq Scan on a"));
        assert_eq!(r.rows[2][0].as_deref(), Some("↳ Index Scan on b using b_pkey"));
        assert_eq!(r.rows.last().unwrap()[0].as_deref(), Some("Execution ms"));
        assert!(explain_rows_from_json("nope").is_err());
    }
}
