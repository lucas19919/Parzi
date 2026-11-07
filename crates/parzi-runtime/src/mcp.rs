use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parzi_core::config::McpServerCfg;
use parzi_core::error::{ParziError, Result};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const PROTOCOL_VERSION: &str = "2024-11-05";
const HTTP_PROTOCOL_VERSION: &str = "2025-06-18";

const HANDSHAKE_FLOOR: Duration = Duration::from_secs(10);

/// How long a session waits, all connectors together, for ones it has never
/// listed. The rest finish in the background for the next turn.
pub const COLD_LIST_BUDGET: Duration = Duration::from_secs(2);
/// A remembered list older than this is used, and listed again behind it.
const STALE_AFTER: Duration = Duration::from_secs(30 * 60);
/// A connector that failed to list is not tried again for a session this soon.
const FAIL_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct McpTool {
    pub server: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub destructive: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub annotations: Value,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ServerDetails {
    pub title: String,
    pub website: String,
    pub icons: Vec<String>,
    pub tools: Vec<McpTool>,
}

impl McpTool {
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.server, self.name)
    }

    fn from_listing(server: &str, t: &Value) -> Self {
        let text = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or("").trim().to_string();
        let hint = |k: &str| {
            t.pointer(&format!("/annotations/{k}"))
                .and_then(Value::as_bool)
                == Some(true)
        };
        let mut title = text(t.get("title"));
        if title.is_empty() {
            title = text(t.pointer("/annotations/title"));
        }
        Self {
            server: server.to_string(),
            name: t.get("name").and_then(Value::as_str).unwrap_or("?").into(),
            title,
            description: text(t.get("description")),
            read_only: hint("readOnlyHint"),
            destructive: hint("destructiveHint"),
            input_schema: t.get("inputSchema").cloned().unwrap_or(Value::Null),
            annotations: t
                .get("annotations")
                .filter(|a| a.is_object())
                .cloned()
                .unwrap_or(Value::Null),
        }
    }
}

enum Conn {
    Stdio {
        child: Box<Child>,
        stdin: Box<ChildStdin>,
        stdout: Box<BufReader<ChildStdout>>,
    },
    Http(Arc<std::sync::Mutex<crate::mcp_http::HttpConn>>),
}

struct LiveServer {
    conn: Conn,
    info: Value,
    next_id: u64,
    tools: Option<Vec<McpTool>>,
    last_used: Instant,
    epoch: u64,
}

type Slot = Arc<tokio::sync::Mutex<Option<LiveServer>>>;

/// A connector's tool list as last seen, from a listing or from disk.
struct Known {
    key: String,
    tools: Vec<McpTool>,
    saved_ms: u64,
}

pub struct McpManager {
    inner: Arc<Inner>,
}

struct Inner {
    configs: std::sync::RwLock<HashMap<String, McpServerCfg>>,
    slots: tokio::sync::Mutex<HashMap<String, Slot>>,
    idle_kill: Duration,
    epoch: AtomicU64,
    exposed_calls: AtomicU64,
    starts: AtomicU64,
    known: std::sync::Mutex<HashMap<String, Known>>,
    listing: std::sync::Mutex<HashMap<String, Instant>>,
    failed: std::sync::Mutex<HashMap<String, (String, Instant)>>,
    settled: tokio::sync::watch::Sender<u64>,
}

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

async fn http_send(
    conn: Arc<std::sync::Mutex<crate::mcp_http::HttpConn>>,
    name: &str,
    msg: Value,
    timeout: Duration,
) -> Result<Option<Value>> {
    let name = name.to_string();
    let out = tokio::task::spawn_blocking(move || {
        let mut c = conn
            .lock()
            .map_err(|_| crate::mcp_http::HttpError::Other("connection poisoned".into()))?;
        c.send(&msg, timeout)
    })
    .await
    .map_err(|e| tool_err(e.to_string()))?;
    out.map_err(|e| match e {
        crate::mcp_http::HttpError::SignIn(challenge) => {
            crate::mcp_http::remember_challenge(&name, challenge);
            tool_err(format!(
                "{}: sign in to {name} in Settings › Connections",
                crate::mcp_http::SIGN_IN
            ))
        }
        crate::mcp_http::HttpError::Other(e) => tool_err(format!("`{name}`: {e}")),
    })
}

async fn rpc_round_trip<W, R>(
    stdin: &mut W,
    stdout: &mut R,
    id: u64,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<std::result::Result<Value, String>>
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
        let Ok(v) = serde_json::from_str::<Value>(line) else {
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
        return Ok(Ok(v.get("result").cloned().unwrap_or(Value::Null)));
    }
}

async fn kill_tree(child: &mut Child) {
    if let Some(pid) = child.id() {
        let _ = tokio::task::spawn_blocking(move || parzi_providers::process::kill_tree(pid)).await;
    }
    let _ = child.kill().await;
}

impl McpManager {
    pub fn new(configs: HashMap<String, McpServerCfg>, idle_kill_secs: u64) -> Self {
        Self {
            inner: Arc::new(Inner {
                configs: std::sync::RwLock::new(configs),
                slots: tokio::sync::Mutex::new(HashMap::new()),
                idle_kill: Duration::from_secs(idle_kill_secs.max(10)),
                epoch: AtomicU64::new(0),
                exposed_calls: AtomicU64::new(0),
                starts: AtomicU64::new(0),
                known: std::sync::Mutex::default(),
                listing: std::sync::Mutex::default(),
                failed: std::sync::Mutex::default(),
                settled: tokio::sync::watch::channel(0).0,
            }),
        }
    }

    pub fn exposed_tools_calls(&self) -> u64 {
        self.inner.exposed_calls.load(Ordering::Relaxed)
    }

    /// How many times a connector was started or connected to.
    pub fn live_starts(&self) -> u64 {
        self.inner.starts.load(Ordering::Relaxed)
    }

    pub(crate) async fn set_configs(&self, configs: HashMap<String, McpServerCfg>) {
        let dead: Vec<String> = {
            let old = self
                .inner
                .configs
                .read()
                .map(|c| c.clone())
                .unwrap_or_default();
            let slots = self.inner.slots.lock().await;
            slots
                .keys()
                .filter(|k| match (configs.get(*k), old.get(*k)) {
                    (Some(c), Some(o)) => {
                        !c.enabled || crate::mcp_cache::key(k, c) != crate::mcp_cache::key(k, o)
                    }
                    (Some(c), None) => !c.enabled,
                    (None, _) => true,
                })
                .cloned()
                .collect()
        };
        for k in dead {
            self.stop(&k).await;
        }
        if let Ok(mut w) = self.inner.configs.write() {
            *w = configs;
        }
    }

    pub fn is_tool_exposed(&self, server: &str, tool: &str) -> bool {
        self.inner.is_tool_exposed(server, tool)
    }

    pub fn tool_mode(&self, server: &str, tool: &str) -> Option<String> {
        self.inner.config_for(server)?.tool_mode(tool)
    }

    pub async fn exposed_tools(&self, name: &str) -> Result<Vec<McpTool>> {
        self.inner.exposed_calls.fetch_add(1, Ordering::Relaxed);
        let all = self.inner.list_tools(name, false).await?;
        Ok(all
            .into_iter()
            .filter(|t| self.is_tool_exposed(name, &t.name))
            .collect())
    }

    /// Every enabled connector's tools for a session, from what was listed
    /// before. A connector never listed gets a short try, at most `budget`
    /// for all of them together, and lists on in the background otherwise;
    /// an old list is used while a new one is fetched behind it.
    pub async fn session_tools(&self, budget: Duration) -> Vec<McpTool> {
        let mut configs: Vec<(String, McpServerCfg)> = self
            .inner
            .configs
            .read()
            .map(|c| {
                c.iter()
                    .filter(|(_, c)| c.enabled)
                    .map(|(k, c)| (k.clone(), c.clone()))
                    .collect()
            })
            .unwrap_or_default();
        configs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out = vec![];
        let mut cold = vec![];
        for (name, cfg) in &configs {
            match self.inner.cached(name, cfg) {
                Some((tools, stale)) => {
                    if stale {
                        Inner::start_listing(&self.inner, name);
                    }
                    out.extend(tools);
                }
                None => {
                    if Inner::start_listing(&self.inner, name) {
                        cold.push(name.clone());
                    }
                }
            }
        }
        if !cold.is_empty() {
            self.inner.wait_listed(&cold, budget).await;
            for (name, cfg) in configs.iter().filter(|(n, _)| cold.contains(n)) {
                if let Some((tools, _)) = self.inner.cached(name, cfg) {
                    out.extend(tools);
                }
            }
        }
        out.retain(|t| self.is_tool_exposed(&t.server, &t.name));
        out
    }

    pub async fn details(&self, name: &str) -> Result<ServerDetails> {
        let tools = self.inner.list_tools(name, false).await?;
        let slot = self.inner.slot_for(name).await;
        let info = slot
            .lock()
            .await
            .as_ref()
            .map(|sv| sv.info.clone())
            .unwrap_or_default();
        let text = |k: &str| {
            info.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let icons = info
            .get("icons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|i| i.get("src").and_then(Value::as_str))
            .filter(|s| s.starts_with("https://") || s.starts_with("data:image/"))
            .map(str::to_string)
            .collect();
        Ok(ServerDetails {
            title: if text("title").is_empty() {
                text("name")
            } else {
                text("title")
            },
            website: text("websiteUrl"),
            icons,
            tools,
        })
    }

    pub async fn call_tool(&self, server: &str, tool: &str, args: Value) -> (bool, String) {
        match self.call_raw(server, tool, args).await {
            Ok(res) => {
                let (ok, blocks) = crate::mcp_relay::relay_result(&res);
                let text = crate::mcp_relay::text_of(&blocks);
                if ok {
                    (true, text)
                } else {
                    (false, ParziError::Tool(server.into(), text).to_string())
                }
            }
            Err(e) => (false, e.to_string()),
        }
    }

    /// A `tools/call` as the connector answered it, `isError` and all; `Err`
    /// when the connector could not be reached or broke the protocol.
    pub async fn call_raw(&self, server: &str, tool: &str, args: Value) -> Result<Value> {
        if !self.is_tool_exposed(server, tool) {
            return Err(tool_err(format!(
                "`{tool}` is switched off for the connector `{server}`"
            )));
        }
        self.inner.call(server, tool, args).await
    }

    pub async fn stop(&self, name: &str) {
        let slot = self.inner.slots.lock().await.get(name).cloned();
        if let Some(slot) = slot {
            let mut guard = slot.lock().await;
            Inner::drop_live(&mut guard).await;
        }
    }

    pub async fn shutdown(&self) {
        let slots: Vec<Slot> = {
            let mut reg = self.inner.slots.lock().await;
            reg.drain().map(|(_, s)| s).collect()
        };
        for slot in slots {
            let mut guard = slot.lock().await;
            Inner::drop_live(&mut guard).await;
        }
    }
}

fn age(saved_ms: u64) -> Duration {
    Duration::from_millis(parzi_core::now_ms().saturating_sub(saved_ms))
}

impl Inner {
    fn config_for(&self, name: &str) -> Option<McpServerCfg> {
        self.configs.read().ok()?.get(name).cloned()
    }

    fn is_tool_exposed(&self, server: &str, tool: &str) -> bool {
        self.config_for(server)
            .is_some_and(|c| c.is_tool_exposed(tool))
    }

    async fn slot_for(&self, name: &str) -> Slot {
        self.slots
            .lock()
            .await
            .entry(name.to_string())
            .or_default()
            .clone()
    }

    /// What `name` offered when last listed, and whether that is old.
    fn cached(&self, name: &str, cfg: &McpServerCfg) -> Option<(Vec<McpTool>, bool)> {
        let key = crate::mcp_cache::key(name, cfg);
        if let Ok(known) = self.known.lock() {
            if let Some(k) = known.get(name).filter(|k| k.key == key) {
                return Some((k.tools.clone(), age(k.saved_ms) > STALE_AFTER));
            }
        }
        let saved = crate::mcp_cache::load(name, cfg)?;
        let stale = age(saved.saved_ms) > STALE_AFTER;
        if let Ok(mut known) = self.known.lock() {
            known.insert(
                name.to_string(),
                Known {
                    key,
                    tools: saved.tools.clone(),
                    saved_ms: saved.saved_ms,
                },
            );
        }
        Some((saved.tools, stale))
    }

    fn remember(&self, name: &str, tools: &[McpTool]) {
        let Some(cfg) = self.config_for(name) else {
            return;
        };
        let key = crate::mcp_cache::key(name, &cfg);
        let saved_ms = crate::mcp_cache::save(name, &cfg, tools)
            .map_or_else(parzi_core::now_ms, |s| s.saved_ms);
        if let Ok(mut known) = self.known.lock() {
            known.insert(
                name.to_string(),
                Known {
                    key,
                    tools: tools.to_vec(),
                    saved_ms,
                },
            );
        }
        if let Ok(mut f) = self.failed.lock() {
            f.remove(name);
        }
    }

    /// Lists `name` in the background unless that is under way already.
    /// False when it failed a moment ago and is left alone for now.
    fn start_listing(me: &Arc<Self>, name: &str) -> bool {
        let Some(cfg) = me.config_for(name).filter(|c| c.enabled) else {
            return false;
        };
        let key = crate::mcp_cache::key(name, &cfg);
        let backing_off = me.failed.lock().is_ok_and(|f| {
            f.get(name)
                .is_some_and(|(k, at)| *k == key && at.elapsed() < FAIL_BACKOFF)
        });
        if backing_off {
            return false;
        }
        {
            let Ok(mut l) = me.listing.lock() else {
                return false;
            };
            if l.contains_key(name) {
                return true;
            }
            l.insert(name.to_string(), Instant::now());
        }
        let (me, name) = (me.clone(), name.to_string());
        tokio::spawn(async move {
            let res = me.list_tools(&name, true).await;
            if let Ok(mut l) = me.listing.lock() {
                l.remove(&name);
            }
            if let Err(e) = res {
                tracing::info!(server = %name, "connector did not list its tools: {e}");
                if let Ok(mut f) = me.failed.lock() {
                    f.insert(name, (key, Instant::now()));
                }
            }
            me.settled.send_modify(|n| *n = n.wrapping_add(1));
        });
        true
    }

    /// Waits for the listings of `names`, each at most `budget` from when it
    /// started, and never longer than `budget` from now.
    async fn wait_listed(&self, names: &[String], budget: Duration) {
        let mut settled = self.settled.subscribe();
        let latest = Instant::now() + budget;
        loop {
            let due = self.listing.lock().ok().and_then(|l| {
                names
                    .iter()
                    .filter_map(|n| l.get(n))
                    .map(|started| *started + budget)
                    .max()
            });
            let Some(due) = due.map(|d| d.min(latest)) else {
                return;
            };
            if Instant::now() >= due {
                return;
            }
            let until = tokio::time::Instant::from_std(due);
            if tokio::time::timeout_at(until, settled.changed())
                .await
                .is_err()
            {
                return;
            }
        }
    }

    async fn request(
        sv: &mut LiveServer,
        name: &str,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<std::result::Result<Value, String>> {
        sv.next_id += 1;
        let id = sv.next_id;
        match &mut sv.conn {
            Conn::Stdio { stdin, stdout, .. } => {
                rpc_round_trip(stdin, stdout, id, method, params, timeout).await
            }
            Conn::Http(h) => {
                let msg = serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
                let reply = http_send(h.clone(), name, msg, timeout)
                    .await?
                    .unwrap_or(Value::Null);
                Ok(match reply.get("error") {
                    Some(err) => Err(format!("server error: {err}")),
                    None => Ok(reply.get("result").cloned().unwrap_or(Value::Null)),
                })
            }
        }
    }

    async fn notify(
        sv: &mut LiveServer,
        name: &str,
        method: &str,
        timeout: Duration,
    ) -> Result<()> {
        let note = serde_json::json!({"jsonrpc": "2.0", "method": method});
        match &mut sv.conn {
            Conn::Stdio { stdin, .. } => {
                let mut wire = serde_json::to_string(&note)?;
                wire.push('\n');
                stdin
                    .write_all(wire.as_bytes())
                    .await
                    .map_err(|e| tool_err(format!("write failed: {e}")))?;
                stdin
                    .flush()
                    .await
                    .map_err(|e| tool_err(format!("flush failed: {e}")))
            }
            Conn::Http(h) => http_send(h.clone(), name, note, timeout).await.map(|_| ()),
        }
    }

    async fn handshake(sv: &mut LiveServer, name: &str, timeout: Duration) -> Result<()> {
        let offered = match sv.conn {
            Conn::Stdio { .. } => PROTOCOL_VERSION,
            Conn::Http(_) => HTTP_PROTOCOL_VERSION,
        };
        let init = serde_json::json!({
            "protocolVersion": offered,
            "capabilities": {},
            "clientInfo": {"name": "parzi", "version": env!("CARGO_PKG_VERSION")},
        });
        let res = Self::request(sv, name, "initialize", init, timeout)
            .await?
            .map_err(tool_err)?;
        sv.info = res.get("serverInfo").cloned().unwrap_or(Value::Null);
        match res.get("protocolVersion").and_then(|v| v.as_str()) {
            Some(v) => {
                if v != offered {
                    tracing::debug!(server = name, version = v, "mcp: server negotiated {v}");
                }
                if let Conn::Http(h) = &sv.conn {
                    if let Ok(mut c) = h.lock() {
                        c.set_protocol(v);
                    }
                }
            }
            None => {
                return Err(tool_err(format!(
                    "`{name}`: initialize answered without protocolVersion"
                )))
            }
        }
        Self::notify(sv, name, "notifications/initialized", timeout).await
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
        self.starts.fetch_add(1, Ordering::Relaxed);
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        let timeout = Duration::from_millis(cfg.timeout_ms.max(1_000));
        let handshake = timeout.max(HANDSHAKE_FLOOR);
        if !cfg.url.trim().is_empty() {
            let mut sv = LiveServer {
                conn: Conn::Http(Arc::new(std::sync::Mutex::new(
                    crate::mcp_http::HttpConn::new(name, &cfg),
                ))),
                next_id: 0,
                tools: None,
                info: Value::Null,
                last_used: Instant::now(),
                epoch,
            };
            Self::handshake(&mut sv, name, handshake).await?;
            *guard = Some(sv);
            Self::spawn_reaper(name.to_string(), slot.clone(), self.idle_kill, epoch);
            return Ok(());
        }
        let program = parzi_providers::process::which(&cfg.command).ok_or_else(|| {
            tool_err(format!(
                "`{}` isn't installed on this computer",
                cfg.command
            ))
        })?;
        let mut cmd = Command::new(program);
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
        let mut sv = LiveServer {
            conn: Conn::Stdio {
                child: Box::new(child),
                stdin: Box::new(stdin),
                stdout: Box::new(BufReader::new(stdout)),
            },
            next_id: 0,
            tools: None,
            info: Value::Null,
            last_used: Instant::now(),
            epoch,
        };
        if let Err(e) = Self::handshake(&mut sv, name, handshake).await {
            Self::drop_live(&mut Some(sv)).await;
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
        if let Some(LiveServer {
            conn: Conn::Stdio { mut child, .. },
            ..
        }) = guard.take()
        {
            kill_tree(&mut child).await;
        }
    }

    fn timeout_for(&self, name: &str) -> Duration {
        Duration::from_millis(
            self.config_for(name)
                .map_or(30_000, |c| c.timeout_ms)
                .max(1_000),
        )
    }

    /// The connector's tools; `fresh` asks it again even when this connection
    /// listed them already.
    async fn list_tools(&self, name: &str, fresh: bool) -> Result<Vec<McpTool>> {
        let slot = self.slot_for(name).await;
        let mut guard = slot.lock().await;
        self.ensure_live(&slot, &mut guard, name).await?;
        let cached = {
            let sv = guard.as_mut().expect("live after ensure");
            sv.last_used = Instant::now();
            sv.tools.clone().filter(|_| !fresh)
        };
        if let Some(t) = cached {
            return Ok(t);
        }
        let timeout = self.timeout_for(name);
        let res = {
            let sv = guard.as_mut().expect("live after ensure");
            Self::request(sv, name, "tools/list", serde_json::json!({}), timeout).await
        };
        let res = match res {
            Ok(Ok(v)) => v,
            Ok(Err(msg)) => return Err(tool_err(msg)),
            Err(e) => {
                Self::drop_live(&mut guard).await;
                return Err(e);
            }
        };
        let out: Vec<McpTool> = res
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|t| McpTool::from_listing(name, t))
            .collect();
        if let Some(sv) = guard.as_mut() {
            sv.tools = Some(out.clone());
        }
        drop(guard);
        self.remember(name, &out);
        Ok(out)
    }

    async fn call(&self, server: &str, tool: &str, args: Value) -> Result<Value> {
        let slot = self.slot_for(server).await;
        let mut guard = slot.lock().await;
        self.ensure_live(&slot, &mut guard, server).await?;
        let timeout = self.timeout_for(server);
        let params = serde_json::json!({"name": tool, "arguments": args});
        let res = {
            let sv = guard.as_mut().expect("live after ensure");
            sv.last_used = Instant::now();
            Self::request(sv, server, "tools/call", params, timeout).await
        };
        match res {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(msg)) => Err(tool_err(msg)),
            Err(e) => {
                Self::drop_live(&mut guard).await;
                Err(e)
            }
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

    #[test]
    fn a_listed_tool_keeps_its_schema_title_and_hints() {
        let t = McpTool::from_listing(
            "dd",
            &serde_json::json!({
                "name": "render",
                "description": " Draw it ",
                "inputSchema": {"type": "object", "properties": {"spec": {"type": "string"}}},
                "annotations": {"title": "Draw and audit", "readOnlyHint": false, "destructiveHint": true},
            }),
        );
        assert_eq!(t.qualified(), "dd.render");
        assert_eq!(t.title, "Draw and audit");
        assert_eq!(t.description, "Draw it");
        assert!(t.destructive && !t.read_only);
        assert_eq!(t.input_schema["properties"]["spec"]["type"], "string");
        let back: McpTool = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
        assert_eq!(back.input_schema, t.input_schema);
        assert_eq!(back.annotations, t.annotations);
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
