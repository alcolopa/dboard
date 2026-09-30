use crate::model::Environment;

const DESTRUCTIVE: [&str; 8] = [
    "DROP TABLE", "DROP DATABASE", "DROP SCHEMA", "DROP VIEW",
    "TRUNCATE", "DELETE FROM", "ALTER TABLE", "DROP COLUMN",
];

pub fn is_destructive(sql: &str) -> bool {
    let up = sql.trim().to_uppercase();
    DESTRUCTIVE.iter().any(|k| up.contains(k))
}

pub fn requires_confirmation(env: Environment, destructive: bool) -> bool {
    destructive && env.requires_destructive_confirmation()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_destructive() {
        assert!(is_destructive("  drop table users"));
        assert!(is_destructive("DELETE FROM t WHERE 1=1"));
        assert!(!is_destructive("SELECT * FROM t"));
    }

    #[test]
    fn confirmation_only_prod_staging() {
        assert!(requires_confirmation(Environment::Production, true));
        assert!(requires_confirmation(Environment::Staging, true));
        assert!(!requires_confirmation(Environment::Local, true));
        assert!(!requires_confirmation(Environment::Production, false));
    }
}
