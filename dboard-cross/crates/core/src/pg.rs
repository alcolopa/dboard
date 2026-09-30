use crate::model::*;
use crate::sql::{self, Dialect};
use crate::{tls, Error, Result};
use std::collections::HashMap;
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
            SslMode::Prefer => match Self::secure(&cfg, c.ssl).await {
                Ok(cl) => cl,
                Err(_) => Self::plain(&cfg).await?,
            },
            _ => Self::secure(&cfg, c.ssl).await?,
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

    async fn secure(cfg: &Config, mode: SslMode) -> Result<Client> {
        let mut cfg = cfg.clone();
        cfg.ssl_mode(tokio_postgres::config::SslMode::Require);
        let connector = tokio_postgres_rustls::MakeRustlsConnect::new(tls::client_config(mode));
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
                    "SELECT n.nspname, p.proname, p.prokind::text FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                     WHERE p.prokind IN ('f','p') AND {SYS} \
                     UNION ALL \
                     SELECT n.nspname, c.relname, 'S' FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE c.relkind = 'S' AND {SYS} ORDER BY 1, 2"
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
        let objects = objs
            .iter()
            .map(|r| DbObject {
                schema: r.get(0),
                name: r.get(1),
                kind: match r.get::<_, String>(2).as_str() {
                    "p" => ObjectKind::Procedure,
                    "S" => ObjectKind::Sequence,
                    _ => ObjectKind::Function,
                },
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

    async fn run(&self, stmt: &str, params: &[String], first: Option<&str>) -> Result<u64> {
        let mut p: Vec<&(dyn ToSql + Sync)> = Vec::with_capacity(params.len() + 1);
        if let Some(v) = first.as_ref() {
            p.push(v);
        }
        for k in params {
            p.push(k);
        }
        Ok(self.client.execute(stmt, &p).await?)
    }

    pub async fn update(&mut self, t: &Table, column: &str, key: &[String], new: Option<&str>) -> Result<()> {
        let stmt = sql::update_cell(D, t, column, new.is_none()).ok_or_else(|| Error::Db(format!("unknown column {column}")))?;
        if self.run(&stmt, key, new).await? == 0 {
            return Err(Error::Db("No row matched; it may have been changed or deleted.".into()));
        }
        Ok(())
    }

    pub async fn insert(&mut self, t: &Table, vals: &[(String, String)]) -> Result<()> {
        let cols: Vec<&str> = vals.iter().map(|v| v.0.as_str()).collect();
        let stmt = sql::insert_row(D, t, &cols).ok_or_else(|| Error::Db("unknown column".into()))?;
        let params: Vec<String> = vals.iter().map(|v| v.1.clone()).collect();
        self.run(&stmt, &params, None).await?;
        Ok(())
    }

    pub async fn delete(&mut self, t: &Table, key: &[String]) -> Result<()> {
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
        if o.kind == ObjectKind::Sequence {
            let r = self
                .query(&format!(
                    "SELECT start_value, minimum_value, maximum_value, increment FROM information_schema.sequences \
                     WHERE sequence_schema = '{}' AND sequence_name = '{}'",
                    o.schema.replace('\'', "''"),
                    o.name.replace('\'', "''")
                ))
                .await?;
            let row = r.rows.first().cloned().unwrap_or_default();
            let g = |i: usize| row.get(i).cloned().flatten().unwrap_or_default();
            return Ok(format!(
                "CREATE SEQUENCE {}.{}\n  START {}\n  MINVALUE {}\n  MAXVALUE {}\n  INCREMENT {};",
                sql::quote_ident(&o.schema), sql::quote_ident(&o.name), g(0), g(1), g(2), g(3)
            ));
        }
        let r = self
            .query(&format!(
                "SELECT pg_get_functiondef(p.oid) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                 WHERE n.nspname = '{}' AND p.proname = '{}' LIMIT 1",
                o.schema.replace('\'', "''"),
                o.name.replace('\'', "''")
            ))
            .await?;
        Ok(r.rows.first().and_then(|x| x.first()).cloned().flatten().unwrap_or_default())
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
