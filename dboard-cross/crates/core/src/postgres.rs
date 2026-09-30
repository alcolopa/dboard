use crate::edit::{EditHistory, EditRecord};
use crate::model::*;
use crate::sql;
use crate::{Error, Result};
use std::time::Instant;
use tokio_postgres::{types::ToSql, Client, NoTls};

pub struct PgDriver {
    client: Client,
    pub config: ConnectionConfig,
    pub metadata: Metadata,
    pub history: EditHistory,
}

impl PgDriver {
    pub async fn connect(config: ConnectionConfig, password: &str) -> Result<Self> {
        let mut cfg = tokio_postgres::Config::new();
        cfg.host(&config.host)
            .port(config.port)
            .dbname(&config.database)
            .user(&config.username)
            .password(password)
            .connect_timeout(std::time::Duration::from_secs(10));
        let (client, conn) = cfg.connect(NoTls).await?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let mut d = Self { client, config, metadata: Metadata::default(), history: EditHistory::default() };
        d.refresh_metadata().await?;
        Ok(d)
    }

    pub async fn server_version(&self) -> Result<String> {
        Ok(self.client.query_one("SHOW server_version", &[]).await?.get(0))
    }

    pub async fn refresh_metadata(&mut self) -> Result<()> {
        // One round-trip for tables/views, one for columns (incl. PK flag).
        let tables = self
            .client
            .query(
                "SELECT n.nspname, c.relname, c.relkind::text \
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE c.relkind IN ('r','p','v','m') \
                   AND n.nspname NOT IN ('pg_catalog','information_schema') \
                   AND n.nspname NOT LIKE 'pg_toast%' \
                 ORDER BY n.nspname, c.relname",
                &[],
            )
            .await?;
        let cols = self
            .client
            .query(
                "SELECT n.nspname, c.relname, a.attname, \
                        format_type(a.atttypid, NULL), NOT a.attnotnull, \
                        coalesce(a.attnum = ANY (i.indkey), false) \
                 FROM pg_attribute a \
                 JOIN pg_class c ON c.oid = a.attrelid \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 LEFT JOIN pg_index i ON i.indrelid = c.oid AND i.indisprimary \
                 WHERE a.attnum > 0 AND NOT a.attisdropped \
                   AND c.relkind IN ('r','p','v','m') \
                   AND n.nspname NOT IN ('pg_catalog','information_schema') \
                   AND n.nspname NOT LIKE 'pg_toast%' \
                 ORDER BY n.nspname, c.relname, a.attnum",
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
            })
            .collect();
        for r in &cols {
            let (s, t): (String, String) = (r.get(0), r.get(1));
            if let Some(tab) = out.iter_mut().find(|x| x.schema == s && x.name == t) {
                tab.columns.push(Column {
                    name: r.get(2),
                    // format_type output is valid in a cast (e.g. `character varying(20)`, `integer[]`).
                    type_name: r.get(3),
                    nullable: r.get(4),
                    is_primary_key: r.get(5),
                });
            }
        }
        self.metadata = Metadata { tables: out };
        Ok(())
    }

    pub fn table(&self, schema: &str, name: &str) -> Option<&Table> {
        self.metadata.tables.iter().find(|t| t.schema == schema && t.name == name)
    }

    pub async fn fetch_page(&self, schema: &str, name: &str, page: &Page) -> Result<Rows> {
        let t = self.table(schema, name).ok_or(Error::Db(format!("unknown table {schema}.{name}")))?;
        let started = Instant::now();
        let rows = self.client.query(sql::select_page(t, page).as_str(), &[]).await?;
        let data = rows
            .iter()
            .map(|r| (0..t.columns.len()).map(|i| r.get::<_, Option<String>>(i)).collect())
            .collect();
        let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
        let est = self
            .client
            .query_opt(sql::count_estimate(schema, name).as_str(), &[])
            .await
            .ok()
            .flatten()
            .map(|r| r.get::<_, i64>(0));
        Ok(Rows {
            columns: t.columns.iter().map(|c| c.name.clone()).collect(),
            rows: data,
            duration_ms,
            total_estimate: est,
        })
    }

    /// Run arbitrary SQL via the simple-query protocol: every value comes back
    /// as text, and `SHOW`, `EXPLAIN`, `RETURNING` and multi-statement input all work.
    /// Returns the last result set, or the affected-row count if none returned rows.
    pub async fn execute_query(&self, sql_text: &str) -> Result<Rows> {
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

    /// Instant cell edit. `row` is the full row as displayed (in column order),
    /// used to read the primary key values.
    pub async fn edit_cell(
        &mut self,
        schema: &str,
        name: &str,
        row: &[Cell],
        column: &str,
        new: Cell,
    ) -> Result<()> {
        let (stmt, key, old) = {
            let t = self.table(schema, name).ok_or(Error::Db(format!("unknown table {schema}.{name}")))?;
            if !t.has_primary_key() {
                return Err(Error::Unsafe(
                    "This table has no primary key, so inline edits are disabled to protect your data.".into(),
                ));
            }
            let stmt = sql::update_cell(t, column, new.is_none())
                .ok_or(Error::Db(format!("unknown column {column}")))?;
            let idx_of = |n: &str| t.columns.iter().position(|c| c.name == n);
            let key: Vec<String> = t
                .primary_keys()
                .iter()
                .map(|pk| idx_of(&pk.name).and_then(|i| row.get(i).cloned().flatten()).unwrap_or_default())
                .collect();
            let old = idx_of(column).and_then(|i| row.get(i).cloned()).unwrap_or(None);
            (stmt, key, old)
        };
        self.run_update(&stmt, new.as_deref(), &key).await?;
        self.history.push(EditRecord {
            schema: schema.into(),
            table: name.into(),
            column: column.into(),
            key,
            old,
            new,
        });
        Ok(())
    }

    /// Revert the most recent edit by writing the old value back.
    pub async fn undo(&mut self) -> Result<Option<EditRecord>> {
        let Some(r) = self.history.pop() else { return Ok(None) };
        let stmt = {
            let t = self.table(&r.schema, &r.table).ok_or(Error::Db("table no longer exists".into()))?;
            sql::update_cell(t, &r.column, r.old.is_none()).ok_or(Error::Db("column no longer exists".into()))?
        };
        if let Err(e) = self.run_update(&stmt, r.old.as_deref(), &r.key).await {
            self.history.push(r); // keep it so the user can retry
            return Err(e);
        }
        Ok(Some(r))
    }

    async fn run_update(&self, stmt: &str, new: Option<&str>, key: &[String]) -> Result<()> {
        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::with_capacity(key.len() + 1);
        if let Some(v) = new.as_ref() {
            params.push(v);
        }
        for k in key {
            params.push(k);
        }
        let n = self.client.execute(stmt, &params).await?;
        if n == 0 {
            return Err(Error::Db("No row matched; it may have been changed or deleted.".into()));
        }
        Ok(())
    }
}
