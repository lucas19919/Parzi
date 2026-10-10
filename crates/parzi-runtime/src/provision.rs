//! ParziOS on the server: apply the spec the desktop sends, install the
//! user service, keep `parzi serve` answering, and relay `parzi rpc`.
//!
//! `parzi rpc` is the only door a remote client uses. It runs on the
//! server as the SSH user, reads the 0600 `serve.json` itself and relays
//! newline JSON between its stdio and the loopback socket, so SSH is the
//! authentication and the serve token never leaves the machine.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parzi_core::brain::Vault;
use parzi_core::config::ParziConfig;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::desk;

pub const SPEC_VERSION: u32 = 1;
pub const SERVICE_NAME: &str = "parzi-os.service";
const MAX_NOTES: usize = 2_000;
const MAX_NOTE_BYTES: usize = 256 * 1024;
const MAX_RPC_LINE: usize = 1024 * 1024;

/// What the desktop sends once at setup. Connectors and agent program
/// paths are per machine and never travel; neither do secrets.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Spec {
    pub version: u32,
    #[serde(default)]
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<ParziConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<SpecNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecNote {
    pub path: String,
    pub text: String,
}

/// One line of setup progress. `--json` prints these one per line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub step: String,
    pub ok: bool,
    pub detail: String,
}

impl Step {
    pub fn ok(step: &str, detail: impl Into<String>) -> Self {
        Self {
            step: step.into(),
            ok: true,
            detail: detail.into(),
        }
    }

    pub fn fail(step: &str, detail: impl Into<String>) -> Self {
        Self {
            step: step.into(),
            ok: false,
            detail: detail.into(),
        }
    }
}

/// The portable part of a config: what a fresh machine should inherit.
#[must_use]
pub fn portable(mut cfg: ParziConfig) -> ParziConfig {
    cfg.mcp = parzi_core::config::McpConfig::default();
    for entry in cfg.providers.values_mut() {
        entry.binary.clear();
    }
    cfg
}

/// The incoming config wins, except for what belongs to this machine:
/// its connectors and the agent program paths it already found.
#[must_use]
pub fn merge_config(local: ParziConfig, incoming: ParziConfig) -> ParziConfig {
    let mut out = portable(incoming);
    out.version = parzi_core::config::CONFIG_VERSION;
    out.mcp = local.mcp;
    for (id, mine) in local.providers {
        let entry = out.providers.entry(id).or_insert_with(|| mine.clone());
        entry.binary = mine.binary;
    }
    out
}

/// Apply a spec. Notes are only added: a note that already exists here was
/// written here, and setup never overwrites it.
pub fn apply_spec(spec: &Spec) -> Vec<Step> {
    if spec.version != SPEC_VERSION {
        return vec![Step::fail(
            "spec",
            format!(
                "spec version {} is not {SPEC_VERSION}: update Parzi on both machines",
                spec.version
            ),
        )];
    }
    let mut out = vec![];
    if let Some(incoming) = spec.config.clone() {
        let local = ParziConfig::load().unwrap_or_default();
        out.push(match merge_config(local, incoming).save() {
            Ok(()) => Step::ok("config", "settings copied; connectors stay per machine"),
            Err(e) => Step::fail("config", format!("could not write the config: {e}")),
        });
    }
    if !spec.notes.is_empty() {
        out.push(match restore_notes(&spec.notes) {
            Ok((added, kept, skipped)) => {
                let mut detail = format!("{added} notes added");
                if kept > 0 {
                    detail.push_str(&format!(", {kept} already here and kept"));
                }
                if skipped > 0 {
                    detail.push_str(&format!(", {skipped} skipped (bad path or over 256 KB)"));
                }
                Step::ok("brain", detail)
            }
            Err(e) => Step::fail("brain", e),
        });
    }
    out
}

fn restore_notes(notes: &[SpecNote]) -> Result<(usize, usize, usize), String> {
    let vault = Vault::open().map_err(|e| e.to_string())?;
    let (mut added, mut kept, mut skipped) = (0, 0, 0);
    for note in notes.iter().take(MAX_NOTES) {
        let full = match vault.resolve(&note.path) {
            Ok(full) if full != vault.root() && note.text.len() <= MAX_NOTE_BYTES => full,
            _ => {
                skipped += 1;
                continue;
            }
        };
        if full.exists() {
            kept += 1;
            continue;
        }
        parzi_core::atomic_write(&full, note.text.as_bytes())
            .map_err(|e| format!("could not write {}: {e}", note.path))?;
        added += 1;
    }
    Ok((added, kept, skipped + notes.len().saturating_sub(MAX_NOTES)))
}

/// `~/.config/systemd/user/parzi-os.service`, honouring `XDG_CONFIG_HOME`.
#[must_use]
pub fn unit_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs_home().map(|h| h.join(".config")))?;
    Some(base.join("systemd").join("user").join(SERVICE_NAME))
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

async fn quiet(program: &str, args: &[&str], wait: Duration) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn().map_err(|e| format!("{program}: {e}"))?;
    let out = tokio::time::timeout(wait, child.wait_with_output())
        .await
        .map_err(|_| format!("{program} {} timed out", args.join(" ")))?
        .map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        Err(format!("{program} {}: {}", args.join(" "), line.trim()))
    }
}

async fn systemctl(args: &[&str]) -> Result<String, String> {
    let mut all = vec!["--user"];
    all.extend_from_slice(args);
    quiet("systemctl", &all, Duration::from_secs(20)).await
}

/// True when this login has a systemd user manager to talk to.
pub async fn systemd_user() -> bool {
    cfg!(target_os = "linux") && systemctl(&["show-environment"]).await.is_ok()
}

/// Write and enable the unit so the engine starts with the user's login.
/// [`ensure_serving`] starts it. Without a user manager (containers, WSL
/// without systemd) there is nothing to install: `parzi rpc` starts the
/// engine detached whenever it is down.
pub async fn install_service(exe: &Path) -> Vec<Step> {
    if !systemd_user().await {
        return vec![Step::ok(
            "service",
            "no systemd user manager here, so the engine runs on its own and `parzi rpc` restarts it when it is down",
        )];
    }
    let Some(path) = unit_path() else {
        return vec![Step::fail("service", "no home directory")];
    };
    let unit = crate::osserve::systemd_unit(exe);
    if let Err(e) = parzi_core::atomic_write(&path, unit.as_bytes()) {
        return vec![Step::fail(
            "service",
            format!("could not write {}: {e}", path.display()),
        )];
    }
    let mut out = vec![];
    let enabled = async {
        systemctl(&["daemon-reload"]).await?;
        systemctl(&["enable", SERVICE_NAME]).await
    }
    .await;
    out.push(match enabled {
        Ok(_) => Step::ok("service", format!("{SERVICE_NAME} installed and enabled")),
        Err(e) => Step::fail("service", e),
    });
    if !lingering().await {
        let user = std::env::var("USER").unwrap_or_else(|_| "$USER".into());
        out.push(Step::ok(
            "linger",
            format!("the engine stops when you log out of this machine. To keep it running, run once: sudo loginctl enable-linger {user}"),
        ));
    }
    out
}

async fn lingering() -> bool {
    let user = std::env::var("USER").unwrap_or_default();
    if user.is_empty() {
        return false;
    }
    quiet(
        "loginctl",
        &["show-user", &user, "--property=Linger"],
        Duration::from_secs(5),
    )
    .await
    .is_ok_and(|s| s.trim() == "Linger=yes")
}

/// Start `parzi serve` in its own process group with output in the log
/// folder, so it outlives the SSH session that started it.
pub fn spawn_detached(exe: &Path) -> Result<(), String> {
    let root = parzi_core::paths::ensure_dirs().map_err(|e| e.to_string())?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("logs").join("serve.log"))
        .map_err(|e| format!("serve log: {e}"))?;
    let err = log.try_clone().map_err(|e| format!("serve log: {e}"))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(log)
        .stderr(err);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn()
        .map(drop)
        .map_err(|e| format!("could not start parzi serve: {e}"))
}

/// The running engine's version, if it answers.
pub async fn serving_version() -> Option<String> {
    let v = desk::call_serve("health", json!({}), Duration::from_secs(3))
        .await
        .ok()?;
    Some(v.get("version")?.as_str()?.to_string())
}

/// Make sure an engine of this binary's version answers. An older one is
/// asked to stop first, so an upgrade takes effect without a reboot.
pub async fn ensure_serving(exe: &Path, wait: Duration) -> Result<String, String> {
    let ours = env!("CARGO_PKG_VERSION");
    match serving_version().await {
        Some(v) if v == ours => return Ok(v),
        Some(_) => {
            let _ = desk::call_serve("shutdown", json!({}), Duration::from_secs(3)).await;
            for _ in 0..30 {
                if serving_version().await.is_none() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        None => {}
    }
    let unit_installed = unit_path().is_some_and(|p| p.exists());
    if unit_installed && systemd_user().await {
        systemctl(&["restart", SERVICE_NAME]).await?;
    } else {
        spawn_detached(exe)?;
    }
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        if let Some(v) = serving_version().await {
            return Ok(v);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(
                "parzi serve did not answer. See ~/.parzi/logs/serve.log or `journalctl --user -u parzi-os`".into(),
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// How long a relayed op may take before the client hears a timeout.
fn op_wait(op: &str) -> Duration {
    match op {
        "providers" | "doctor" => Duration::from_secs(90),
        "session.send" | "session.kill" | "session.delete" => Duration::from_secs(60),
        _ => Duration::from_secs(20),
    }
}

/// Relay newline JSON between stdio and the local engine until stdin
/// closes. Requests carry an `id` that the matching reply echoes; replies
/// may arrive out of order.
pub async fn relay(exe: &Path) -> std::io::Result<()> {
    let out = Arc::new(tokio::sync::Mutex::new(tokio::io::stdout()));
    let hello = match ensure_serving(exe, Duration::from_secs(15)).await {
        Ok(v) => json!({ "rpc": "parzi", "version": env!("CARGO_PKG_VERSION"), "serving": v }),
        Err(e) => json!({ "rpc": "parzi", "version": env!("CARGO_PKG_VERSION"), "error": e }),
    };
    write_line(&out, &hello).await?;
    let mut input = BufReader::new(tokio::io::stdin());
    let mut buf = Vec::new();
    let mut inflight = tokio::task::JoinSet::new();
    loop {
        buf.clear();
        let n = read_line_capped(&mut input, &mut buf, MAX_RPC_LINE).await?;
        if n == 0 {
            break;
        }
        while inflight.try_join_next().is_some() {}
        let line = String::from_utf8_lossy(&buf).trim().to_string();
        if line.is_empty() {
            continue;
        }
        let out = out.clone();
        inflight.spawn(async move {
            let reply = relay_one(&line).await;
            let _ = write_line(&out, &reply).await;
        });
    }
    // stdin closed: answer what was already asked before exiting.
    while inflight.join_next().await.is_some() {}
    Ok(())
}

async fn relay_one(line: &str) -> Value {
    let Ok(Value::Object(mut req)) = serde_json::from_str::<Value>(line) else {
        return json!({ "ok": false, "error": "not a JSON object" });
    };
    let id = req.remove("id").unwrap_or(Value::Null);
    req.remove("token");
    let op = req
        .remove("op")
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let mut reply = match desk::call_serve(&op, Value::Object(req), op_wait(&op)).await {
        Ok(v) => v,
        Err(e) => json!({ "ok": false, "error": e }),
    };
    reply["id"] = id;
    reply
}

async fn write_line(out: &tokio::sync::Mutex<tokio::io::Stdout>, v: &Value) -> std::io::Result<()> {
    let mut line = serde_json::to_string(v).unwrap_or_else(|_| "{}".into());
    line.push('\n');
    let mut out = out.lock().await;
    out.write_all(line.as_bytes()).await?;
    out.flush().await
}

/// `read_until(b'\n')` with a ceiling: a line longer than `cap` is read to
/// its end and dropped, so one bad line cannot grow memory without bound.
pub async fn read_line_capped<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    cap: usize,
) -> std::io::Result<usize> {
    let mut total = 0;
    let mut overflow = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(total);
        }
        let (take, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if !overflow && buf.len() + take <= cap {
            buf.extend_from_slice(&chunk[..take]);
        } else {
            overflow = true;
            buf.clear();
        }
        reader.consume(take);
        total += take;
        if done {
            return Ok(total);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parzi_core::config::{McpServerCfg, ProviderEntry};

    #[test]
    fn connectors_and_program_paths_stay_on_their_machine() {
        let mut desktop = ParziConfig::default();
        desktop.favorite_models = vec!["claude/opus".into()];
        desktop.providers.insert(
            "claude".into(),
            ProviderEntry {
                binary: r"C:\tools\claude.exe".into(),
                default_model: "opus".into(),
                ..Default::default()
            },
        );
        desktop.mcp.servers.insert(
            "mail".into(),
            McpServerCfg {
                command: "mail-mcp".into(),
                ..Default::default()
            },
        );
        let sent = portable(desktop);
        assert!(sent.mcp.servers.is_empty());
        assert!(sent.providers["claude"].binary.is_empty());

        let mut server = ParziConfig::default();
        server.providers.insert(
            "claude".into(),
            ProviderEntry {
                binary: "/opt/claude".into(),
                ..Default::default()
            },
        );
        server.mcp.servers.insert(
            "db".into(),
            McpServerCfg {
                command: "pg-mcp".into(),
                ..Default::default()
            },
        );
        let merged = merge_config(server, sent);
        assert_eq!(merged.providers["claude"].binary, "/opt/claude");
        assert_eq!(merged.providers["claude"].default_model, "opus");
        assert_eq!(merged.favorite_models, vec!["claude/opus".to_string()]);
        assert!(merged.mcp.servers.contains_key("db"));
        assert!(!merged.mcp.servers.contains_key("mail"));
    }

    #[test]
    fn a_spec_from_another_version_is_refused() {
        let steps = apply_spec(&Spec {
            version: SPEC_VERSION + 1,
            ..Default::default()
        });
        assert_eq!(steps.len(), 1);
        assert!(!steps[0].ok);
    }

    #[tokio::test]
    async fn capped_lines_drop_the_long_one_and_keep_the_next() {
        let data = b"short\nthis line is far too long\nok\n".to_vec();
        let mut reader = BufReader::with_capacity(4, &data[..]);
        let mut buf = Vec::new();
        read_line_capped(&mut reader, &mut buf, 10).await.unwrap();
        assert_eq!(buf, b"short\n");
        buf.clear();
        let n = read_line_capped(&mut reader, &mut buf, 10).await.unwrap();
        assert!(n > 10);
        assert!(buf.is_empty());
        buf.clear();
        read_line_capped(&mut reader, &mut buf, 10).await.unwrap();
        assert_eq!(buf, b"ok\n");
        buf.clear();
        assert_eq!(
            read_line_capped(&mut reader, &mut buf, 10).await.unwrap(),
            0
        );
    }
}
