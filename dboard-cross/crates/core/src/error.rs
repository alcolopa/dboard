use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Db(String),
    #[error("not connected")]
    NotConnected,
    #[error("{0}")]
    Unsafe(String),
}

impl From<tokio_postgres::Error> for Error {
    fn from(e: tokio_postgres::Error) -> Self {
        if let Some(db) = e.as_db_error() {
            return Error::Db(humanize(db.code().code(), db.message()));
        }
        // Include the underlying cause (e.g. "Connection refused") instead of just
        // "error connecting to server".
        let mut msg = e.to_string();
        let mut src = std::error::Error::source(&e);
        while let Some(s) = src {
            msg.push_str(&format!(": {s}"));
            src = s.source();
        }
        Error::Db(msg)
    }
}

/// Turns common Postgres errors into readable messages (mirrors the Mac app).
pub fn humanize(code: &str, message: &str) -> String {
    match code {
        "23505" => "A record with this unique value already exists in the table.".into(),
        "23503" => "This change violates a foreign key constraint.".into(),
        "23502" => "A required (NOT NULL) column cannot be empty.".into(),
        "23514" => "This change violates a CHECK constraint.".into(),
        "22P02" | "22003" | "22007" | "22008" => format!("Invalid value for this column type: {message}"),
        "42501" => "Permission denied for this operation.".into(),
        _ => message.to_string(),
    }
}
