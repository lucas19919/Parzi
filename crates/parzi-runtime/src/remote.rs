//! The desktop's side of ParziOS: reach a Linux machine over the user's
//! own `ssh`, put Parzi there, and talk to its engine through `parzi rpc`.
//!
//! No port is ever opened. Every remote action is `ssh … sh -s` with the
//! script on stdin, so the remote login shell (bash, zsh or fish) only ever
//! parses `sh -s`. The live link is one long `ssh … parzi rpc` whose stdio
//! carries newline JSON.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use parzi_core::brain::Vault;
use parzi_core::config::ParziConfig;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::oneshot;

use crate::provision::{self, Spec, SpecNote, Step, SPEC_VERSION};

/// Where setup puts the binary. `~` works in every login shell; scripts
/// run by `sh -s` use `$HOME` instead.
pub const REMOTE_BIN: &str = "~/.local/bin/parzi";
pub const RELEASES: &str = "lucas19919/Parzi";
const CONNECT: Duration = Duration::from_secs(30);
const STEP: Duration = Duration::from_secs(120);
const SETUP_WAIT: Duration = Duration::from_secs(300);
const DOWNLOAD: Duration = Duration::from_secs(600);
const MAX_SPEC_NOTES_BYTES: usize = 8 * 1024 * 1024;
const MAX_REPLY: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub user: String,
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Parzi's own key, when setup added it to the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<PathBuf>,
}

impl Target {
    /// `host` may carry a port (`box:2222`). A user or host with spaces,
    /// control characters or a leading dash is refused: they reach `ssh`
    /// as arguments.
    pub fn parse(user: &str, host: &str) -> Result<Self, String> {
        let user = word(user, "user")?;
        let host = word(host, "host")?;
        let (host, port) = match host.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') && !p.is_empty() => {
                let port = p
                    .parse::<u16>()
                    .ok()
                    .filter(|p| *p > 0)
                    .ok_or_else(|| format!("`{p}` is not a port"))?;
                (h.to_string(), Some(port))
            }
            _ => (host, None),
        };
        if host.is_empty() {
            return Err("enter an SSH host".into());
        }
        Ok(Self {
            user,
            host,
            port,
            identity: None,
        })
    }

    #[must_use]
    pub fn label(&self) -> String {
        match self.port {
            Some(p) => format!("{}@{}:{p}", self.user, self.host),
            None => format!("{}@{}", self.user, self.host),
        }
    }

    fn destination(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }
}

fn word(field: &str, what: &str) -> Result<String, String> {
    let t = field.trim();
    if t.is_empty() {
        return Err(format!("enter an SSH {what}"));
    }
    if t.starts_with('-') || t.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("the {what} must be a single word"));
    }
    Ok(t.to_string())
}

/// The remote the desktop links to, kept in `~/.parzi/remote.json`. No
/// secret is ever in it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Saved {
    #[serde(flatten)]
    pub target: Target,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub linked_at: i64,
}

fn saved_path() -> Option<PathBuf> {
    parzi_core::paths::parzi_dir()
        .ok()
        .map(|d| d.join("remote.json"))
}

#[must_use]
pub fn load_saved() -> Option<Saved> {
    let raw = std::fs::read_to_string(saved_path()?).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn save(saved: &Saved) -> Result<(), String> {
    let path = saved_path().ok_or("no home directory")?;
    let body = serde_json::to_vec_pretty(saved).map_err(|e| e.to_string())?;
    parzi_core::atomic_write(&path, &body).map_err(|e| e.to_string())
}

pub fn forget() -> Result<(), String> {
    match saved_path() {
        Some(p) if p.exists() => std::fs::remove_file(p).map_err(|e| e.to_string()),
        _ => Ok(()),
    }
}

/// How a remote command is started: the program plus the arguments in
/// front of the remote command line, which is always one last argument.
#[derive(Debug, Clone)]
pub struct Transport {
    program: OsString,
    args: Vec<OsString>,
    envs: Vec<(OsString, OsString)>,
}

impl Transport {
    /// Key login only, never a prompt. This is what the live link uses.
    #[must_use]
    pub fn ssh(target: &Target) -> Self {
        Self::build(target, true)
    }

    fn build(target: &Target, batch: bool) -> Self {
        let mut args: Vec<OsString> = vec![];
        let mut opt = |o: &str| {
            args.push("-o".into());
            args.push(o.into());
        };
        opt("ConnectTimeout=12");
        opt("StrictHostKeyChecking=accept-new");
        opt("ServerAliveInterval=20");
        opt("ServerAliveCountMax=3");
        if batch {
            opt("BatchMode=yes");
        } else {
            opt("NumberOfPasswordPrompts=1");
            opt("PreferredAuthentications=password,keyboard-interactive");
            opt("PubkeyAuthentication=no");
        }
        if let Some(key) = &target.identity {
            args.push("-i".into());
            args.push(key.clone().into_os_string());
        }
        if let Some(port) = target.port {
            args.push("-p".into());
            args.push(port.to_string().into());
        }
        args.push("-T".into());
        args.push(target.destination().into());
        Self {
            program: "ssh".into(),
            args,
            envs: vec![],
        }
    }

    fn with_password(target: &Target, askpass: &Askpass) -> Self {
        let mut t = Self::build(target, false);
        t.envs
            .push(("SSH_ASKPASS".into(), askpass.path.clone().into()));
        t.envs.push(("SSH_ASKPASS_REQUIRE".into(), "force".into()));
        t.envs.push(("DISPLAY".into(), "parzi:0".into()));
        t
    }

    /// Any program that runs its last argument as a shell command line on
    /// the other side, e.g. `wsl.exe -d Ubuntu -e sh -c`.
    #[must_use]
    pub fn custom(program: impl Into<OsString>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(OsString::from).collect(),
            envs: vec![],
        }
    }

    fn command(&self, remote: &str) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(&self.args).arg(remote);
        for (k, v) in &self.envs {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
        cmd.kill_on_drop(true);
        cmd
    }

    /// Run a POSIX script on the remote with `sh -s`.
    pub async fn script(&self, script: &str, wait: Duration) -> Result<Ran, String> {
        let mut lines = vec![];
        let ran = self
            .script_lines(script, wait, |l| lines.push(l.to_string()))
            .await?;
        Ok(Ran {
            stdout: lines.join("\n"),
            ..ran
        })
    }

    /// Like [`Transport::script`], handing each stdout line over as it arrives.
    pub async fn script_lines(
        &self,
        script: &str,
        wait: Duration,
        mut on_line: impl FnMut(&str),
    ) -> Result<Ran, String> {
        let mut cmd = self.command("sh -s");
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(spawn_error)?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let body = script.as_bytes().to_vec();
        let feed = tokio::spawn(async move {
            let _ = stdin.write_all(&body).await;
            let _ = stdin.shutdown().await;
        });
        let err = tokio::spawn(read_tail(stderr));
        let work = async {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                let n = provision::read_line_capped(&mut reader, &mut buf, 1024 * 1024)
                    .await
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                on_line(String::from_utf8_lossy(&buf).trim_end());
            }
            child.wait().await.map_err(|e| e.to_string())
        };
        let status = tokio::time::timeout(wait, work)
            .await
            .map_err(|_| "the remote did not finish in time".to_string())??;
        feed.abort();
        let stderr = err.await.unwrap_or_default();
        Ok(Ran {
            ok: status.success(),
            code: status.code(),
            stdout: String::new(),
            stderr,
        })
    }

    /// The command line for a visible terminal: `ssh -t … target 'cmd'`.
    /// Returns None for a custom transport.
    #[must_use]
    pub fn terminal_args(&self, remote: &str) -> Option<Vec<String>> {
        if self.program != "ssh" {
            return None;
        }
        let mut out: Vec<String> = vec!["ssh".into(), "-t".into()];
        for a in &self.args {
            let a = a.to_string_lossy().to_string();
            if a != "-T" && a != "BatchMode=yes" {
                out.push(a);
            } else if a == "BatchMode=yes" {
                out.pop();
            }
        }
        out.push(remote.to_string());
        Some(out)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Ran {
    pub ok: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Ran {
    /// The last useful stderr line, or the stdout `PARZI-ERR` line.
    #[must_use]
    pub fn why(&self) -> String {
        if let Some(line) = self
            .stdout
            .lines()
            .find_map(|l| l.strip_prefix("PARZI-ERR "))
        {
            return line.trim().to_string();
        }
        last_line(&self.stderr)
    }
}

fn last_line(text: &str) -> String {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .chars()
        .take(300)
        .collect()
}

fn spawn_error(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "no SSH client found. Install OpenSSH".into()
    } else {
        format!("could not start ssh: {e}")
    }
}

async fn read_tail(mut r: impl tokio::io::AsyncRead + Unpin) -> String {
    let mut all = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                all.extend_from_slice(&buf[..n]);
                if all.len() > 16 * 1024 {
                    all.drain(..all.len() - 8 * 1024);
                }
            }
        }
    }
    String::from_utf8_lossy(&all).to_string()
}

#[must_use]
pub fn unreachable(stderr: &str) -> bool {
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

/// A one-line askpass program holding the password, deleted on drop.
/// The password never reaches a log, a setting or the saved remote.
struct Askpass {
    path: PathBuf,
}

impl Drop for Askpass {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn askpass_dir() -> Result<PathBuf, String> {
    Ok(parzi_core::paths::parzi_dir()
        .map_err(|e| e.to_string())?
        .join("tmp"))
}

/// Remove password helpers a crash left behind. Setup only ever runs one
/// at a time, so any found here are stale.
pub fn sweep_askpass() {
    let Ok(dir) = askpass_dir() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with("askpass-") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

impl Askpass {
    fn write(password: &str) -> Result<Self, String> {
        if password
            .chars()
            .any(|c| c == '\r' || c == '\n' || c == '\0')
        {
            return Err("that password has characters ssh cannot take".into());
        }
        sweep_askpass();
        let dir = askpass_dir()?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        if cfg!(windows) {
            let path = dir.join(format!("askpass-{id}.bat"));
            // Delayed expansion would eat `!` in the password.
            let body = format!(
                "@echo off\r\nsetlocal DisableDelayedExpansion\r\necho({}\r\n",
                batch_escape(password)
            );
            std::fs::write(&path, body).map_err(|e| e.to_string())?;
            Ok(Self { path })
        } else {
            let path = dir.join(format!("askpass-{id}.sh"));
            let body = format!("#!/bin/sh\nprintf '%s\\n' {}\n", sh_quote(password));
            write_private(&path, body.as_bytes(), 0o700)?;
            Ok(Self { path })
        }
    }
}

fn write_private(path: &Path, bytes: &[u8], _mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(_mode)
            .open(path)
            .map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }
}

/// Escape for `echo(` in a batch file.
fn batch_escape(password: &str) -> String {
    let mut out = String::with_capacity(password.len() + 4);
    for c in password.chars() {
        match c {
            '%' => out.push_str("%%"),
            '&' | '|' | '<' | '>' | '^' | '"' | '(' | ')' => {
                out.push('^');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Single-quote for POSIX sh.
#[must_use]
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Parzi's own SSH key, made once with `ssh-keygen` and no passphrase.
fn parzi_key() -> Result<PathBuf, String> {
    Ok(parzi_core::paths::parzi_dir()
        .map_err(|e| e.to_string())?
        .join("ssh")
        .join("id_ed25519"))
}

async fn ensure_key() -> Result<(PathBuf, String), String> {
    let key = parzi_key()?;
    let public = key.with_extension("pub");
    if !key.exists() || !public.exists() {
        if let Some(dir) = key.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let host = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "desktop".into());
        let comment = format!(
            "parzi@{}",
            host.chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
        );
        let mut cmd = tokio::process::Command::new("ssh-keygen");
        cmd.args(["-q", "-t", "ed25519", "-N", "", "-C", &comment, "-f"])
            .arg(&key)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
        let out = cmd
            .output()
            .await
            .map_err(|e| format!("could not run ssh-keygen: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "ssh-keygen failed: {}",
                last_line(&String::from_utf8_lossy(&out.stderr))
            ));
        }
    }
    let line = std::fs::read_to_string(&public)
        .map_err(|e| format!("could not read {}: {e}", public.display()))?
        .trim()
        .to_string();
    let safe = line.split_whitespace().count() >= 2
        && line
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " +/=@._:-".contains(c));
    if !safe {
        return Err(format!(
            "{} does not look like a public key",
            public.display()
        ));
    }
    Ok((key, line))
}

fn authorize_script(public: &str) -> String {
    let key = sh_quote(public);
    format!(
        "umask 077\n\
         mkdir -p \"$HOME/.ssh\" && chmod 700 \"$HOME/.ssh\"\n\
         touch \"$HOME/.ssh/authorized_keys\" && chmod 600 \"$HOME/.ssh/authorized_keys\"\n\
         grep -qxF {key} \"$HOME/.ssh/authorized_keys\" || printf '%s\\n' {key} >> \"$HOME/.ssh/authorized_keys\"\n"
    )
}

const PROBE: &str = r#"echo "os=$(uname -s)"
echo "arch=$(uname -m)"
if command -v curl >/dev/null 2>&1; then echo fetch=curl; elif command -v wget >/dev/null 2>&1; then echo fetch=wget; else echo fetch=none; fi
if systemctl --user show-environment >/dev/null 2>&1; then echo systemd=yes; else echo systemd=no; fi
echo "have=$("$HOME/.local/bin/parzi" --version 2>/dev/null)"
"#;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Probe {
    pub os: String,
    pub arch: String,
    pub fetch: String,
    pub systemd: bool,
    pub have: String,
}

impl Probe {
    fn parse(out: &str) -> Self {
        let mut p = Self::default();
        for line in out.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim().to_string();
            match k.trim() {
                "os" => p.os = v,
                "arch" => p.arch = v,
                "fetch" => p.fetch = v,
                "systemd" => p.systemd = v == "yes",
                "have" => p.have = v,
                _ => {}
            }
        }
        p
    }

    /// The release asset for this machine, or why there is none.
    pub fn asset(&self) -> Result<&'static str, String> {
        match (self.os.as_str(), self.arch.as_str()) {
            ("Linux", "x86_64" | "amd64") => Ok("parzi-linux-x64"),
            ("Linux", "aarch64" | "arm64") => Ok("parzi-linux-arm64"),
            ("Linux", other) => Err(format!("ParziOS has no build for {other} yet")),
            (os, _) => Err(format!("ParziOS runs on Linux; this machine is {os}")),
        }
    }
}

fn download_script(repo: &str, version: &str, asset: &str, fetch: &str) -> String {
    let url = format!("https://github.com/{repo}/releases/download/v{version}/{asset}");
    let get = if fetch == "wget" {
        "wget -qO \"$2\" \"$1\""
    } else {
        "curl -fsSL \"$1\" -o \"$2\""
    };
    format!(
        "set -u\n\
         dir=\"$HOME/.local/bin\"; mkdir -p \"$dir\"\n\
         tmp=\"$dir/.parzi-download.$$\"\n\
         fetch() {{ {get}; }}\n\
         if ! fetch {url} \"$tmp\"; then rm -f \"$tmp\"; echo 'PARZI-ERR Parzi {version} has no Linux build on GitHub yet'; exit 4; fi\n\
         if ! fetch {url}.sha256 \"$tmp.sha256\"; then rm -f \"$tmp\" \"$tmp.sha256\"; echo 'PARZI-ERR the checksum for Parzi {version} is missing'; exit 4; fi\n\
         want=$(cut -d' ' -f1 \"$tmp.sha256\"); got=$(sha256sum \"$tmp\" | cut -d' ' -f1)\n\
         rm -f \"$tmp.sha256\"\n\
         if [ \"$want\" != \"$got\" ]; then rm -f \"$tmp\"; echo 'PARZI-ERR the download does not match its checksum'; exit 5; fi\n\
         chmod 755 \"$tmp\" && mv -f \"$tmp\" \"$dir/parzi\"\n\
         \"$dir/parzi\" --version\n",
        url = sh_quote(&url),
    )
}

/// Upload a local Linux build through the script itself, base64 in a
/// here-document, so no second channel (scp) is needed.
fn upload_script(binary: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(binary);
    let mut wrapped = String::with_capacity(b64.len() + b64.len() / 76 + 1);
    for chunk in b64.as_bytes().chunks(76) {
        wrapped.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        wrapped.push('\n');
    }
    format!(
        "set -u\n\
         dir=\"$HOME/.local/bin\"; mkdir -p \"$dir\"\n\
         tmp=\"$dir/.parzi-upload.$$\"\n\
         base64 -d > \"$tmp\" <<'PARZI_BINARY_END'\n{wrapped}PARZI_BINARY_END\n\
         chmod 755 \"$tmp\" && mv -f \"$tmp\" \"$dir/parzi\"\n\
         \"$dir/parzi\" --version\n"
    )
}

fn spec_script(spec_json: &str) -> String {
    let end = format!("PARZI_SPEC_{}", uuid::Uuid::new_v4().simple());
    format!(
        "umask 077\n\
         mkdir -p \"$HOME/.parzi\"\n\
         cat > \"$HOME/.parzi/setup-spec.json\" <<'{end}'\n{spec_json}\n{end}\n"
    )
}

const SETUP: &str =
    "\"$HOME/.local/bin/parzi\" setup --spec \"$HOME/.parzi/setup-spec.json\" --enable --json\n";
/// An upgrade keeps the server's own settings: no spec.
const UPGRADE: &str = "\"$HOME/.local/bin/parzi\" setup --enable --json\n";

/// What travels: the portable config, plus brain notes when asked.
/// Project notes stay home: they point at folders on this machine.
#[must_use]
pub fn local_spec(with_notes: bool) -> Spec {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    let config = ParziConfig::load().ok().map(provision::portable);
    let mut notes = vec![];
    if with_notes {
        if let Ok(vault) = Vault::open() {
            let (all, _) = vault.catalog();
            let mut bytes = 0;
            for meta in all {
                if meta.folder.is_some() || meta.archived {
                    continue;
                }
                let Ok(text) = vault.read(&meta.path) else {
                    continue;
                };
                bytes += text.len();
                if bytes > MAX_SPEC_NOTES_BYTES {
                    break;
                }
                notes.push(SpecNote {
                    path: meta.path,
                    text,
                });
            }
        }
    }
    Spec {
        version: SPEC_VERSION,
        from: format!("Parzi {} on {host}", env!("CARGO_PKG_VERSION")),
        config,
        notes,
    }
}

/// One progress line of a remote setup, for the wizard.
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub step: String,
    /// "run", "ok", "fail" or "note".
    pub state: &'static str,
    pub detail: String,
}

impl Progress {
    fn new(step: &str, state: &'static str, detail: impl Into<String>) -> Self {
        Self {
            step: step.into(),
            state,
            detail: detail.into(),
        }
    }
}

pub struct SetupOptions {
    pub password: Option<String>,
    pub notes: bool,
    /// A local Linux build to upload instead of downloading the release.
    pub binary: Option<PathBuf>,
    pub version: String,
    pub repo: String,
    /// Replaces ssh, for tests and other transports.
    pub transport: Option<Transport>,
}

impl Default for SetupOptions {
    fn default() -> Self {
        Self {
            password: None,
            notes: true,
            binary: None,
            version: env!("CARGO_PKG_VERSION").into(),
            repo: RELEASES.into(),
            transport: None,
        }
    }
}

/// Connect, install Parzi, hand it the spec, start the engine, link.
/// Each step reports through `on`; the first failure ends setup with its
/// reason.
pub async fn setup(
    target: Target,
    opts: SetupOptions,
    on: &(dyn Fn(Progress) + Send + Sync),
) -> Result<(Saved, Link), String> {
    let fail = |step: &str, why: String| {
        on(Progress::new(step, "fail", why.clone()));
        Err::<(Saved, Link), String>(why)
    };
    let mut target = target;
    let label = target.label();

    on(Progress::new("connect", "run", format!("Reaching {label}")));
    let mut ssh = opts
        .transport
        .clone()
        .unwrap_or_else(|| Transport::ssh(&target));
    let first = ssh.script("exit 0\n", CONNECT).await;
    let first = match first {
        Err(e) => return fail("connect", e),
        Ok(r) => r,
    };
    if first.ok {
        on(Progress::new(
            "connect",
            "ok",
            format!("{label} accepted your key"),
        ));
    } else {
        let why = first.why();
        if unreachable(&why) {
            return fail("connect", format!("{label} is not reachable ({why})"));
        }
        let Some(pw) = opts.password.as_deref().filter(|p| !p.is_empty()) else {
            return fail(
                "connect",
                format!(
                    "{label} refused the key ({why}). Add the password if this server takes one."
                ),
            );
        };
        let askpass = match Askpass::write(pw) {
            Ok(a) => a,
            Err(e) => return fail("connect", e),
        };
        let with_pw = Transport::with_password(&target, &askpass);
        match with_pw.script("exit 0\n", CONNECT).await {
            Ok(r) if r.ok => {}
            Ok(r) => {
                return fail(
                    "connect",
                    format!("{label} refused that password ({})", r.why()),
                )
            }
            Err(e) => return fail("connect", e),
        }
        on(Progress::new(
            "connect",
            "ok",
            format!("{label} accepted the password"),
        ));

        on(Progress::new(
            "key",
            "run",
            "Adding Parzi's key so it can reconnect without the password",
        ));
        let (key, public) = match ensure_key().await {
            Ok(k) => k,
            Err(e) => return fail("key", e),
        };
        match with_pw.script(&authorize_script(&public), CONNECT).await {
            Ok(r) if r.ok => {}
            Ok(r) => return fail("key", format!("could not add the key ({})", r.why())),
            Err(e) => return fail("key", e),
        }
        drop(askpass);
        target.identity = Some(key);
        ssh = Transport::ssh(&target);
        match ssh.script("exit 0\n", CONNECT).await {
            Ok(r) if r.ok => on(Progress::new("key", "ok", "key login works")),
            Ok(r) => {
                return fail(
                    "key",
                    format!(
                        "the server does not take key logins ({}). ParziOS needs one.",
                        r.why()
                    ),
                )
            }
            Err(e) => return fail("key", e),
        }
    }

    provision_remote(&ssh, &opts, true, on).await?;

    on(Progress::new("link", "run", "Linking this desktop"));
    let link = match Link::open(&ssh).await {
        Ok(l) => l,
        Err(e) => return fail("link", e),
    };
    let version = match link
        .call("health", json!({}), Duration::from_secs(10))
        .await
    {
        Ok(v) => v
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        Err(e) => return fail("link", e),
    };
    let saved = Saved {
        target,
        version: version.clone(),
        linked_at: chrono_now(),
    };
    if opts.transport.is_none() {
        if let Err(e) = save(&saved) {
            return fail("link", e);
        }
    }
    on(Progress::new(
        "link",
        "ok",
        format!("linked to Parzi {version} on {label}"),
    ));
    Ok((saved, link))
}

/// Probe the machine, install the matching build, send the spec when asked,
/// and run `parzi setup --enable` there. Each failure is reported via `on`.
async fn provision_remote(
    ssh: &Transport,
    opts: &SetupOptions,
    with_spec: bool,
    on: &(dyn Fn(Progress) + Send + Sync),
) -> Result<(), String> {
    let fail = |step: &str, why: String| {
        on(Progress::new(step, "fail", why.clone()));
        Err::<(), String>(why)
    };
    on(Progress::new("probe", "run", "Looking at the machine"));
    let probe = match ssh.script(PROBE, CONNECT).await {
        Ok(r) if r.ok => Probe::parse(&r.stdout),
        Ok(r) => return fail("probe", r.why()),
        Err(e) => return fail("probe", e),
    };
    let asset = match probe.asset() {
        Ok(a) => a,
        Err(e) => return fail("probe", e),
    };
    on(Progress::new(
        "probe",
        "ok",
        format!(
            "{} {}{}",
            probe.os,
            probe.arch,
            if probe.systemd {
                ", systemd"
            } else {
                ", no systemd user manager"
            }
        ),
    ));

    let want = format!("parzi {}", opts.version);
    on(Progress::new(
        "install",
        "run",
        format!("Putting Parzi {} there", opts.version),
    ));
    if probe.have.trim() == want && opts.binary.is_none() {
        on(Progress::new(
            "install",
            "ok",
            format!("Parzi {} is already there", opts.version),
        ));
    } else {
        let script = if let Some(path) = &opts.binary {
            match std::fs::read(path) {
                Ok(bytes) => upload_script(&bytes),
                Err(e) => {
                    return fail("install", format!("could not read {}: {e}", path.display()))
                }
            }
        } else {
            if probe.fetch == "none" {
                return fail("install", "the machine has neither curl nor wget".into());
            }
            download_script(&opts.repo, &opts.version, asset, &probe.fetch)
        };
        match ssh.script(&script, DOWNLOAD).await {
            Ok(r) if r.ok => on(Progress::new("install", "ok", last_line(&r.stdout))),
            Ok(r) => return fail("install", r.why()),
            Err(e) => return fail("install", e),
        }
    }

    if with_spec {
        on(Progress::new(
            "spec",
            "run",
            if opts.notes {
                "Sending your settings and brain notes"
            } else {
                "Sending your settings"
            },
        ));
        let spec = local_spec(opts.notes);
        let count = spec.notes.len();
        let body = match serde_json::to_string(&spec) {
            Ok(b) => b,
            Err(e) => return fail("spec", e.to_string()),
        };
        match ssh.script(&spec_script(&body), STEP).await {
            Ok(r) if r.ok => on(Progress::new(
                "spec",
                "ok",
                if opts.notes {
                    format!("settings and {count} notes sent")
                } else {
                    "settings sent".into()
                },
            )),
            Ok(r) => return fail("spec", r.why()),
            Err(e) => return fail("spec", e),
        }
    }

    on(Progress::new("setup", "run", "Setting Parzi up there"));
    let mut failed: Option<String> = None;
    let ran = ssh
        .script_lines(
            if with_spec { SETUP } else { UPGRADE },
            SETUP_WAIT,
            |line| {
                let Ok(step) = serde_json::from_str::<Step>(line) else {
                    return;
                };
                // The desk already reported its own spec and home rows.
                if step.step == "done" || (step.ok && (step.step == "spec" || step.step == "home"))
                {
                    return;
                }
                if !step.ok && failed.is_none() {
                    failed = Some(format!("{}: {}", step.step, step.detail));
                }
                on(Progress::new(
                    &step.step,
                    if step.ok { "ok" } else { "fail" },
                    step.detail,
                ));
            },
        )
        .await;
    match ran {
        Ok(r) if r.ok => on(Progress::new("setup", "ok", "the engine is running")),
        Ok(r) => return fail("setup", failed.unwrap_or_else(|| r.why())),
        Err(e) => return fail("setup", e),
    }
    Ok(())
}

/// Bring the linked server to this desk's version: install the matching
/// build and restart its engine, keeping the server's own settings.
pub async fn upgrade(
    saved: &Saved,
    on: &(dyn Fn(Progress) + Send + Sync),
) -> Result<Saved, String> {
    let ssh = Transport::ssh(&saved.target);
    provision_remote(&ssh, &SetupOptions::default(), false, on).await?;
    let next = Saved {
        version: env!("CARGO_PKG_VERSION").into(),
        ..saved.clone()
    };
    save(&next)?;
    Ok(next)
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

type Waiters = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;
type EventSink = Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Value>>>>;

/// One live `parzi rpc` over ssh. Calls are matched to replies by id; a
/// dead link fails every waiting call and reports `alive() == false`.
pub struct Link {
    stdin: tokio::sync::Mutex<ChildStdin>,
    waiters: Waiters,
    next: AtomicU64,
    alive: Arc<AtomicBool>,
    events: EventSink,
    stderr: Arc<Mutex<String>>,
    _child: Child,
    reader: tokio::task::JoinHandle<()>,
    pub version: String,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl Link {
    pub async fn open(transport: &Transport) -> Result<Self, String> {
        let mut cmd = transport.command(&format!("{REMOTE_BIN} rpc"));
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(spawn_error)?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr_pipe = child.stderr.take().ok_or("no stderr")?;
        let stderr = Arc::new(Mutex::new(String::new()));
        let sink = stderr.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr_pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut s = sink.lock().unwrap_or_else(|e| e.into_inner());
                if s.len() > 8 * 1024 {
                    s.clear();
                }
                s.push_str(&line);
                s.push('\n');
            }
        });
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        let hello = tokio::time::timeout(
            CONNECT,
            provision::read_line_capped(&mut reader, &mut buf, MAX_REPLY),
        )
        .await;
        let why = || {
            let s = stderr.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let line = last_line(&s);
            if line.is_empty() {
                "the remote did not answer".to_string()
            } else {
                line
            }
        };
        match hello {
            Ok(Ok(n)) if n > 0 => {}
            Ok(_) => return Err(why()),
            Err(_) => return Err(format!("the remote did not answer in time ({})", why())),
        }
        let hello: Value = serde_json::from_slice(&buf).map_err(|_| {
            format!(
                "the remote answered something else: {}",
                String::from_utf8_lossy(&buf).trim()
            )
        })?;
        if hello.get("rpc").and_then(Value::as_str) != Some("parzi") {
            return Err("the remote is not running parzi rpc".into());
        }
        if let Some(e) = hello.get("error").and_then(Value::as_str) {
            return Err(e.to_string());
        }
        let version = hello
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let waiters: Waiters = Arc::default();
        let alive = Arc::new(AtomicBool::new(true));
        let events: EventSink = Arc::default();
        let reader = tokio::spawn(pump(reader, waiters.clone(), alive.clone(), events.clone()));
        Ok(Self {
            stdin: tokio::sync::Mutex::new(stdin),
            waiters,
            next: AtomicU64::new(1),
            alive,
            events,
            stderr,
            _child: child,
            reader,
            version,
        })
    }

    /// Live run events, approvals and questions from the server, in the
    /// desk's UI shape. The receiver ends when the link closes.
    pub async fn subscribe(&self) -> Result<tokio::sync::mpsc::UnboundedReceiver<Value>, String> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        *self.events.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        let mut stdin = self.stdin.lock().await;
        let sent = match stdin.write_all(b"{\"op\":\"subscribe\"}\n").await {
            Ok(()) => stdin.flush().await,
            Err(e) => Err(e),
        };
        sent.map_err(|_| self.dead_reason())?;
        Ok(rx)
    }

    #[must_use]
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// Send one op. A reply with `ok: false` becomes `Err(error)`.
    pub async fn call(&self, op: &str, body: Value, wait: Duration) -> Result<Value, String> {
        if !self.alive() {
            return Err(self.dead_reason());
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut req = if body.is_object() { body } else { json!({}) };
        // `rid`, not `id`: session ops already use `id` for the session.
        req["rid"] = json!(id);
        req["op"] = json!(op);
        let mut line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        line.push('\n');
        let (tx, rx) = oneshot::channel();
        self.waiters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        let sent = {
            let mut stdin = self.stdin.lock().await;
            match stdin.write_all(line.as_bytes()).await {
                Ok(()) => stdin.flush().await,
                Err(e) => Err(e),
            }
        };
        if sent.is_err() {
            self.waiters
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            self.alive.store(false, Ordering::Relaxed);
            return Err(self.dead_reason());
        }
        let reply = match tokio::time::timeout(wait, rx).await {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => return Err(self.dead_reason()),
            Err(_) => {
                self.waiters
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
                return Err(format!("the remote did not answer `{op}` in time"));
            }
        };
        if reply.get("ok").and_then(Value::as_bool) == Some(false) {
            return Err(reply
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("failed")
                .to_string());
        }
        Ok(reply)
    }

    fn dead_reason(&self) -> String {
        let s = self
            .stderr
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let line = last_line(&s);
        if line.is_empty() {
            "the link to the remote closed".into()
        } else {
            format!("the link to the remote closed ({line})")
        }
    }
}

async fn pump(
    mut reader: BufReader<tokio::process::ChildStdout>,
    waiters: Waiters,
    alive: Arc<AtomicBool>,
    events: EventSink,
) {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match provision::read_line_capped(&mut reader, &mut buf, MAX_REPLY).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let Ok(mut v) = serde_json::from_slice::<Value>(&buf) else {
            continue;
        };
        if let Some(ev) = v.get_mut("event").map(Value::take) {
            if let Some(tx) = events.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                let _ = tx.send(ev);
            }
            continue;
        }
        let Some(obj) = v.as_object_mut() else {
            continue;
        };
        // `rid` from this release on; an older `parzi rpc` echoed a numeric
        // `id` (session ids are uuids, so the two never mix).
        let id = match obj.get("rid").and_then(Value::as_u64) {
            Some(rid) => {
                obj.remove("rid");
                rid
            }
            None => match obj.get("id").and_then(Value::as_u64) {
                Some(old) => {
                    obj.remove("id");
                    old
                }
                None => continue,
            },
        };
        let tx = waiters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        if let Some(tx) = tx {
            let _ = tx.send(v);
        }
    }
    alive.store(false, Ordering::Relaxed);
    waiters.lock().unwrap_or_else(|e| e.into_inner()).clear();
    // Closing the sink ends the subscriber's loop, so it can reconnect.
    events.lock().unwrap_or_else(|e| e.into_inner()).take();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_single_words_and_ports_parse() {
        let t = Target::parse(" claw ", "gpu-box").unwrap();
        assert_eq!(t.label(), "claw@gpu-box");
        let t = Target::parse("claw", "10.0.0.5:2222").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("10.0.0.5", Some(2222)));
        assert_eq!(Target::parse("a", "fe80::1").unwrap().port, None);
        assert!(Target::parse("", "h").is_err());
        assert!(Target::parse("a b", "h").is_err());
        assert!(Target::parse("a", "-oProxyCommand=x").is_err());
        assert!(Target::parse("-l", "h").is_err());
        assert!(Target::parse("a", "h:99999").is_err());
    }

    #[test]
    fn quoting_survives_the_shells() {
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
        assert_eq!(batch_escape("100%"), "100%%");
        assert_eq!(
            batch_escape("a&b|c<d>e^f\"g(h)"),
            "a^&b^|c^<d^>e^^f^\"g^(h^)"
        );
    }

    #[test]
    fn the_probe_names_a_release_asset() {
        let p = Probe::parse("os=Linux\narch=x86_64\nfetch=curl\nsystemd=yes\nhave=parzi 0.1.22\n");
        assert_eq!(p.asset().unwrap(), "parzi-linux-x64");
        assert!(p.systemd);
        assert_eq!(p.have, "parzi 0.1.22");
        assert!(Probe::parse("os=Darwin\narch=arm64").asset().is_err());
        assert!(Probe::parse("os=Linux\narch=riscv64").asset().is_err());
    }

    #[test]
    fn the_terminal_line_asks_for_a_tty_and_allows_prompts() {
        let t = Target::parse("claw", "box:2222").unwrap();
        let args = Transport::ssh(&t)
            .terminal_args("~/.local/bin/parzi agent login claude")
            .unwrap();
        assert_eq!(args[..2], ["ssh".to_string(), "-t".to_string()]);
        assert!(!args.iter().any(|a| a == "BatchMode=yes" || a == "-T"));
        assert!(args.windows(2).any(|w| w[0] == "-p" && w[1] == "2222"));
        assert_eq!(
            args.last().unwrap(),
            "~/.local/bin/parzi agent login claude"
        );
        assert!(Transport::custom("wsl.exe", &[])
            .terminal_args("x")
            .is_none());
    }

    #[test]
    fn scripts_quote_what_they_embed() {
        let s = authorize_script("ssh-ed25519 AAAAC3Nz parzi@box");
        assert!(s.contains("grep -qxF 'ssh-ed25519 AAAAC3Nz parzi@box'"));
        let s = download_script("o/r", "1.2.3", "parzi-linux-x64", "curl");
        assert!(s.contains("'https://github.com/o/r/releases/download/v1.2.3/parzi-linux-x64'"));
        assert!(s.contains("sha256sum"));
        let s = spec_script("{\"version\":1}");
        assert!(s.contains("{\"version\":1}"));
        assert!(s.starts_with("umask 077"));
    }
}
