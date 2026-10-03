//! Parse `postgres://user:pass@host:5432/db?sslmode=require` style connection strings.

use crate::model::{DbType, SslMode};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedUrl {
    pub db_type: DbType,
    pub host: String,
    pub port: Option<u16>,
    pub user: String,
    pub password: String,
    pub database: String,
    pub ssl: Option<SslMode>,
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `None` when the text is not a postgres / mysql URL (MongoDB strings go in their own field).
pub fn parse(text: &str) -> Option<ParsedUrl> {
    let text = text.trim();
    let (scheme, rest) = text.split_once("://")?;
    let db_type = match scheme.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" => DbType::Postgres,
        "mysql" | "mariadb" => DbType::MySql,
        _ => return None,
    };
    let (rest, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (auth, hostpart) = match rest.rfind('@') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => ("", rest),
    };
    let (hostport, database) = hostpart.split_once('/').unwrap_or((hostpart, ""));
    let (user, password) = auth.split_once(':').unwrap_or((auth, ""));
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => (h, p.parse().ok()),
        _ => (hostport, None),
    };
    let ssl = query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k != "sslmode" && k != "ssl-mode" && k != "ssl" {
            return None;
        }
        Some(match v.to_ascii_lowercase().as_str() {
            "disable" | "false" => SslMode::Disable,
            "prefer" | "preferred" => SslMode::Prefer,
            "require" | "required" | "true" => SslMode::Require,
            "verify-full" | "verify_identity" | "verify-ca" | "verify_ca" => SslMode::VerifyFull,
            _ => return None,
        })
    });
    Some(ParsedUrl {
        db_type,
        host: host.trim_matches(|c| c == '[' || c == ']').to_string(),
        port,
        user: decode(user),
        password: decode(password),
        database: decode(database),
        ssl,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_postgres() {
        let p = parse("postgres://al%40x:p%3Aw@db.example.com:6543/shop?sslmode=require").unwrap();
        assert_eq!(p.db_type, DbType::Postgres);
        assert_eq!((p.host.as_str(), p.port), ("db.example.com", Some(6543)));
        assert_eq!((p.user.as_str(), p.password.as_str(), p.database.as_str()), ("al@x", "p:w", "shop"));
        assert_eq!(p.ssl, Some(SslMode::Require));
    }

    #[test]
    fn parses_minimal_mysql() {
        let p = parse("mysql://localhost/app").unwrap();
        assert_eq!(p.db_type, DbType::MySql);
        assert_eq!((p.host.as_str(), p.port, p.user.as_str(), p.database.as_str()), ("localhost", None, "", "app"));
    }

    #[test]
    fn rejects_other_schemes() {
        assert!(parse("mongodb://h/db").is_none());
        assert!(parse("not a url").is_none());
    }
}
