//! Password references: instead of a literal password, the password field may hold
//!   `env:NAME`            read the environment variable
//!   `op://vault/item/f`   read from 1Password with the `op` CLI
//!   `aws-rds-iam[:region]`  an RDS IAM auth token from the `aws` CLI
//!   `gcloud-sql-iam`      a Cloud SQL IAM login token from the `gcloud` CLI
//! References are resolved each time a connection opens, so rotating tokens just work.

use crate::model::ConnectionConfig;
use crate::{Error, Result};

#[derive(Debug, PartialEq, Eq)]
pub enum Reference {
    Literal,
    Env(String),
    Command { program: String, args: Vec<String>, what: &'static str },
}

pub fn classify(cfg: &ConnectionConfig, raw: &str) -> Reference {
    let raw = raw.trim();
    if let Some(name) = raw.strip_prefix("env:") {
        return Reference::Env(name.trim().to_string());
    }
    if raw.starts_with("op://") {
        return Reference::Command { program: "op".into(), args: vec!["read".into(), "--no-newline".into(), raw.into()], what: "1Password" };
    }
    if raw == "aws-rds-iam" || raw.starts_with("aws-rds-iam:") {
        let mut args: Vec<String> = ["rds", "generate-db-auth-token", "--hostname", &cfg.host, "--port", &cfg.port.to_string(), "--username", &cfg.username].iter().map(|s| s.to_string()).collect();
        if let Some(region) = raw.strip_prefix("aws-rds-iam:").filter(|r| !r.trim().is_empty()) {
            args.push("--region".into());
            args.push(region.trim().into());
        }
        return Reference::Command { program: "aws".into(), args, what: "the AWS CLI" };
    }
    if raw == "gcloud-sql-iam" {
        return Reference::Command { program: "gcloud".into(), args: vec!["sql".into(), "generate-login-token".into()], what: "the gcloud CLI" };
    }
    Reference::Literal
}

/// Turn whatever is in the password field into the actual password.
pub async fn resolve(cfg: &ConnectionConfig, raw: &str) -> Result<String> {
    match classify(cfg, raw) {
        Reference::Literal => Ok(raw.to_string()),
        Reference::Env(name) => std::env::var(&name).map_err(|_| Error::Db(format!("The environment variable {name} is not set."))),
        Reference::Command { program, args, what } => {
            let out = tokio::task::spawn_blocking(move || std::process::Command::new(&program).args(&args).output().map_err(|e| (program, e)))
                .await
                .map_err(|e| Error::Db(e.to_string()))?
                .map_err(|(program, e)| Error::Db(format!("Could not run `{program}` to get the password from {what}: {e}. Is it installed and signed in?")))?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                return Err(Error::Db(format!("{what} could not provide the password: {}", err.trim())));
            }
            Ok(String::from_utf8_lossy(&out.stdout).trim_end_matches(['\n', '\r']).to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ConnectionConfig {
        ConnectionConfig { host: "db.example.com".into(), port: 5432, username: "app".into(), ..Default::default() }
    }

    #[test]
    fn classifies_references() {
        assert_eq!(classify(&cfg(), "hunter2"), Reference::Literal);
        assert_eq!(classify(&cfg(), "env: DB_PASS"), Reference::Env("DB_PASS".into()));
        match classify(&cfg(), "op://Private/prod-db/password") {
            Reference::Command { program, args, .. } => {
                assert_eq!(program, "op");
                assert_eq!(args.last().unwrap(), "op://Private/prod-db/password");
            }
            other => panic!("{other:?}"),
        }
        match classify(&cfg(), "aws-rds-iam:eu-west-1") {
            Reference::Command { program, args, .. } => {
                assert_eq!(program, "aws");
                assert!(args.windows(2).any(|w| w == ["--hostname", "db.example.com"]));
                assert!(args.windows(2).any(|w| w == ["--region", "eu-west-1"]));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(classify(&cfg(), "gcloud-sql-iam"), Reference::Command { .. }));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolves_env_and_reports_missing() {
        std::env::set_var("DBOARD_TEST_SECRET", "s3cret");
        assert_eq!(resolve(&cfg(), "env:DBOARD_TEST_SECRET").await.unwrap(), "s3cret");
        assert_eq!(resolve(&cfg(), "plain").await.unwrap(), "plain");
        let err = resolve(&cfg(), "env:DBOARD_SURELY_UNSET").await.unwrap_err().to_string();
        assert!(err.contains("DBOARD_SURELY_UNSET"));
    }

    /// Runs only when a stand-in `op` that prints `from-op` is first on PATH (set DBOARD_TEST_FAKE_OP=1).
    #[tokio::test(flavor = "current_thread")]
    async fn reads_the_password_from_a_cli() {
        if std::env::var("DBOARD_TEST_FAKE_OP").is_err() {
            return;
        }
        assert_eq!(resolve(&cfg(), "op://x/y/z").await.unwrap(), "from-op");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_cli_gives_a_helpful_message() {
        if std::env::var("DBOARD_TEST_FAKE_OP").is_ok() {
            return;
        }
        let err = resolve(&cfg(), "op://x/y/z").await.unwrap_err().to_string();
        assert!(err.contains("1Password"), "{err}");
    }
}
