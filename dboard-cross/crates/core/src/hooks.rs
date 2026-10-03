//! User hooks: commands that run on events, configured in `hooks.json` in the config folder.
//!
//! ```json
//! [{"event": "before_write", "command": "/usr/local/bin/check-ticket", "args": [], "timeout_secs": 10}]
//! ```
//! The command receives a JSON object on stdin ({event, connection, environment, statement, ...})
//! and the same fields as `DBOARD_*` environment variables. For `before_write` a non-zero exit
//! vetoes the statement and the command's output is shown as the reason.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Hook {
    /// `before_write`, `after_write`, `on_connect` or `on_disconnect`.
    pub event: String,
    pub command: String,
    pub args: Vec<String>,
    pub timeout_secs: u64,
}

impl Default for Hook {
    fn default() -> Self {
        Self { event: String::new(), command: String::new(), args: Vec::new(), timeout_secs: 10 }
    }
}

pub const SAMPLE: &str = r#"[
  {
    "event": "before_write",
    "command": "/path/to/your/script",
    "args": [],
    "timeout_secs": 10
  }
]
"#;

#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    /// What the command printed (stdout then stderr), trimmed.
    pub output: String,
}

pub fn parse(text: &str) -> Result<Vec<Hook>, String> {
    serde_json::from_str::<Vec<Hook>>(text).map_err(|e| format!("hooks.json is not valid: {e}"))
}

/// Run one hook and wait for it (up to its timeout). A hook that cannot start, or times out, fails.
pub fn run(hook: &Hook, payload: &serde_json::Value) -> Outcome {
    let mut cmd = Command::new(&hook.command);
    cmd.args(&hook.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(obj) = payload.as_object() {
        for (k, v) in obj {
            let text = v.as_str().map(String::from).unwrap_or_else(|| v.to_string());
            cmd.env(format!("DBOARD_{}", k.to_uppercase()), text);
        }
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Outcome { ok: false, output: format!("could not start {}: {e}", hook.command) },
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.to_string().as_bytes());
    }
    let deadline = Instant::now() + Duration::from_secs(hook.timeout_secs.max(1));
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Outcome { ok: false, output: format!("{} did not finish within {} s", hook.command, hook.timeout_secs.max(1)) };
            }
            Err(e) => return Outcome { ok: false, output: e.to_string() },
        }
    };
    let (mut out, mut err) = (String::new(), String::new());
    if let Some(mut s) = child.stdout.take() {
        let _ = s.read_to_string(&mut out);
    }
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_string(&mut err);
    }
    let output = format!("{}{}{}", out.trim(), if !out.trim().is_empty() && !err.trim().is_empty() { "\n" } else { "" }, err.trim());
    Outcome { ok: status.success(), output }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serde_json::json;

    fn hook(script: &str) -> Hook {
        Hook { event: "before_write".into(), command: "sh".into(), args: vec!["-c".into(), script.into()], timeout_secs: 5 }
    }

    #[test]
    fn parses_the_config_and_fills_defaults() {
        let hooks = parse(r#"[{"event":"on_connect","command":"x"}]"#).unwrap();
        assert_eq!(hooks[0].timeout_secs, 10);
        assert!(hooks[0].args.is_empty());
        assert!(parse("{ nope").is_err());
        assert!(parse(SAMPLE).is_ok());
    }

    #[test]
    fn exit_status_decides_and_output_is_returned() {
        let ok = run(&hook("echo fine"), &json!({"statement": "UPDATE t"}));
        assert_eq!(ok, Outcome { ok: true, output: "fine".into() });
        let veto = run(&hook("echo 'needs a ticket' >&2; exit 3"), &json!({}));
        assert!(!veto.ok);
        assert_eq!(veto.output, "needs a ticket");
    }

    #[test]
    fn receives_json_on_stdin_and_env_vars() {
        let o = run(&hook(r#"read line; echo "$DBOARD_ENVIRONMENT|$DBOARD_STATEMENT|$line""#), &json!({"environment": "Production", "statement": "DELETE FROM t"}));
        assert!(o.ok);
        assert!(o.output.starts_with("Production|DELETE FROM t|"));
        assert!(o.output.contains("\"statement\":\"DELETE FROM t\""));
    }

    #[test]
    fn slow_or_missing_commands_fail_instead_of_hanging() {
        let slow = Hook { timeout_secs: 1, ..hook("sleep 5") };
        let started = Instant::now();
        let o = run(&slow, &json!({}));
        assert!(!o.ok && o.output.contains("did not finish"));
        assert!(started.elapsed() < Duration::from_secs(4));
        let missing = Hook { command: "/no/such/program".into(), ..Hook::default() };
        assert!(!run(&missing, &json!({})).ok);
    }
}
