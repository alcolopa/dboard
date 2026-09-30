//! Saved connections (JSON in the OS config dir) + passwords in the OS keyring.

use crate::model::ConnectionConfig;
use std::path::PathBuf;

const SERVICE: &str = "dboard";

fn config_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("dboard").join("connections.json"))
}

pub fn load_connections() -> Vec<ConnectionConfig> {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_connections(list: &[ConnectionConfig]) -> std::io::Result<()> {
    let Some(p) = config_path() else { return Ok(()) };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, serde_json::to_string_pretty(list).unwrap_or_default())
}

fn account(c: &ConnectionConfig) -> String {
    format!("{}@{}:{}/{}", c.username, c.host, c.port, c.database)
}

pub fn load_password(c: &ConnectionConfig) -> Option<String> {
    keyring::Entry::new(SERVICE, &account(c)).ok()?.get_password().ok()
}

pub fn save_password(c: &ConnectionConfig, pw: &str) {
    if let Ok(e) = keyring::Entry::new(SERVICE, &account(c)) {
        let _ = e.set_password(pw);
    }
}
