//! Scheduling and retention for automatic database backups.

use std::path::{Path, PathBuf};

pub fn is_due(now: u64, last: Option<u64>, every_hours: u32) -> bool {
    every_hours > 0 && last.map_or(true, |l| now.saturating_sub(l) >= every_hours as u64 * 3600)
}

/// Delete the oldest files in `dir` (by name, which carries a timestamp) beyond `keep`.
pub fn prune(dir: &Path, keep: usize) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect();
    files.sort();
    let excess = files.len().saturating_sub(keep.max(1));
    files.into_iter().take(excess).filter(|p| std::fs::remove_file(p).is_ok()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_when_never_run_or_interval_passed() {
        assert!(!is_due(1000, None, 0), "0 hours means off");
        assert!(is_due(1000, None, 6));
        assert!(!is_due(10_000, Some(9_000), 1));
        assert!(is_due(9_000 + 3600, Some(9_000), 1));
    }

    #[test]
    fn prune_keeps_the_newest_files() {
        let dir = std::env::temp_dir().join(format!("dboard-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["db-20240101-000000.sql", "db-20240102-000000.sql", "db-20240103-000000.sql", "db-20240104-000000.sql"] {
            std::fs::write(dir.join(n), "x").unwrap();
        }
        assert_eq!(prune(&dir, 2), 2);
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["db-20240103-000000.sql", "db-20240104-000000.sql"]);
        assert_eq!(prune(&dir, 0), 1, "never deletes the last remaining backup");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
