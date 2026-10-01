//! Pure statement builders for user / access management (no I/O, so they can be unit-tested).

use crate::model::{AccessLevel, NewUser};
use crate::sql::{literal, quote_ident, Dialect};

fn my_account(user: &str, host: &str) -> String {
    let host = if host.trim().is_empty() { "%" } else { host.trim() };
    format!("{}@{}", literal(Dialect::My, user), literal(Dialect::My, host))
}

/// Account string `'user'@'host'` for MySQL statements.
pub fn mysql_account(user: &str, host: &str) -> String {
    my_account(user, host)
}

// ---- PostgreSQL --------------------------------------------------------------------------

pub fn pg_create_user(n: &NewUser) -> String {
    let mut s = format!("CREATE ROLE {} WITH LOGIN", quote_ident(&n.name));
    if !n.password.is_empty() {
        s.push_str(&format!(" PASSWORD {}", literal(Dialect::Pg, &n.password)));
    }
    if n.admin {
        s.push_str(" SUPERUSER");
    }
    s
}

pub fn pg_set_password(user: &str, password: &str) -> String {
    format!("ALTER ROLE {} WITH PASSWORD {}", quote_ident(user), literal(Dialect::Pg, password))
}

/// Statements that make `user`'s rights on `db` (every given schema) equal to `level`.
/// Existing rights are revoked first so lowering a level really lowers it.
pub fn pg_set_access(db: &str, user: &str, level: AccessLevel, schemas: &[String]) -> Vec<String> {
    let (u, d) = (quote_ident(user), quote_ident(db));
    let mut out = Vec::new();
    for s in schemas {
        let s = quote_ident(s);
        for what in ["TABLES", "SEQUENCES", "FUNCTIONS"] {
            out.push(format!("REVOKE ALL ON ALL {what} IN SCHEMA {s} FROM {u}"));
            out.push(format!("ALTER DEFAULT PRIVILEGES IN SCHEMA {s} REVOKE ALL ON {what} FROM {u}"));
        }
        out.push(format!("REVOKE ALL ON SCHEMA {s} FROM {u}"));
    }
    out.push(format!("REVOKE ALL ON DATABASE {d} FROM {u}"));
    if level == AccessLevel::None {
        return out;
    }
    out.push(format!("GRANT {} ON DATABASE {d} TO {u}", if level == AccessLevel::Full { "ALL PRIVILEGES" } else { "CONNECT" }));
    for s in schemas {
        let s = quote_ident(s);
        let (schema_priv, table_priv, seq_priv, func_priv) = match level {
            AccessLevel::ReadOnly => ("USAGE", "SELECT", "SELECT", None),
            AccessLevel::ReadWrite => ("USAGE", "SELECT, INSERT, UPDATE, DELETE", "USAGE, SELECT, UPDATE", Some("EXECUTE")),
            _ => ("ALL", "ALL", "ALL", Some("ALL")),
        };
        out.push(format!("GRANT {schema_priv} ON SCHEMA {s} TO {u}"));
        out.push(format!("GRANT {table_priv} ON ALL TABLES IN SCHEMA {s} TO {u}"));
        out.push(format!("GRANT {seq_priv} ON ALL SEQUENCES IN SCHEMA {s} TO {u}"));
        out.push(format!("ALTER DEFAULT PRIVILEGES IN SCHEMA {s} GRANT {table_priv} ON TABLES TO {u}"));
        out.push(format!("ALTER DEFAULT PRIVILEGES IN SCHEMA {s} GRANT {seq_priv} ON SEQUENCES TO {u}"));
        if let Some(f) = func_priv {
            out.push(format!("GRANT {f} ON ALL FUNCTIONS IN SCHEMA {s} TO {u}"));
            out.push(format!("ALTER DEFAULT PRIVILEGES IN SCHEMA {s} GRANT {f} ON FUNCTIONS TO {u}"));
        }
    }
    out
}

// ---- MySQL / MariaDB ---------------------------------------------------------------------

pub fn my_create_user(n: &NewUser) -> String {
    format!("CREATE USER {} IDENTIFIED BY {}", my_account(&n.name, &n.host), literal(Dialect::My, &n.password))
}

pub fn my_set_password(user: &str, host: &str, password: &str) -> String {
    format!("ALTER USER {} IDENTIFIED BY {}", my_account(user, host), literal(Dialect::My, password))
}

pub fn my_drop_user(user: &str, host: &str) -> String {
    format!("DROP USER {}", my_account(user, host))
}

pub fn my_grant_admin(user: &str, host: &str) -> String {
    format!("GRANT ALL PRIVILEGES ON *.* TO {} WITH GRANT OPTION", my_account(user, host))
}

/// `db` is the database to scope the grant to; `None` means every database.
pub fn my_scope(db: Option<&str>) -> String {
    match db {
        Some(d) => format!("{}.*", Dialect::My.quote(d)),
        None => "*.*".into(),
    }
}

/// `(revoke, grant)`. The revoke fails harmlessly when nothing was granted, so callers ignore its error.
pub fn my_set_access(db: Option<&str>, user: &str, host: &str, level: AccessLevel) -> (String, Option<String>) {
    let (scope, acct) = (my_scope(db), my_account(user, host));
    let revoke = format!("REVOKE ALL PRIVILEGES ON {scope} FROM {acct}");
    let privs = match level {
        AccessLevel::None => return (revoke, None),
        AccessLevel::ReadOnly => "SELECT, SHOW VIEW",
        AccessLevel::ReadWrite => "SELECT, INSERT, UPDATE, DELETE, SHOW VIEW, EXECUTE",
        AccessLevel::Full => "ALL PRIVILEGES",
    };
    (revoke, Some(format!("GRANT {privs} ON {scope} TO {acct}")))
}

// ---- MongoDB -----------------------------------------------------------------------------

pub fn mongo_role(level: AccessLevel) -> Option<&'static str> {
    match level {
        AccessLevel::None => None,
        AccessLevel::ReadOnly => Some("read"),
        AccessLevel::ReadWrite => Some("readWrite"),
        AccessLevel::Full => Some("dbOwner"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(admin: bool) -> NewUser {
        NewUser { name: "bo\"b".into(), host: "".into(), password: "p'w".into(), access: AccessLevel::ReadOnly, admin }
    }

    #[test]
    fn pg_statements_quote_everything() {
        assert_eq!(pg_create_user(&user(false)), "CREATE ROLE \"bo\"\"b\" WITH LOGIN PASSWORD 'p''w'");
        assert!(pg_create_user(&user(true)).ends_with(" SUPERUSER"));
        assert_eq!(pg_set_password("a", "x"), "ALTER ROLE \"a\" WITH PASSWORD 'x'");
    }

    #[test]
    fn pg_access_levels() {
        let s = vec!["public".to_string()];
        let none = pg_set_access("d", "u", AccessLevel::None, &s);
        assert!(none.iter().all(|x| x.starts_with("REVOKE") || x.starts_with("ALTER DEFAULT")));
        assert!(none.iter().any(|x| x == "REVOKE ALL ON DATABASE \"d\" FROM \"u\""));
        let ro = pg_set_access("d", "u", AccessLevel::ReadOnly, &s);
        assert!(ro.contains(&"GRANT SELECT ON ALL TABLES IN SCHEMA \"public\" TO \"u\"".to_string()));
        assert!(!ro.iter().any(|x| x.contains("INSERT")));
        let rw = pg_set_access("d", "u", AccessLevel::ReadWrite, &s);
        assert!(rw.contains(&"GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA \"public\" TO \"u\"".to_string()));
        let full = pg_set_access("d", "u", AccessLevel::Full, &s);
        assert!(full.contains(&"GRANT ALL PRIVILEGES ON DATABASE \"d\" TO \"u\"".to_string()));
    }

    #[test]
    fn mysql_statements() {
        assert_eq!(my_create_user(&user(false)), "CREATE USER 'bo\"b'@'%' IDENTIFIED BY 'p\\'w'");
        assert_eq!(my_drop_user("a", "localhost"), "DROP USER 'a'@'localhost'");
        let (rev, grant) = my_set_access(Some("shop"), "a", "%", AccessLevel::ReadWrite);
        assert_eq!(rev, "REVOKE ALL PRIVILEGES ON `shop`.* FROM 'a'@'%'");
        assert_eq!(grant.unwrap(), "GRANT SELECT, INSERT, UPDATE, DELETE, SHOW VIEW, EXECUTE ON `shop`.* TO 'a'@'%'");
        assert!(my_set_access(None, "a", "%", AccessLevel::None).1.is_none());
        assert!(my_set_access(None, "a", "%", AccessLevel::Full).1.unwrap().contains("*.*"));
    }

    #[test]
    fn mongo_roles() {
        assert_eq!(mongo_role(AccessLevel::ReadWrite), Some("readWrite"));
        assert_eq!(mongo_role(AccessLevel::None), None);
    }
}
