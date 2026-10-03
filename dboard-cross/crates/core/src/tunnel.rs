//! SSH port forwarding through the system `ssh` client (`ssh -N -L`). Keys and agents work as
//! usual; password prompts do not (BatchMode), so use a key or ssh-agent.

use crate::{Error, Result};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Tunnel {
    child: Child,
    pub local_port: u16,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct TunnelSpec<'a> {
    pub ssh_host: &'a str,
    pub ssh_port: u16,
    pub ssh_user: &'a str,
    pub key_path: &'a str,
    pub remote_host: &'a str,
    pub remote_port: u16,
}

/// Arguments for the `ssh` command line (separate so they can be unit tested).
pub fn ssh_args(spec: &TunnelSpec, local_port: u16) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-N".into(),
        "-o".into(), "ExitOnForwardFailure=yes".into(),
        "-o".into(), "BatchMode=yes".into(),
        "-o".into(), "StrictHostKeyChecking=accept-new".into(),
        "-o".into(), "ServerAliveInterval=30".into(),
        "-L".into(), format!("127.0.0.1:{local_port}:{}:{}", spec.remote_host, spec.remote_port),
        "-p".into(), spec.ssh_port.to_string(),
    ];
    if !spec.key_path.trim().is_empty() {
        a.push("-i".into());
        a.push(spec.key_path.trim().to_string());
    }
    let target = if spec.ssh_user.trim().is_empty() { spec.ssh_host.to_string() } else { format!("{}@{}", spec.ssh_user.trim(), spec.ssh_host) };
    a.push(target);
    a
}

pub async fn open(spec: TunnelSpec<'_>) -> Result<Tunnel> {
    let local_port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).and_then(|l| l.local_addr()).map_err(|e| Error::Db(format!("No free local port: {e}")))?.port();
    let mut child = Command::new("ssh")
        .args(ssh_args(&spec, local_port))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Db(format!("Could not start the ssh client: {e}. Is OpenSSH installed?")))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let mut msg = String::new();
            if let Some(mut e) = child.stderr.take() {
                use std::io::Read;
                let _ = e.read_to_string(&mut msg);
            }
            let msg = msg.trim();
            return Err(Error::Db(format!("SSH tunnel failed ({status}){}", if msg.is_empty() { String::new() } else { format!(": {msg}") })));
        }
        if TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, local_port).into(), Duration::from_millis(200)).is_ok() {
            return Ok(Tunnel { child, local_port });
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err(Error::Db("SSH tunnel timed out after 15 s.".into()));
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_forward_args() {
        let s = TunnelSpec { ssh_host: "bastion.example.com", ssh_port: 2222, ssh_user: "me", key_path: "/k/id", remote_host: "db.internal", remote_port: 5432 };
        let a = ssh_args(&s, 40000);
        assert!(a.contains(&"127.0.0.1:40000:db.internal:5432".to_string()));
        assert!(a.windows(2).any(|w| w == ["-i", "/k/id"]));
        assert_eq!(a.last().unwrap(), "me@bastion.example.com");
    }
}
