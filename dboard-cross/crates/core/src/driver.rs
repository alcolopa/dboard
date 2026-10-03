//! One connection to any supported database, behind a single API.

use crate::dump::{DumpOptions, DumpStats, ImportOptions, ImportStats};
use crate::edit::{EditHistory, EditKind, EditRecord};
use crate::model::*;
use crate::mongo::Mongo;
use crate::mysql::My;
use crate::pg::Pg;
use crate::{Error, Result};
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

enum Inner {
    Pg(Pg),
    My(My),
    Mongo(Mongo),
}

macro_rules! dispatch {
    ($self:ident, $d:ident => $body:expr) => {
        match &mut $self.inner {
            Inner::Pg($d) => $body,
            Inner::My($d) => $body,
            Inner::Mongo($d) => $body,
        }
    };
}

/// Cancels the statement a connection is running. Cheap to clone and usable from any task.
pub enum Canceller {
    Pg { token: tokio_postgres::CancelToken, tls: Option<ConnectionConfig> },
    My { opts: mysql_async::Opts, id: u32 },
    Unsupported,
}

impl Canceller {
    /// Ask the server to stop the running statement. `false` when that is not possible.
    pub async fn cancel(&self) -> bool {
        match self {
            Canceller::Pg { token, tls } => match tls {
                Some(cfg) => match crate::tls::client_config(cfg) {
                    Ok(c) => token.cancel_query(tokio_postgres_rustls::MakeRustlsConnect::new(c)).await.is_ok(),
                    Err(_) => false,
                },
                None => token.cancel_query(tokio_postgres::NoTls).await.is_ok(),
            },
            Canceller::My { opts, id } => {
                use mysql_async::prelude::Queryable;
                match mysql_async::Conn::new(opts.clone()).await {
                    Ok(mut c) => c.query_drop(format!("KILL QUERY {id}")).await.is_ok(),
                    Err(_) => false,
                }
            }
            Canceller::Unsupported => false,
        }
    }
}

pub struct Conn {
    inner: Inner,
    tunnel: Option<crate::tunnel::Tunnel>,
    pub config: ConnectionConfig,
    pub metadata: Metadata,
    pub history: EditHistory,
    pub server_version: String,
    in_tx: bool,
    timeout_ms: u64,
}

impl Conn {
    pub async fn connect(config: ConnectionConfig, password: &str) -> Result<Self> {
        let tunnel = Self::open_tunnel(&config).await?;
        let effective = Self::through(&config, tunnel.as_ref());
        let inner = match config.db_type {
            DbType::Postgres => Inner::Pg(Pg::connect(&effective, password).await?),
            DbType::MySql => Inner::My(My::connect(&effective, password).await?),
            DbType::Mongo => Inner::Mongo(Mongo::connect(&effective, password).await?),
        };
        let mut c = Self { inner, tunnel, config, metadata: Metadata::default(), history: EditHistory::default(), server_version: String::new(), in_tx: false, timeout_ms: 0 };
        c.server_version = dispatch!(c, d => d.version().await).unwrap_or_default();
        c.refresh_metadata().await?;
        Ok(c)
    }

    async fn open_tunnel(config: &ConnectionConfig) -> Result<Option<crate::tunnel::Tunnel>> {
        if config.ssh_host.trim().is_empty() || (config.db_type == DbType::Mongo && !config.mongo_uri.trim().is_empty()) {
            return Ok(None);
        }
        crate::tunnel::open(crate::tunnel::TunnelSpec {
            ssh_host: config.ssh_host.trim(),
            ssh_port: if config.ssh_port == 0 { 22 } else { config.ssh_port },
            ssh_user: &config.ssh_user,
            key_path: &config.ssh_key,
            remote_host: &config.host,
            remote_port: config.port,
        })
        .await
        .map(Some)
    }

    /// The config to actually dial: the local end of the SSH tunnel when there is one.
    fn through(config: &ConnectionConfig, tunnel: Option<&crate::tunnel::Tunnel>) -> ConnectionConfig {
        let mut c = config.clone();
        if let Some(t) = tunnel {
            c.host = "127.0.0.1".into();
            c.port = t.local_port;
        }
        c
    }

    pub fn db_type(&self) -> DbType {
        self.config.db_type
    }

    pub async fn refresh_metadata(&mut self) -> Result<()> {
        self.metadata = dispatch!(self, d => d.metadata().await)?;
        Ok(())
    }

    pub fn table(&self, schema: &str, name: &str) -> Option<&Table> {
        self.metadata.tables.iter().find(|t| t.schema == schema && t.name == name)
    }

    fn table_owned(&self, schema: &str, name: &str) -> Result<Table> {
        self.table(schema, name).cloned().ok_or_else(|| Error::Db(format!("unknown table {schema}.{name}")))
    }

    pub async fn fetch_page(&mut self, schema: &str, name: &str, page: &Page) -> Result<Rows> {
        let t = self.table_owned(schema, name)?;
        if let Inner::Mongo(m) = &mut self.inner {
            let (rows, columns) = m.fetch(&t, page).await?;
            if let Some(tab) = self.metadata.tables.iter_mut().find(|x| x.schema == schema && x.name == name) {
                tab.columns = columns;
            }
            return Ok(rows);
        }
        match &mut self.inner {
            Inner::Pg(d) => d.fetch(&t, page).await,
            Inner::My(d) => d.fetch(&t, page).await,
            Inner::Mongo(_) => unreachable!(),
        }
    }

    pub async fn execute_query(&mut self, text: &str) -> Result<Rows> {
        dispatch!(self, d => d.query(text).await)
    }

    pub fn canceller(&self) -> Canceller {
        match &self.inner {
            Inner::Pg(d) => d.canceller(),
            Inner::My(d) => d.canceller(),
            Inner::Mongo(_) => Canceller::Unsupported,
        }
    }

    /// Server-side limit for a single statement, 0 = none (MongoDB: not applied).
    pub async fn set_statement_timeout(&mut self, ms: u64) -> Result<()> {
        self.timeout_ms = ms;
        match &mut self.inner {
            Inner::Pg(d) => d.set_timeout(ms).await,
            Inner::My(d) => d.set_timeout(ms).await,
            Inner::Mongo(_) => Ok(()),
        }
    }

    pub fn in_transaction(&self) -> bool {
        self.in_tx
    }

    /// Start a transaction: every statement and grid edit on this connection is held until
    /// `commit` or `rollback`. SQL engines only.
    pub async fn begin(&mut self) -> Result<()> {
        if self.config.db_type == DbType::Mongo {
            return Err(Error::Db("Transactions are not available for MongoDB here.".into()));
        }
        if self.in_tx {
            return Ok(());
        }
        dispatch!(self, d => d.query("BEGIN").await)?;
        self.in_tx = true;
        Ok(())
    }

    pub async fn commit(&mut self) -> Result<()> {
        self.end_tx("COMMIT").await
    }

    pub async fn rollback(&mut self) -> Result<()> {
        self.end_tx("ROLLBACK").await
    }

    async fn end_tx(&mut self, stmt: &str) -> Result<()> {
        if !self.in_tx {
            return Ok(());
        }
        let res = dispatch!(self, d => d.query(stmt).await);
        if res.is_ok() {
            self.in_tx = false;
            self.history.clear();
            let _ = self.refresh_metadata().await;
        }
        res.map(|_| ())
    }

    pub async fn explain(&mut self, text: &str, analyze: bool) -> Result<Rows> {
        dispatch!(self, d => d.explain(text, analyze).await)
    }

    pub async fn ddl(&mut self, schema: &str, name: &str) -> Result<String> {
        let t = self.table_owned(schema, name)?;
        dispatch!(self, d => d.ddl(&t).await)
    }

    pub async fn object_def(&mut self, o: &DbObject) -> Result<String> {
        match &mut self.inner {
            Inner::Pg(d) => d.object_def(o).await,
            Inner::My(d) => d.object_def(o).await,
            Inner::Mongo(_) => Ok(String::new()),
        }
    }

    /// Values that address a displayed row: its primary key, or (tables without one) every column.
    fn key_for(t: &Table, row: &[Cell]) -> Vec<Cell> {
        let pks = t.primary_keys();
        if pks.is_empty() {
            return t.columns.iter().enumerate().map(|(i, _)| row.get(i).cloned().flatten()).collect();
        }
        pks.iter().map(|pk| t.columns.iter().position(|c| c.name == pk.name).and_then(|i| row.get(i).cloned().flatten())).collect()
    }

    /// Instant cell edit. `row` is the full row as displayed, used to read the key values.
    pub async fn edit_cell(&mut self, schema: &str, name: &str, row: &[Cell], column: &str, new: Cell) -> Result<()> {
        let t = self.table_owned(schema, name)?;
        if !t.is_editable() {
            return Err(Error::Unsafe(
                "This object has no primary key (or is a view), so inline edits are disabled to protect your data.".into(),
            ));
        }
        let key = Self::key_for(&t, row);
        let col_idx = t.columns.iter().position(|c| c.name == column);
        let old = col_idx.and_then(|i| row.get(i).cloned()).unwrap_or(None);
        dispatch!(self, d => d.update(&t, column, &key, new.as_deref()).await)?;
        // The key of the row as it is now (the edited column may be part of it).
        let mut after = row.to_vec();
        if let (Some(i), Some(slot)) = (col_idx, col_idx.and_then(|i| after.get_mut(i))) {
            let _ = i;
            *slot = new.clone();
        }
        let key = Self::key_for(&t, &after);
        self.history.push(EditRecord {
            schema: schema.into(),
            table: name.into(),
            key,
            kind: EditKind::Update { column: column.into(), old, new },
            at: crate::config::now_secs(),
        });
        Ok(())
    }

    /// Revert the most recent change.
    pub async fn undo(&mut self) -> Result<Option<EditRecord>> {
        match self.history.len() {
            0 => Ok(None),
            n => self.undo_at(n - 1).await.map(Some),
        }
    }

    /// Revert one entry (0 = oldest). Refused when a newer edit touched the same cell.
    pub async fn undo_at(&mut self, index: usize) -> Result<EditRecord> {
        if self.history.blocked_by_newer(index) {
            return Err(Error::Unsafe("A newer change to the same cell exists. Undo that one first.".into()));
        }
        let r = self.history.remove(index).ok_or_else(|| Error::Db("That change is no longer in the history.".into()))?;
        let res = self.apply_inverse(&r).await;
        if let Err(e) = res {
            self.history.insert(index, r); // keep it so the user can retry
            return Err(e);
        }
        Ok(r)
    }

    async fn apply_inverse(&mut self, r: &EditRecord) -> Result<()> {
        let t = self.table_owned(&r.schema, &r.table)?;
        match &r.kind {
            EditKind::Update { column, old, .. } => dispatch!(self, d => d.update(&t, column, &r.key, old.as_deref()).await),
            EditKind::Delete { row, doc } => match &mut self.inner {
                Inner::Mongo(m) => match doc {
                    Some(json) => m.insert_json(&t, json).await,
                    None => Err(Error::Db("The deleted document was not kept, so it cannot be restored.".into())),
                },
                Inner::Pg(d) => d.insert_nullable(&t, &Self::named_values(&t, row)).await,
                Inner::My(d) => d.insert_nullable(&t, &Self::named_values(&t, row)).await,
            },
        }
    }

    fn named_values(t: &Table, row: &[Cell]) -> Vec<(String, Cell)> {
        t.columns.iter().zip(row.iter()).map(|(c, v)| (c.name.clone(), v.clone())).collect()
    }

    pub async fn insert_row(&mut self, schema: &str, name: &str, values: &[(String, String)]) -> Result<()> {
        let t = self.table_owned(schema, name)?;
        if t.kind != TableKind::Table && t.kind != TableKind::Collection {
            return Err(Error::Unsafe("Rows can only be inserted into tables.".into()));
        }
        dispatch!(self, d => d.insert(&t, values).await)
    }

    pub async fn delete_row(&mut self, schema: &str, name: &str, row: &[Cell]) -> Result<()> {
        let t = self.table_owned(schema, name)?;
        if !t.is_editable() {
            return Err(Error::Unsafe("Rows can only be deleted from tables with a primary key.".into()));
        }
        let key = Self::key_for(&t, row);
        // MongoDB: keep the whole document (nested fields are not in the grid) for undo.
        let doc = match &mut self.inner {
            Inner::Mongo(m) => m.get_document_json(&t, &key).await.ok(),
            _ => None,
        };
        dispatch!(self, d => d.delete(&t, &key).await)?;
        self.history.push(EditRecord {
            schema: schema.into(),
            table: name.into(),
            key,
            kind: EditKind::Delete { row: row.to_vec(), doc },
            at: crate::config::now_secs(),
        });
        Ok(())
    }

    pub async fn truncate(&mut self, schema: &str, name: &str) -> Result<()> {
        let t = self.table_owned(schema, name)?;
        dispatch!(self, d => d.truncate(&t).await)
    }

    pub async fn drop_table(&mut self, schema: &str, name: &str) -> Result<()> {
        let t = self.table_owned(schema, name)?;
        dispatch!(self, d => d.drop_table(&t).await)?;
        self.refresh_metadata().await
    }

    // ---- MongoDB extras ---------------------------------------------------------------

    pub async fn document_json(&mut self, schema: &str, name: &str, row: &[Cell]) -> Result<String> {
        let t = self.table_owned(schema, name)?;
        let key = Self::key_for(&t, row);
        match &mut self.inner {
            Inner::Mongo(m) => m.get_document_json(&t, &key).await,
            _ => Err(Error::Db("Document editing is only available for MongoDB.".into())),
        }
    }

    pub async fn replace_document(&mut self, schema: &str, name: &str, row: &[Cell], json: &str) -> Result<()> {
        let t = self.table_owned(schema, name)?;
        let key = Self::key_for(&t, row);
        match &mut self.inner {
            Inner::Mongo(m) => m.replace_document(&t, &key, json).await,
            _ => Err(Error::Db("Document editing is only available for MongoDB.".into())),
        }
    }

    pub async fn insert_document(&mut self, schema: &str, name: &str, json: &str) -> Result<()> {
        let t = self.table_owned(schema, name)?;
        match &mut self.inner {
            Inner::Mongo(m) => m.insert_json(&t, json).await,
            _ => Err(Error::Db("Document insert is only available for MongoDB.".into())),
        }
    }
}

impl Conn {
    // ---- databases --------------------------------------------------------------------------

    pub async fn list_databases(&mut self) -> Result<Vec<String>> {
        dispatch!(self, d => d.list_databases().await)
    }

    /// The database commands run against, if one is selected.
    pub async fn current_database(&mut self) -> Result<Option<String>> {
        match &mut self.inner {
            Inner::Pg(d) => d.current_database().await.map(Some),
            Inner::My(_) => Ok(Some(self.config.database.clone()).filter(|d| !d.is_empty())),
            Inner::Mongo(m) => Ok(m.current_database()),
        }
    }

    /// Point this connection at another database on the same server (`None`: all databases,
    /// MySQL / MongoDB only). PostgreSQL needs a fresh connection, hence the password.
    pub async fn switch_database(&mut self, db: Option<&str>, password: &str) -> Result<()> {
        if self.in_tx {
            return Err(Error::Db("Commit or roll back the open transaction before switching database.".into()));
        }
        match &mut self.inner {
            Inner::Pg(_) => {
                let name = db.ok_or_else(|| Error::Db("Pick a database.".into()))?;
                let mut cfg = Self::through(&self.config, self.tunnel.as_ref());
                cfg.database = name.to_string();
                self.inner = Inner::Pg(Pg::connect(&cfg, password).await?);
                if self.timeout_ms > 0 {
                    let ms = self.timeout_ms;
                    self.set_statement_timeout(ms).await?;
                }
            }
            Inner::My(d) => d.use_database(db).await?,
            Inner::Mongo(m) => m.use_database(db),
        }
        self.config.database = db.unwrap_or_default().to_string();
        self.history.clear();
        self.refresh_metadata().await
    }

    // ---- users ------------------------------------------------------------------------------

    pub async fn list_users(&mut self) -> Result<Vec<UserInfo>> {
        dispatch!(self, d => d.list_users().await)
    }

    pub async fn user_grants(&mut self, u: &UserInfo) -> Result<Vec<String>> {
        dispatch!(self, d => d.user_grants(u).await)
    }

    pub async fn create_user(&mut self, n: &NewUser) -> Result<()> {
        if n.name.trim().is_empty() {
            return Err(Error::Db("Enter a user name.".into()));
        }
        dispatch!(self, d => d.create_user(n).await)
    }

    pub async fn set_password(&mut self, u: &UserInfo, password: &str) -> Result<()> {
        dispatch!(self, d => d.set_password(u, password).await)
    }

    pub async fn drop_user(&mut self, u: &UserInfo) -> Result<()> {
        dispatch!(self, d => d.drop_user(u).await)
    }

    /// Set what `u` may do in the current database.
    pub async fn set_access(&mut self, u: &UserInfo, level: AccessLevel) -> Result<()> {
        match &mut self.inner {
            Inner::Pg(d) => d.set_access(&u.name, level).await,
            Inner::My(d) => d.set_access(u, level).await,
            Inner::Mongo(m) => m.set_access(u, level).await,
        }
    }

    // ---- export / import --------------------------------------------------------------------

    /// Write the whole current database to `path` (SQL script, or line-oriented JSON for MongoDB).
    pub async fn export_database(&mut self, path: &Path, opts: &DumpOptions, progress: &mut dyn FnMut(String)) -> Result<DumpStats> {
        let part = path.with_extension(format!("{}.part", path.extension().and_then(|e| e.to_str()).unwrap_or("tmp")));
        let file = std::fs::File::create(&part).map_err(|e| Error::Db(format!("Cannot write {}: {e}", path.display())))?;
        let mut out = BufWriter::new(file);
        let (tables, objects) = (self.metadata.tables.clone(), self.metadata.objects.clone());
        let res = match &mut self.inner {
            Inner::Pg(d) => d.dump(opts, &mut out, progress).await,
            Inner::My(d) => d.dump(opts, &tables, &objects, &mut out, progress).await,
            Inner::Mongo(m) => m.dump(opts, &tables, &mut out, progress).await,
        };
        let res = res.and_then(|st| out.flush().map(|_| st).map_err(Error::from));
        drop(out);
        match res {
            Ok(st) => {
                std::fs::rename(&part, path).map_err(|e| Error::Db(format!("Cannot write {}: {e}", path.display())))?;
                Ok(st)
            }
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                Err(e)
            }
        }
    }

    /// Run an exported script (or a dboard MongoDB export) against the current database.
    pub async fn import_database(&mut self, path: &Path, opts: &ImportOptions, progress: &mut dyn FnMut(String)) -> Result<ImportStats> {
        let file = std::fs::File::open(path).map_err(|e| Error::Db(format!("Cannot read {}: {e}", path.display())))?;
        let mut reader = BufReader::new(file);
        let res = match &mut self.inner {
            Inner::Pg(d) => d.import(&mut reader, opts, progress).await,
            Inner::My(d) => d.import(&mut reader, opts, progress).await,
            Inner::Mongo(m) => {
                let target = m.current_database();
                m.import(&mut reader, target.as_deref(), opts, progress).await
            }
        };
        let _ = self.refresh_metadata().await;
        res
    }
}

/// Open a connection just to verify credentials, then drop it. Returns the server version.
pub async fn test_connection(config: ConnectionConfig, password: &str) -> Result<String> {
    let c = Conn::connect(config, password).await?;
    Ok(c.server_version.clone())
}
