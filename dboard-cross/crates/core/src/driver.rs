//! One connection to any supported database, behind a single API.

use crate::edit::{EditHistory, EditRecord};
use crate::model::*;
use crate::mongo::Mongo;
use crate::mysql::My;
use crate::pg::Pg;
use crate::{Error, Result};

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

pub struct Conn {
    inner: Inner,
    pub config: ConnectionConfig,
    pub metadata: Metadata,
    pub history: EditHistory,
    pub server_version: String,
}

impl Conn {
    pub async fn connect(config: ConnectionConfig, password: &str) -> Result<Self> {
        let inner = match config.db_type {
            DbType::Postgres => Inner::Pg(Pg::connect(&config, password).await?),
            DbType::MySql => Inner::My(My::connect(&config, password).await?),
            DbType::Mongo => Inner::Mongo(Mongo::connect(&config, password).await?),
        };
        let mut c = Self { inner, config, metadata: Metadata::default(), history: EditHistory::default(), server_version: String::new() };
        c.server_version = dispatch!(c, d => d.version().await).unwrap_or_default();
        c.refresh_metadata().await?;
        Ok(c)
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

    /// Key values (in PK column order) for a displayed row.
    fn key_for(t: &Table, row: &[Cell]) -> Vec<String> {
        t.primary_keys()
            .iter()
            .map(|pk| t.columns.iter().position(|c| c.name == pk.name).and_then(|i| row.get(i).cloned().flatten()).unwrap_or_default())
            .collect()
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
        let old = t.columns.iter().position(|c| c.name == column).and_then(|i| row.get(i).cloned()).unwrap_or(None);
        dispatch!(self, d => d.update(&t, column, &key, new.as_deref()).await)?;
        self.history.push(EditRecord { schema: schema.into(), table: name.into(), column: column.into(), key, old, new });
        Ok(())
    }

    /// Revert the most recent edit by writing the old value back.
    pub async fn undo(&mut self) -> Result<Option<EditRecord>> {
        let Some(r) = self.history.pop() else { return Ok(None) };
        let t = match self.table_owned(&r.schema, &r.table) {
            Ok(t) => t,
            Err(e) => {
                self.history.push(r);
                return Err(e);
            }
        };
        if let Err(e) = dispatch!(self, d => d.update(&t, &r.column, &r.key, r.old.as_deref()).await) {
            self.history.push(r); // keep it so the user can retry
            return Err(e);
        }
        Ok(Some(r))
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
        dispatch!(self, d => d.delete(&t, &key).await)
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

/// Open a connection just to verify credentials, then drop it. Returns the server version.
pub async fn test_connection(config: ConnectionConfig, password: &str) -> Result<String> {
    let c = Conn::connect(config, password).await?;
    Ok(c.server_version.clone())
}
