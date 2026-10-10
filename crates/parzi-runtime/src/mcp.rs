use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parzi_core::config::McpServerCfg;
use parzi_core::error::{ParziError, Result};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const PROTOCOL_VERSION: &str = "2024-11-05";

const HANDSHAKE_FLOOR: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct McpTool {
    pub server: String,
    pub name: String,
}

impl McpTool {
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.server, self.name)
    }
}

struct LiveServer {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    tools: Option<Vec<McpTool>>,
    last_used: Instant,
    epoch: u64,
}

type Slot = Arc<tokio::sync::Mutex<Option<LiveServer>>>;

pub struct McpManager {
    configs: std::sync::RwLock<HashMap<String, McpServerCfg>>,
    slots: tokio::sync::Mutex<HashMap<String, Slot>>,
    idle_kill: Duration,
    epoch: AtomicU64,
    exposed_calls: AtomicU64,
}

/// The parent variables any spawned helper (MCP server, hook, shell.exec)
/// sees: OS plumbing and toolchain homes, never keys or tokens.
const CHILD_ENV_PASSTHROUGH: &[&str] = &[
    "PATH",
    "SYSTEMROOT",
    "TEMP",
    "TMP",
    "TMPDIR",
    "HOME",
    "APPDATA",
    "USERPROFILE",
    "LANG",
    "LC_ALL",
    "PATHEXT",
    "COMSPEC",
    "WINDIR",
    "SYSTEMDRIVE",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "USERNAME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "GOPATH",
    "GOROOT",
    "JAVA_HOME",
];

/// Scrubbed child environment: the passthrough list plus `extra`.
pub(crate) fn child_env(extra: &HashMap<String, String>) -> HashMap<String, String> {
    let parent: HashMap<String, String> = std::env::vars().collect();
    child_env_from(&parent, extra)
}

fn child_env_from(
    parent: &HashMap<String, String>,
    extra: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for k in CHILD_ENV_PASSTHROUGH {
        let hit = parent
            .get(*k)
            .map(|v| ((*k).to_string(), v.clone()))
            .or_else(|| {
                if !cfg!(windows) {
                    return None;
                }
                parent
                    .iter()
                    .find(|(pk, _)| pk.eq_ignore_ascii_case(k))
                    .map(|(pk, v)| (pk.clone(), v.clone()))
            });
        if let Some((name, v)) = hit {
            if !v.is_empty() {
                out.insert(name, v);
            }
        }
    }
    for (k, v) in extra {
        out.insert(k.clone(), v.clone());
    }
    out
}

fn tool_err(msg: String) -> ParziError {
    ParziError::Tool("mcp".into(), msg)
}

async fn rpc_round_trip<W, R>(
    stdin: &mut W,
    stdout: &mut R,
    id: u64,
    method: &str,
    params: serde_json::Value,
    timeout: Duration,
) -> Result<std::result::Result<serde_json::Value, String>>
where
    W: AsyncWrite + Unpin,
    R: AsyncBufRead + Unpin,
{
    let mut wire = serde_json::to_string(&serde_json::json!({
        "jsonrpc": "2.0", "id": id, "method": method, "params": params,
    }))?;
    wire.push('\n');
    stdin
        .write_all(wire.as_bytes())
        .await
        .map_err(|e| tool_err(format!("write failed: {e}")))?;
    stdin
        .flush()
        .await
        .map_err(|e| tool_err(format!("flush failed: {e}")))?;
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let mut out = String::new();
        let n = tokio::time::timeout_at(deadline, stdout.read_line(&mut out))
            .await
            .map_err(|_| tool_err(format!("{method} timed out")))?
            .map_err(|e| tool_err(format!("read failed: {e}")))?;
        if n == 0 {
            return Err(tool_err(format!("{method}: server closed the pipe")));
        }
        let line = out.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            tracing::warn!(
                "mcp: skipping non-json stdout line: {}",
                line.chars().take(200).collect::<String>()
            );
            continue;
        };
        if v.get("method").is_some() {
            continue;
        }
        if v.get("id").and_then(|i| i.as_u64()) != Some(id) {
            continue;
        }
        if let Some(err) = v.get("error") {
            return Ok(Err(format!("server error: {err}")));
        }
        return Ok(Ok(v
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null)));
    }
}

async fn kill_tree(child: &mut Child) {
    // Descendants first (blocking taskkill/ps, off the workers), then the root.
    if let Some(pid) = child.id() {
        let _ = tokio::task::spawn_blocking(move || parzi_providers::process::kill_tree(pid)).await;
    }
    let _ = child.kill().await;
}

impl McpManager {
    pub fn new(configs: HashMap<String, McpServerCfg>, idle_kill_secs: u64) -> Self {
        Self {
            configs: std::sync::RwLock::new(configs),
            slots: tokio::sync::Mutex::new(HashMap::new()),
            idle_kill: Duration::from_secs(idle_kill_secs.max(10)),
            epoch: AtomicU64::new(0),
            exposed_calls: AtomicU64::new(0),
        }
    }

    pub fn exposed_tools_calls(&self) -> u64 {
        self.exposed_calls.load(Ordering::Relaxed)
    }

    fn config_for(&self, name: &str) -> Option<McpServerCfg> {
        self.configs.read().ok()?.get(name).cloned()
    }

    async fn slot_for(&self, name: &str) -> Slot {
        self.slots
            .lock()
            .await
            .entry(name.to_string())
            .or_default()
            .clone()
    }

    pub(crate) async fn set_configs(&self, configs: HashMap<String, McpServerCfg>) {
        let dead: Vec<String> = {
            let slots = self.slots.lock().await;
            slots
                .keys()
                .filter(|k| match configs.get(*k) {
                    None => true,
                    Some(c) => !c.enabled,
                })
                .cloned()
                .collect()
        };
        for k in dead {
            self.stop(&k).await;
        }
        if let Ok(mut w) = self.configs.write() {
            *w = configs;
        }
    }

    pub fn is_tool_exposed(&self, server: &str, tool: &str) -> bool {
        match self.config_for(server) {
            Some(c) => c.is_tool_exposed(tool),
            None => false,
        }
    }

    pub fn tool_mode(&self, server: &str, tool: &str) -> Option<String> {
        self.config_for(server)?.tool_mode(tool)
    }

    pub async fn exposed_tools(&self, name: &str) -> Result<Vec<McpTool>> {
        self.exposed_calls.fetch_add(1, Ordering::Relaxed);
        let all = self.list_tools(name).await?;
        Ok(all
            .into_iter()
            .filter(|t| self.is_tool_exposed(name, &t.name))
            .collect())
    }

    async fn request(
        sv: &mut LiveServer,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<std::result::Result<serde_json::Value, String>> {
        sv.next_id += 1;
        let id = sv.next_id;
        rpc_round_trip(&mut sv.stdin, &mut sv.stdout, id, method, params, timeout).await
    }

    async fn handshake(sv: &mut LiveServer, name: &str, timeout: Duration) -> Result<()> {
        let init = serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "parzi", "version": env!("CARGO_PKG_VERSION")},
        });
        let res = Self::request(sv, "initialize", init, timeout)
            .await?
            .map_err(tool_err)?;
        match res.get("protocolVersion").and_then(|v| v.as_str()) {
            Some(v) if v != PROTOCOL_VERSION => {
                tracing::warn!(server = name, version = v, "mcp: server negotiated down");
            }
            None => {
                return Err(tool_err(format!(
                    "`{name}`: initialize answered without protocolVersion"
                )))
            }
            _ => {}
        }
        let note = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let mut wire = serde_json::to_string(&note)?;
        wire.push('\n');
        sv.stdin
            .write_all(wire.as_bytes())
            .await
            .map_err(|e| tool_err(format!("write failed: {e}")))?;
        sv.stdin
            .flush()
            .await
            .map_err(|e| tool_err(format!("flush failed: {e}")))?;
        Ok(())
    }

    async fn ensure_live(
        &self,
        slot: &Slot,
        guard: &mut Option<LiveServer>,
        name: &str,
    ) -> Result<()> {
        if guard.is_some() {
            return Ok(());
        }
        let cfg = self
            .config_for(name)
            .ok_or_else(|| tool_err(format!("unknown server `{name}`")))?;
        if !cfg.enabled {
            return Err(tool_err(format!("server `{name}` disabled")));
        }
        let mut cmd = Command::new(&cfg.command);
        cmd.args(&cfg.args)
            .env_clear()
            .envs(child_env(&cfg.env))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
        let mut child: Child = cmd
            .spawn()
            .map_err(|e| tool_err(format!("spawn `{name}`: {e}")))?;
        parzi_providers::process::adopt(&child);
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| tool_err("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| tool_err("no stdout".into()))?;
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        let mut sv = LiveServer {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            tools: None,
            last_used: Instant::now(),
            epoch,
        };
        let timeout = Duration::from_millis(cfg.timeout_ms.max(1_000));
        let handshake = timeout.max(HANDSHAKE_FLOOR);
        if let Err(e) = Self::handshake(&mut sv, name, handshake).await {
            kill_tree(&mut sv.child).await;
            return Err(e);
        }
        *guard = Some(sv);
        Self::spawn_reaper(name.to_string(), slot.clone(), self.idle_kill, epoch);
        Ok(())
    }

    fn spawn_reaper(name: String, slot: Slot, idle: Duration, epoch: u64) {
        let tick = std::cmp::max(idle / 2, Duration::from_secs(1));
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tick).await;
                let mut guard = slot.lock().await;
                match guard.as_ref() {
                    Some(sv) if sv.epoch == epoch => {
                        if sv.last_used.elapsed() >= idle {
                            Self::drop_live(&mut guard).await;
                            tracing::debug!(server = %name, "mcp: reaped idle server");
                            return;
                        }
                    }
                    _ => return,
                }
            }
        });
    }

    async fn drop_live(guard: &mut Option<LiveServer>) {
        if let Some(mut sv) = guard.take() {
            kill_tree(&mut sv.child).await;
        }
    }

    fn timeout_for(&self, name: &str) -> Duration {
        Duration::from_millis(
            self.config_for(name)
                .map(|c| c.timeout_ms)
                .unwrap_or(30_000)
                .max(1_000),
        )
    }

    async fn list_tools(&self, name: &str) -> Result<Vec<McpTool>> {
        let slot = self.slot_for(name).await;
        let mut guard = slot.lock().await;
        self.ensure_live(&slot, &mut guard, name).await?;
        let cached = {
            let sv = guard.as_mut().expect("live after ensure");
            sv.last_used = Instant::now();
            sv.tools.clone()
        };
        if let Some(t) = cached {
            return Ok(t);
        }
        let timeout = self.timeout_for(name);
        let res = {
            let sv = guard.as_mut().expect("live after ensure");
            Self::request(sv, "tools/list", serde_json::json!({}), timeout).await
        };
        let res = match res {
            Ok(Ok(v)) => v,
            Ok(Err(msg)) => return Err(tool_err(msg)),
            Err(e) => {
                Self::drop_live(&mut guard).await;
                return Err(e);
            }
        };
        let mut out = vec![];
        if let Some(arr) = res.get("tools").and_then(|t| t.as_array()) {
            for t in arr {
                out.push(McpTool {
                    server: name.to_string(),
                    name: t.get("name").and_then(|n| n.as_str()).unwrap_or("?").into(),
                });
            }
        }
        if let Some(sv) = guard.as_mut() {
            sv.tools = Some(out.clone());
        }
        Ok(out)
    }

    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> (bool, String) {
        let res = self.call_inner(server, tool, args).await;
        match res {
            Ok(t) => (true, t),
            Err(e) => (false, e.to_string()),
        }
    }

    async fn call_inner(
        &self,
        server: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<String> {
        let slot = self.slot_for(server).await;
        let mut guard = slot.lock().await;
        self.ensure_live(&slot, &mut guard, server).await?;
        let timeout = self.timeout_for(server);
        let params = serde_json::json!({"name": tool, "arguments": args});
        let res = {
            let sv = guard.as_mut().expect("live after ensure");
            sv.last_used = Instant::now();
            Self::request(sv, "tools/call", params, timeout).await
        };
        let res = match res {
            Ok(Ok(v)) => v,
            Ok(Err(msg)) => return Err(tool_err(msg)),
            Err(e) => {
                Self::drop_live(&mut guard).await;
                return Err(e);
            }
        };
        let is_err = res
            .get("isError")
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        let mut text = String::new();
        if let Some(arr) = res.get("content").and_then(|c| c.as_array()) {
            for c in arr {
                if let Some(t) = c.get("text").and_then(|t| t.as_str()) {
                    text.push_str(t);
                    text.push('\n');
                }
            }
        }
        if text.is_empty() {
            text = res.to_string();
        }
        if is_err {
            return Err(ParziError::Tool(server.into(), text));
        }
        Ok(text)
    }

    pub async fn stop(&self, name: &str) {
        let slot = self.slots.lock().await.get(name).cloned();
        if let Some(slot) = slot {
            let mut guard = slot.lock().await;
            Self::drop_live(&mut guard).await;
        }
    }

    pub async fn shutdown(&self) {
        let slots: Vec<Slot> = {
            let mut reg = self.slots.lock().await;
            reg.drain().map(|(_, s)| s).collect()
        };
        for slot in slots {
            let mut guard = slot.lock().await;
            Self::drop_live(&mut guard).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn child_env_scrubs_parent_secrets_but_keeps_passthrough_and_cfg() {
        let parent = map(&[
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/home/u"),
            ("ANTHROPIC_API_KEY", "sk-ant-secret"),
            ("XAI_API_KEY", "x-secret"),
            ("SOME_RANDOM_VAR", "nope"),
        ]);
        let extra = map(&[("GITHUB_PERSONAL_ACCESS_TOKEN", "ghp-x")]);
        let env = child_env_from(&parent, &extra);
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin:/bin"));
        assert_eq!(env.get("HOME").map(String::as_str), Some("/home/u"));
        assert_eq!(
            env.get("GITHUB_PERSONAL_ACCESS_TOKEN").map(String::as_str),
            Some("ghp-x")
        );
        assert!(!env.contains_key("ANTHROPIC_API_KEY"));
        assert!(!env.contains_key("XAI_API_KEY"));
        assert!(!env.contains_key("SOME_RANDOM_VAR"));
    }

    #[test]
    fn child_env_passes_plumbing_never_secrets() {
        for k in [
            "PATH",
            "SYSTEMROOT",
            "TEMP",
            "HOME",
            "USERPROFILE",
            "PATHEXT",
        ] {
            assert!(CHILD_ENV_PASSTHROUGH.contains(&k), "{k} must pass");
        }
        for k in CHILD_ENV_PASSTHROUGH {
            let up = k.to_ascii_uppercase();
            assert!(
                !["KEY", "TOKEN", "SECRET", "PASS"]
                    .iter()
                    .any(|s| up.contains(s)),
                "{k} looks like a secret"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn child_env_matches_windows_env_casing() {
        let parent = map(&[("SystemRoot", "C:\\Windows"), ("Path", "C:\\bin")]);
        let env = child_env_from(&parent, &HashMap::new());
        assert_eq!(
            env.get("SystemRoot").map(String::as_str),
            Some("C:\\Windows")
        );
        assert_eq!(env.get("Path").map(String::as_str), Some("C:\\bin"));
    }

    #[tokio::test]
    async fn round_trip_skips_notifications_and_stale_ids() {
        let (client, server) = tokio::io::duplex(4096);
        let (rx, mut tx) = tokio::io::split(client);
        let mut rx = BufReader::new(rx);
        tokio::spawn(async move {
            let (srx, mut stx) = tokio::io::split(server);
            let mut line = String::new();
            BufReader::new(srx).read_line(&mut line).await.unwrap();
            for frame in [
                "[info] listening on stdio",
                r#"{"jsonrpc":"2.0","method":"notifications/message","params":{}}"#,
                r#"{"jsonrpc":"2.0","id":41,"result":{"stale":true}}"#,
                r#"{"jsonrpc":"2.0","id":7,"method":"sampling/createMessage","params":{}}"#,
                r#"{"jsonrpc":"2.0","id":7,"result":{"ok":true}}"#,
            ] {
                stx.write_all(format!("{frame}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        let got = rpc_round_trip(
            &mut tx,
            &mut rx,
            7,
            "tools/list",
            serde_json::json!({}),
            Duration::from_secs(5),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(got, serde_json::json!({"ok": true}));
    }

    #[tokio::test]
    async fn round_trip_times_out_on_silence() {
        let (client, _server) = tokio::io::duplex(4096);
        let (rx, mut tx) = tokio::io::split(client);
        let mut rx = BufReader::new(rx);
        let err = rpc_round_trip(
            &mut tx,
            &mut rx,
            1,
            "tools/call",
            serde_json::json!({}),
            Duration::from_millis(80),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
    }

    #[tokio::test]
    async fn server_error_object_keeps_the_connection() {
        let (client, server) = tokio::io::duplex(4096);
        let (rx, mut tx) = tokio::io::split(client);
        let mut rx = BufReader::new(rx);
        tokio::spawn(async move {
            let (srx, mut stx) = tokio::io::split(server);
            let mut line = String::new();
            BufReader::new(srx).read_line(&mut line).await.unwrap();
            stx.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32601}}\n")
                .await
                .unwrap();
        });
        let got = rpc_round_trip(
            &mut tx,
            &mut rx,
            1,
            "tools/call",
            serde_json::json!({}),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert!(got.unwrap_err().contains("server error"));
    }
}
