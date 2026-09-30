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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    #[serde(default)]
    pub environment: Environment,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            name: "Local Postgres".into(),
            host: "localhost".into(),
            port: 5432,
            database: "postgres".into(),
            username: "postgres".into(),
            environment: Environment::Local,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// Postgres type name usable in a cast (e.g. `int4`, `_text`, `"MyEnum"`), already quoted if needed.
    pub type_name: String,
    pub nullable: bool,
    pub is_primary_key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    Table,
    View,
    MaterializedView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub schema: String,
    pub name: String,
    pub kind: TableKind,
    pub columns: Vec<Column>,
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
}

#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub tables: Vec<Table>,
}

/// A cell value. Everything is fetched as text (`col::text`) so any Postgres
/// type renders without per-type decoding; `None` is SQL NULL.
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
    /// Raw user-supplied WHERE body (without the `WHERE` keyword).
    pub filter: Option<String>,
}
