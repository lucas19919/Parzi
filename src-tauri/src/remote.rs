use std::path::PathBuf;
use std::time::Duration;

const SSH_TIMEOUT: Duration = Duration::from_secs(25);
const SETUP_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(serde::Serialize)]
pub struct RemoteReport {
    pub connected: bool,
    pub method: String,
    pub prepared: bool,
    pub detail: String,
}

fn clean(field: &str, what: &str) -> Result<String, String> {
    let t = field.trim();
    if t.is_empty() {
        return Err(format!("enter an SSH {what}"));
    }
    if t.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("{what} must be a single word"));
    }
    Ok(t.to_string())
}

/// Escape a password for `echo(` in a batch askpass helper. Returns
/// None when the password cannot pass through (newlines).
fn batch_escape(password: &str) -> Option<String> {
    if password.chars().any(|c| c == '\r' || c == '\n') {
        return None;
    }
    let mut out = String::with_capacity(password.len() + 4);
    for c in password.chars() {
        match c {
            '%' => out.push_str("%%"),
            '&' | '|' | '<' | '>' | '^' => {
                out.push('^');
                out.push(c);
            }
            '"' => out.push_str("^\""),
            _ => out.push(c),
        }
    }
    Some(out)
}

struct AskpassGuard {
    path: PathBuf,
}

impl Drop for AskpassGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn write_askpass(password: &str) -> Result<AskpassGuard, String> {
    let Some(escaped) = batch_escape(password) else {
        return Err("that password has characters we cannot pass through".into());
    };
    let path = std::env::temp_dir().join(format!("parzi-askpass-{}.bat", uuid::Uuid::new_v4()));
    std::fs::write(&path, format!("@echo off\r\necho({escaped}\r\n")).map_err(|e| e.to_string())?;
    Ok(AskpassGuard { path })
}

async fn ssh_exec(
    target: &str,
    remote_cmd: &str,
    batch_mode: bool,
    askpass: Option<&std::path::Path>,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let mut cmd = tokio::process::Command::new("ssh");
    #[cfg(windows)]
    cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
    cmd.args([
        "-o",
        "ConnectTimeout=12",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-o",
        "NumberOfPasswordPrompts=1",
    ]);
    if batch_mode {
        cmd.args(["-o", "BatchMode=yes"]);
    }
    if let Some(helper) = askpass {
        cmd.env("SSH_ASKPASS", helper);
        cmd.env("SSH_ASKPASS_REQUIRE", "force");
    }
    cmd.arg(target);
    cmd.arg(remote_cmd);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "no SSH client found — install OpenSSH".to_string()
        } else {
            format!("could not start ssh: {e}")
        }
    })?;
    // Dropped on timeout: tokio kills child processes on drop by default.
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) => Err(format!("ssh failed: {e}")),
        Err(_) => Err("ssh timed out".into()),
    }
}

fn tail_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take(300)
        .collect()
}

fn unreachable(stderr: &str) -> bool {
    let l = stderr.to_lowercase();
    [
        "could not resolve hostname",
        "name or service not known",
        "connection timed out",
        "operation timed out",
        "connection refused",
        "no route to host",
        "network is unreachable",
        "connection reset",
    ]
    .iter()
    .any(|m| l.contains(m))
}

fn ssh_missing(out: &Result<std::process::Output, String>) -> bool {
    matches!(out, Err(e) if e.contains("no SSH client found"))
}

#[tauri::command]
pub async fn remote_connect(
    user: String,
    host: String,
    password: Option<String>,
) -> Result<RemoteReport, String> {
    let user = clean(&user, "user")?;
    let host = clean(&host, "host")?;
    let target = format!("{user}@{host}");

    // 1. Keys first: fast, silent, nothing to type.
    let key = ssh_exec(&target, "true", true, None, SSH_TIMEOUT).await;
    if ssh_missing(&key) {
        return Err("no SSH client found — install OpenSSH".into());
    }
    // Passwords live in this scope only: never logged, never stored.
    // The askpass helper file is deleted by its guard on every exit.
    let pw = password.unwrap_or_default();
    let have_pw = !pw.is_empty();

    if let Ok(out) = &key {
        if out.status.success() {
            return finish_with_setup(&target, true, None).await;
        }
    }
    let key_err = key.map(|o| tail_text(&o.stderr)).unwrap_or_default();
    if unreachable(&key_err) {
        return Ok(RemoteReport {
            connected: false,
            method: String::new(),
            prepared: false,
            detail: format!("{host} is not reachable ({key_err})"),
        });
    }

    // 2. Password auth, only when one was given.
    if !have_pw {
        return Ok(RemoteReport {
            connected: false,
            method: String::new(),
            prepared: false,
            detail: format!("{target} refused the key ({key_err}). Add the password if this server needs one."),
        });
    }
    let askpass = write_askpass(&pw)?;
    let attempt = ssh_exec(&target, "true", false, Some(&askpass.path), SSH_TIMEOUT).await;
    match attempt {
        Ok(out) if out.status.success() => finish_with_setup(&target, false, Some(&askpass.path)).await,
        Ok(out) => Ok(RemoteReport {
            connected: false,
            method: String::new(),
            prepared: false,
            detail: format!("{target} refused that password ({})", tail_text(&out.stderr)),
        }),
        Err(e) => Ok(RemoteReport {
            connected: false,
            method: String::new(),
            prepared: false,
            detail: format!("{target}: {e}"),
        }),
    }
}

async fn finish_with_setup(
    target: &str,
    via_key: bool,
    askpass: Option<&std::path::Path>,
) -> Result<RemoteReport, String> {
    let method = if via_key { "key" } else { "password" }.to_string();
    // `parzi setup` prepares ~/.parzi and returns; the engine itself
    // (`parzi serve`) blocks, so starting it stays a manual step.
    let setup = ssh_exec(target, "parzi setup", via_key, askpass, SETUP_TIMEOUT).await;
    match setup {
        Ok(out) if out.status.success() => {
            let detail = format!("Connected to {target} via {method}; the remote is ready.");
            Ok(RemoteReport { connected: true, method, prepared: true, detail })
        }
        Ok(out) => {
            let err = tail_text(&out.stderr);
            let missing = err.contains("command not found") || err.contains("not recognized");
            let detail = if missing {
                format!("Connected to {target} via {method}, but no parzi CLI is on its PATH — install it there first.")
            } else {
                format!("Connected to {target} via {method}, but setup failed ({err})")
            };
            Ok(RemoteReport { connected: true, method, prepared: false, detail })
        }
        Err(e) => {
            let detail = format!("Connected to {target} via {method}, but setup failed ({e})");
            Ok(RemoteReport { connected: true, method, prepared: false, detail })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_and_host_must_be_single_words() {
        assert!(clean("claw", "user").is_ok());
        assert!(clean("  ", "user").is_err());
        assert!(clean("a b", "host").is_err());
        assert!(clean("a\nb", "host").is_err());
    }

    #[test]
    fn batch_escape_quotes_shell_metachars() {
        assert_eq!(batch_escape("p@ss w0rd!").unwrap(), "p@ss w0rd!");
        assert_eq!(batch_escape("100%").unwrap(), "100%%");
        assert_eq!(batch_escape("a&b|c<d>e^f\"g").unwrap(), "a^&b^|c^<d^>e^^f^\"g");
        assert!(batch_escape("multi\nline").is_none());
    }

    #[test]
    fn network_failures_classify_as_unreachable() {
        assert!(unreachable("ssh: Could not resolve hostname gpu-box"));
        assert!(unreachable("connect to host 1.2.3.4 port 22: Connection refused"));
        assert!(!unreachable("claw@h: Permission denied (publickey,password)."));
    }
}
