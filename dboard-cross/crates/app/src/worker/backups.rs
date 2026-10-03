//! Manual and scheduled database backups (part of the worker).

use super::*;

struct Done {
    path: std::path::PathBuf,
    tables: usize,
    rows: u64,
    removed: usize,
}

/// Dump `conn` into `Downloads/dboard-backups/<connection>/` and prune old files. Touches no UI.
async fn backup_conn(conn: &mut Conn, keep: usize) -> Result<Done, String> {
    let name = conn.config.display_name();
    let ext = if conn.config.db_type == DbType::Mongo { "json" } else { "sql" };
    let dir = downloads_dir().join("dboard-backups").join(sanitize(&name));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create the backup folder: {e}"))?;
    let path = dir.join(format!("{}-{}.{ext}", sanitize(&name), chrono::Local::now().format("%Y%m%d-%H%M%S")));
    match conn.export_database(&path, &DumpOptions::default(), &mut |_| {}).await {
        Ok(st) => Ok(Done { removed: crate::backup::prune(&dir, keep), path, tables: st.tables as usize, rows: st.rows as u64 }),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(format!("Backup of {name} failed: {e}"))
        }
    }
}

impl Worker {
    fn note_backup(&mut self, id: &str, d: &Done) {
        self.settings.last_backup.insert(id.to_string(), config::now_secs());
        self.persist_settings();
        self.log_activity(None, &format!("BACKUP {}", d.path.display()));
    }

    /// Back up the active connection now.
    pub(crate) async fn backup_current(&mut self, announce: bool) {
        let keep = self.settings.backup_keep as usize;
        let Some(conn) = self.conn.as_mut() else { return };
        let id = conn.config.id.clone();
        if announce {
            self.toast("Backing up…");
        }
        let Some(conn) = self.conn.as_mut() else { return };
        match backup_conn(conn, keep).await {
            Ok(d) => {
                self.note_backup(&id, &d);
                if announce {
                    let extra = if d.removed > 0 { format!(" ({} old backup(s) removed)", d.removed) } else { String::new() };
                    self.toast(format!("Backed up {} table(s), {} row(s) to {}{extra}", d.tables, d.rows, d.path.display()));
                }
            }
            Err(e) => self.toast(e),
        }
    }

    /// Called on a timer: back up every open connection whose interval has passed, tabs untouched.
    pub(crate) async fn backup_due(&mut self) {
        let every = self.settings.backup_every_hours;
        if every == 0 {
            return;
        }
        let keep = self.settings.backup_keep as usize;
        let now = config::now_secs();
        let is_due = |s: &Settings, id: &str| crate::backup::is_due(now, s.last_backup.get(id).copied(), every);
        let mut failures: Vec<String> = Vec::new();
        let mut finished: Vec<(String, Done)> = Vec::new();

        if let Some(conn) = self.conn.as_mut() {
            let id = conn.config.id.clone();
            if is_due(&self.settings, &id) {
                match backup_conn(conn, keep).await {
                    Ok(d) => finished.push((id, d)),
                    Err(e) => failures.push(e),
                }
            }
        }
        for slot in self.parked.iter_mut() {
            let Some(conn) = slot.as_mut().and_then(|s| s.conn.as_mut()) else { continue };
            let id = conn.config.id.clone();
            if !is_due(&self.settings, &id) {
                continue;
            }
            match backup_conn(conn, keep).await {
                Ok(d) => finished.push((id, d)),
                Err(e) => failures.push(e),
            }
        }
        for (id, d) in &finished {
            self.note_backup(id, d);
        }
        if let Some(e) = failures.first() {
            self.toast(e.clone());
        }
    }
}
