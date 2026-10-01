//! Everything the app persists between runs.
//!
//! * Files live in the OS config dir (`~/.config/dboard`, `%APPDATA%\dboard`,
//!   `~/Library/Application Support/dboard`), overridable with `DBOARD_CONFIG_DIR`.
//! * Every file is a JSON object with a `version` and `#[serde(default)]` fields, so files
//!   written by older or newer builds keep loading; unknown fields are ignored.
//! * Writes are atomic (write temp file, then rename) and a file that fails to parse is
//!   copied to `<name>.corrupt-<ts>` instead of being silently overwritten.
//! * Passwords never touch these files; they live in the OS keyring, keyed by connection id.

use crate::model::ConnectionConfig;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const SERVICE: &str = "dboard";
const FORMAT_VERSION: u32 = 1;

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

// ---------------------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// The default per-user location.
    pub fn open_default() -> Self {
        Self { dir: default_dir() }
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    fn read<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        let path = self.dir.join(name);
        let text = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str(&text) {
            Ok(v) => Some(v),
            Err(_) => {
                // Keep the unreadable file around for recovery.
                let _ = std::fs::copy(&path, self.dir.join(format!("{name}.corrupt-{}", now_secs())));
                None
            }
        }
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!("{name}.tmp"));
        let text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, self.dir.join(name))
    }

    // ---- connections -----------------------------------------------------------------

    pub fn load_connections(&self) -> Vec<ConnectionConfig> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum File {
            Current(ConnectionsFile),
            /// Pre-1.0 builds wrote a bare JSON array.
            Legacy(Vec<ConnectionConfig>),
        }
        let mut list = match self.read::<File>("connections.json") {
            Some(File::Current(f)) => f.connections,
            Some(File::Legacy(l)) => l,
            None => Vec::new(),
        };
        let mut migrated = false;
        for c in &mut list {
            if c.id.is_empty() {
                c.id = new_id();
                migrated = true;
            }
        }
        if migrated {
            let _ = self.save_connections(&list);
        }
        list
    }

    pub fn save_connections(&self, list: &[ConnectionConfig]) -> std::io::Result<()> {
        self.write("connections.json", &ConnectionsFile { version: FORMAT_VERSION, connections: list.to_vec() })
    }

    // ---- settings --------------------------------------------------------------------

    pub fn load_settings(&self) -> Settings {
        self.read("settings.json").unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> std::io::Result<()> {
        self.write("settings.json", s)
    }

    // ---- query history & saved queries -------------------------------------------------

    pub fn load_history(&self) -> Vec<HistoryEntry> {
        self.read::<HistoryFile>("history.json").map(|f| f.entries).unwrap_or_default()
    }

    pub fn save_history(&self, entries: &[HistoryEntry]) -> std::io::Result<()> {
        self.write("history.json", &HistoryFile { version: FORMAT_VERSION, entries: entries.to_vec() })
    }

    pub fn load_saved_queries(&self) -> Vec<SavedQuery> {
        self.read::<SavedFile>("saved_queries.json").map(|f| f.queries).unwrap_or_default()
    }

    pub fn save_saved_queries(&self, q: &[SavedQuery]) -> std::io::Result<()> {
        self.write("saved_queries.json", &SavedFile { version: FORMAT_VERSION, queries: q.to_vec() })
    }
}

fn default_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("DBOARD_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("dboard")
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct ConnectionsFile {
    version: u32,
    connections: Vec<ConnectionConfig>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct HistoryFile {
    version: u32,
    entries: Vec<HistoryEntry>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct SavedFile {
    version: u32,
    queries: Vec<SavedQuery>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub theme: Theme,
    pub compact_density: bool,
    pub default_page_size: i64,
    pub confirm_destructive: bool,
    pub autocomplete: bool,
    pub editor_font_size: u32,
    pub inspector_open: bool,
    /// Docked and kept open across restarts; otherwise the inspector floats over the workspace.
    pub inspector_pinned: bool,
    pub last_connection_id: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            theme: Theme::Dark,
            compact_density: true,
            default_page_size: 100,
            confirm_destructive: true,
            autocomplete: true,
            editor_font_size: 13,
            inspector_open: false,
            inspector_pinned: false,
            last_connection_id: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct HistoryEntry {
    pub sql: String,
    pub connection_id: String,
    pub at: u64,
    pub duration_ms: f64,
    pub ok: bool,
}

pub const FOLDERS: [&str; 5] = ["General", "Users", "Analytics", "Production", "Debugging"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SavedQuery {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub sql: String,
}

// ---------------------------------------------------------------------------------------
// Passwords (OS keyring)
// ---------------------------------------------------------------------------------------

pub mod secrets {
    use super::SERVICE;
    use crate::model::ConnectionConfig;

    fn entry(account: &str) -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, account).ok()
    }

    /// Legacy account name used by the first builds (`user@host:port/db`).
    fn legacy_account(c: &ConnectionConfig) -> String {
        format!("{}@{}:{}/{}", c.username, c.host, c.port, c.database)
    }

    pub fn get(c: &ConnectionConfig) -> Option<String> {
        if let Some(pw) = entry(&c.id).and_then(|e| e.get_password().ok()) {
            return Some(pw);
        }
        // Migrate a legacy entry forward the first time it is found.
        let pw = entry(&legacy_account(c)).and_then(|e| e.get_password().ok())?;
        let _ = set(c, &pw);
        Some(pw)
    }

    /// `Err` carries a readable reason (e.g. no keyring service on this machine).
    pub fn set(c: &ConnectionConfig, password: &str) -> Result<(), String> {
        entry(&c.id)
            .ok_or_else(|| "no OS keyring available".to_string())?
            .set_password(password)
            .map_err(|e| e.to_string())
    }

    pub fn delete(c: &ConnectionConfig) {
        for acc in [c.id.clone(), legacy_account(c)] {
            if let Some(e) = entry(&acc) {
                let _ = e.delete_credential();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Environment;

    fn tmp() -> Store {
        let d = std::env::temp_dir().join(format!("dboard-test-{}", new_id()));
        Store::at(d)
    }

    #[test]
    fn connections_round_trip() {
        let s = tmp();
        assert!(s.load_connections().is_empty());
        let mut c = ConnectionConfig::new_blank();
        c.name = "Prod".into();
        c.host = "db.example.com".into();
        c.environment = Environment::Production;
        s.save_connections(&[c.clone()]).unwrap();
        assert_eq!(s.load_connections(), vec![c]);
    }

    #[test]
    fn blank_connection_has_no_default_credentials() {
        let c = ConnectionConfig::new_blank();
        assert!(c.host.is_empty() && c.username.is_empty() && c.database.is_empty());
        assert!(c.validate().is_some());
    }

    #[test]
    fn migrates_legacy_array_without_ids() {
        let s = tmp();
        std::fs::create_dir_all(s.dir()).unwrap();
        // Shape written by the first builds: bare array, no id / db_type / ssl.
        std::fs::write(
            s.dir().join("connections.json"),
            r#"[{"name":"Old","host":"h","port":5432,"database":"d","username":"u","environment":"Production"}]"#,
        )
        .unwrap();
        let l = s.load_connections();
        assert_eq!(l.len(), 1);
        assert!(!l[0].id.is_empty());
        assert_eq!(l[0].environment, Environment::Production);
        // The migrated file is written back and stable on the next load.
        assert_eq!(s.load_connections()[0].id, l[0].id);
    }

    #[test]
    fn ignores_unknown_fields_from_newer_versions() {
        let s = tmp();
        std::fs::create_dir_all(s.dir()).unwrap();
        std::fs::write(
            s.dir().join("connections.json"),
            r#"{"version":99,"future":true,"connections":[{"id":"x","name":"N","host":"h","brand_new_field":1}]}"#,
        )
        .unwrap();
        let l = s.load_connections();
        assert_eq!(l[0].id, "x");
    }

    #[test]
    fn corrupt_file_is_preserved_not_lost() {
        let s = tmp();
        std::fs::create_dir_all(s.dir()).unwrap();
        std::fs::write(s.dir().join("connections.json"), "{ not json").unwrap();
        assert!(s.load_connections().is_empty());
        let backups = std::fs::read_dir(s.dir()).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().contains("corrupt")).count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn settings_defaults_fill_missing_fields() {
        let s = tmp();
        std::fs::create_dir_all(s.dir()).unwrap();
        std::fs::write(s.dir().join("settings.json"), r#"{"theme":"Light"}"#).unwrap();
        let st = s.load_settings();
        assert_eq!(st.theme, Theme::Light);
        assert_eq!(st.default_page_size, 100);
    }

    #[test]
    fn history_and_saved_queries_persist() {
        let s = tmp();
        s.save_history(&[HistoryEntry { sql: "select 1".into(), ok: true, ..Default::default() }]).unwrap();
        assert_eq!(s.load_history()[0].sql, "select 1");
        s.save_saved_queries(&[SavedQuery { id: "1".into(), name: "n".into(), folder: "Users".into(), sql: "s".into() }]).unwrap();
        assert_eq!(s.load_saved_queries()[0].folder, "Users");
    }
}
