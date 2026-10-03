use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Environment {
    Production,
    Staging,
    Development,
    #[default]
    Local,
}

impl Environment {
    pub const ALL: [Environment; 4] = [Self::Production, Self::Staging, Self::Development, Self::Local];

    pub fn label(self) -> &'static str {
        match self {
            Self::Production => "Production",
            Self::Staging => "Staging",
            Self::Development => "Development",
            Self::Local => "Local",
        }
    }

    /// Badge colour as 0xRRGGBB (same palette as the Mac app).
    pub fn color(self) -> u32 {
        match self {
            Self::Production => 0xEF4444,
            Self::Staging => 0xF59E0B,
            Self::Development => 0x3B82F6,
            Self::Local => 0x10B981,
        }
    }

    pub fn requires_destructive_confirmation(self) -> bool {
        matches!(self, Self::Production | Self::Staging)
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|e| *e == self).unwrap_or(3)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DbType {
    #[default]
    Postgres,
    MySql,
    Mongo,
}

impl DbType {
    pub const ALL: [DbType; 3] = [Self::Postgres, Self::MySql, Self::Mongo];

    pub fn label(self) -> &'static str {
        match self {
            Self::Postgres => "PostgreSQL",
            Self::MySql => "MySQL / MariaDB",
            Self::Mongo => "MongoDB",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::Postgres => 5432,
            Self::MySql => 3306,
            Self::Mongo => 27017,
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|e| *e == self).unwrap_or(0)
    }

    pub fn is_sql(self) -> bool {
        !matches!(self, Self::Mongo)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
    /// Verify the server certificate against the built-in web PKI roots.
    VerifyFull,
}

impl SslMode {
    pub const ALL: [SslMode; 4] = [Self::Disable, Self::Prefer, Self::Require, Self::VerifyFull];
    pub fn label(self) -> &'static str {
        match self {
            Self::Disable => "Disable",
            Self::Prefer => "Prefer",
            Self::Require => "Require",
            Self::VerifyFull => "Verify full",
        }
    }
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|e| *e == self).unwrap_or(1)
    }
}

/// A saved connection. Every field is `#[serde(default)]` so config files written
/// by older or newer versions keep loading (unknown fields are ignored).
/// Nothing here is a default credential: host/user/database start empty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub db_type: DbType,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub environment: Environment,
    pub ssl: SslMode,
    /// MongoDB only: full connection string (overrides host/port/user).
    pub mongo_uri: String,
    pub remember_password: bool,
    /// Optional SSH tunnel (system `ssh`): bastion host, port, user and private key path.
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub ssh_key: String,
    /// Unix seconds; used to sort "recent" connections.
    pub last_used: u64,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            db_type: DbType::Postgres,
            host: String::new(),
            port: DbType::Postgres.default_port(),
            database: String::new(),
            username: String::new(),
            environment: Environment::Local,
            ssl: SslMode::Prefer,
            mongo_uri: String::new(),
            remember_password: true,
            ssh_host: String::new(),
            ssh_port: 22,
            ssh_user: String::new(),
            ssh_key: String::new(),
            last_used: 0,
        }
    }
}

impl ConnectionConfig {
    pub fn new_blank() -> Self {
        Self { id: crate::config::new_id(), ..Default::default() }
    }

    pub fn display_name(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.clone();
        }
        match self.db_type {
            DbType::Mongo if !self.mongo_uri.is_empty() => "MongoDB".into(),
            _ if !self.host.is_empty() => format!("{}@{}", self.username, self.host),
            _ => "Untitled".into(),
        }
    }

    /// Returns a user-facing problem with the form, if any.
    pub fn validate(&self) -> Option<&'static str> {
        if self.db_type == DbType::Mongo && !self.mongo_uri.trim().is_empty() {
            return None;
        }
        if self.host.trim().is_empty() {
            return Some("Enter a host.");
        }
        if self.port == 0 {
            return Some("Enter a valid port.");
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// Type usable in a Postgres cast (already quoted if needed); informational for other engines.
    pub type_name: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    pub default: Option<String>,
    /// `schema.table(column)` this column references, if it is a foreign key.
    pub fk: Option<String>,
}

impl Column {
    pub fn is_bool(&self) -> bool {
        matches!(self.type_name.as_str(), "boolean" | "bool" | "tinyint(1)")
    }
    pub fn is_json(&self) -> bool {
        matches!(self.type_name.as_str(), "json" | "jsonb" | "object" | "array")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    Table,
    View,
    MaterializedView,
    Collection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub schema: String,
    pub name: String,
    pub kind: TableKind,
    pub columns: Vec<Column>,
    pub estimated_rows: Option<i64>,
    pub size_bytes: Option<i64>,
    pub indexes: Vec<String>,
    /// No primary key, but rows can still be addressed exactly (PostgreSQL `ctid`), so inline
    /// edits are allowed. Set by the driver.
    pub keyless_edit: bool,
}

impl Table {
    pub fn primary_keys(&self) -> Vec<&Column> {
        self.columns.iter().filter(|c| c.is_primary_key).collect()
    }
    pub fn has_primary_key(&self) -> bool {
        self.columns.iter().any(|c| c.is_primary_key)
    }
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }
    pub fn full_name(&self) -> String {
        if self.schema.is_empty() { self.name.clone() } else { format!("{}.{}", self.schema, self.name) }
    }
    /// Only base tables / collections whose rows can be addressed exactly are edited inline.
    pub fn is_editable(&self) -> bool {
        matches!(self.kind, TableKind::Table | TableKind::Collection) && (self.has_primary_key() || self.keyless_edit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Function,
    Procedure,
    Sequence,
    Trigger,
    Type,
    Index,
    Event,
    Extension,
}

impl ObjectKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Procedure => "procedure",
            Self::Sequence => "sequence",
            Self::Trigger => "trigger",
            Self::Type => "type",
            Self::Index => "index",
            Self::Event => "event",
            Self::Extension => "extension",
        }
    }
}

/// Everything in a database that is not a table or view: routines, sequences, triggers, ...
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbObject {
    pub schema: String,
    pub name: String,
    pub kind: ObjectKind,
    /// Kind-specific: routine argument list, the table a trigger / index belongs to, ...
    pub detail: String,
}

/// An account that can log in to the server.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserInfo {
    pub name: String,
    /// MySQL: the host part of `'user'@'host'`; MongoDB: the database the user is defined in.
    pub origin: String,
    /// Short human summary, e.g. "superuser · can create databases".
    pub summary: String,
}

impl UserInfo {
    pub fn display(&self) -> String {
        if self.origin.is_empty() { self.name.clone() } else { format!("{} @ {}", self.name, self.origin) }
    }
}

/// How much a user may do in the current database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessLevel {
    None,
    ReadOnly,
    ReadWrite,
    Full,
}

impl AccessLevel {
    pub const ALL: [AccessLevel; 4] = [Self::None, Self::ReadOnly, Self::ReadWrite, Self::Full];
    pub const LABELS: [&'static str; 4] = ["No access", "Read only", "Read & write", "Full control"];

    pub fn from_index(i: i32) -> Self {
        Self::ALL[(i.max(0) as usize).min(3)]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUser {
    pub name: String,
    /// MySQL host (defaults to `%`); ignored elsewhere.
    pub host: String,
    pub password: String,
    pub access: AccessLevel,
    /// PostgreSQL: SUPERUSER. MySQL: all privileges on `*.*`. MongoDB: the `root` role.
    pub admin: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub tables: Vec<Table>,
    pub objects: Vec<DbObject>,
}

/// A cell value. Everything is fetched as text so any type renders without
/// per-type decoding; `None` is SQL NULL.
pub type Cell = Option<String>;

#[derive(Debug, Clone, Default)]
pub struct Rows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    pub duration_ms: f64,
    pub total_estimate: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Page {
    pub limit: i64,
    pub offset: i64,
    pub sort_column: Option<String>,
    pub sort_ascending: bool,
    /// Raw user-supplied WHERE body (SQL) or a JSON filter document (MongoDB).
    pub filter: Option<String>,
}
