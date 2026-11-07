use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use parzi_core::config::McpServerCfg;
use parzi_core::error::{ParziError, Result};
use parzi_core::text::clip;
use parzi_providers::jsonrpc::{Incoming, Peer, RpcError};
use serde_json::{json, Value};
use tokio::process::{Child, Command};
use tokio::sync::mpsc::UnboundedReceiver;

/// The MCP version Parzi offers on both transports.
const PROTOCOL_VERSION: &str = "2025-06-18";
/// The versions a server may answer with. Parzi uses tools, pings, roots and
/// cancellation, which read the same in all of them.
const SUPPORTED: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_PAGES: usize = 100;
/// How long an error waits for the last of a stopped connector's stderr.
const STDERR_WAIT: Duration = Duration::from_millis(300);
const ROOTS_KEPT: usize = 8;

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

/// A connector run as a program, spoken to over its stdin and stdout.
struct Stdio {
    peer: Arc<Peer>,
    child: std::sync::Mutex<Option<Child>>,
    stderr: Arc<std::sync::Mutex<String>>,
    stderr_done: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    broken: AtomicBool,
}

impl Stdio {
    fn dead(&self) -> bool {
        self.peer.is_closed() || self.broken.load(Ordering::SeqCst)
    }

    /// The end of what the program wrote to stderr, once it has had a moment
    /// to finish writing.
    async fn tail(&self) -> String {
        let done = self.stderr_done.lock().ok().and_then(|mut d| d.take());
        if let Some(done) = done {
            let _ = tokio::time::timeout(STDERR_WAIT, done).await;
        }
        self.stderr.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

enum Conn {
    Stdio(Stdio),
    Http {
        conn: Arc<std::sync::Mutex<crate::mcp_http::HttpConn>>,
        next_id: AtomicU64,
    },
}

struct LiveServer {
    conn: Conn,
    info: Value,
    tools: std::sync::Mutex<Option<Vec<McpTool>>>,
    last_used: std::sync::Mutex<Instant>,
    busy: AtomicUsize,
    epoch: u64,
}

/// A call in flight: the idle reaper leaves the connector alone meanwhile.
struct Busy<'a>(&'a LiveServer);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.busy.fetch_sub(1, Ordering::SeqCst);
        self.0.touch();
    }
}

impl LiveServer {
    fn touch(&self) {
        if let Ok(mut t) = self.last_used.lock() {
            *t = Instant::now();
        }
    }

    fn idle_for(&self) -> Duration {
        self.last_used
            .lock()
            .map_or(Duration::ZERO, |t| t.elapsed())
    }

    fn busy(&self) -> Busy<'_> {
        self.busy.fetch_add(1, Ordering::SeqCst);
        Busy(self)
    }
}

type Slot = Arc<tokio::sync::Mutex<Option<Arc<LiveServer>>>>;

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
    /// Connectors whose last listing failed: their settings key, when, and why.
    failed: std::sync::Mutex<HashMap<String, (String, Instant, String)>>,
    settled: tokio::sync::watch::Sender<u64>,
    folders: std::sync::Mutex<Vec<String>>,
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
    // How this machine reaches the internet. A uvx or npx connector installs
    // itself on first start; behind a company proxy or a TLS-inspecting
    // firewall it fails without these, and the agent only sees a tool that
    // never answers. Unix tools read both spellings of the proxy names.
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "https_proxy",
    "http_proxy",
    "all_proxy",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "UV_NATIVE_TLS",
    // Where packages come from, and where installers keep their caches and
    // settings, so a company mirror is used and nothing installs twice.
    "NPM_CONFIG_REGISTRY",
    "npm_config_registry",
    "UV_INDEX_URL",
    "UV_DEFAULT_INDEX",
    "UV_EXTRA_INDEX_URL",
    "PIP_INDEX_URL",
    "UV_CACHE_DIR",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
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

/// An error that names the connector it came from.
fn conn_err(name: &str, msg: impl Into<String>) -> ParziError {
    ParziError::Tool(name.into(), msg.into())
}

/// `msg`, then the last thing the connector said on stderr: its last line on
/// the same line, a few before it below.
fn with_tail(msg: &str, tail: &str) -> String {
    let lines: Vec<&str> = tail
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect();
    let Some(last) = lines.last() else {
        return msg.to_string();
    };
    let recent = lines[lines.len().saturating_sub(8)..].join("\n");
    format!("{msg}: {}\n{}", clip(last.trim(), 300), clip(&recent, 1500))
}

fn file_uri(path: &str) -> String {
    let p = path.replace('\\', "/");
    let p = if p.starts_with('/') {
        p
    } else {
        format!("/{p}")
    };
    let mut out = String::from("file://");
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
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
    .map_err(|e| conn_err(&name, e.to_string()))?;
    out.map_err(|e| match e {
        crate::mcp_http::HttpError::SignIn(challenge) => {
            crate::mcp_http::remember_challenge(&name, challenge);
            conn_err(
                &name,
                format!(
                    "{}: sign in to {name} in Settings › Connections",
                    crate::mcp_http::SIGN_IN
                ),
            )
        }
        crate::mcp_http::HttpError::Other(e) => conn_err(&name, e),
    })
}

impl Conn {
    fn dead(&self) -> bool {
        match self {
            Self::Stdio(s) => s.dead(),
            Self::Http { .. } => false,
        }
    }

    /// One request. `Ok(Err(..))` is the connector refusing it; `Err` is the
    /// connector unreachable, stopped or too slow.
    async fn request(
        &self,
        name: &str,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<std::result::Result<Value, String>> {
        match self {
            Self::Stdio(s) => {
                let id = s.peer.fresh_id();
                match tokio::time::timeout(timeout, s.peer.request_as(id, method, params)).await {
                    Ok(Ok(v)) => Ok(Ok(v)),
                    Ok(Err(e)) if e.code == RpcError::CLOSED => {
                        s.broken.store(true, Ordering::SeqCst);
                        let tail = s.tail().await;
                        Err(conn_err(
                            name,
                            with_tail(&format!("{method}: it stopped before it answered"), &tail),
                        ))
                    }
                    Ok(Err(e)) => Ok(Err(format!("server error {}: {e}", e.code))),
                    Err(_) => {
                        // The server keeps running; it is told to drop the work.
                        if method != "initialize" {
                            let note = json!({"requestId": id, "reason": "Parzi stopped waiting"});
                            if s.peer
                                .notify("notifications/cancelled", note)
                                .await
                                .is_err()
                            {
                                s.broken.store(true, Ordering::SeqCst);
                            }
                        }
                        Err(conn_err(
                            name,
                            format!("{method} timed out after {}s", timeout.as_secs().max(1)),
                        ))
                    }
                }
            }
            Self::Http { conn, next_id } => {
                let id = next_id.fetch_add(1, Ordering::Relaxed) + 1;
                let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
                let reply = http_send(conn.clone(), name, msg, timeout)
                    .await?
                    .unwrap_or(Value::Null);
                Ok(match reply.get("error") {
                    Some(err) => Err(format!("server error: {err}")),
                    None => Ok(reply.get("result").cloned().unwrap_or(Value::Null)),
                })
            }
        }
    }

    async fn notify(&self, name: &str, method: &str, timeout: Duration) -> Result<()> {
        match self {
            Self::Stdio(s) => s.peer.notify(method, Value::Null).await.map_err(|e| {
                s.broken.store(true, Ordering::SeqCst);
                conn_err(name, e.to_string())
            }),
            Self::Http { conn, .. } => {
                let note = json!({"jsonrpc": "2.0", "method": method});
                http_send(conn.clone(), name, note, timeout)
                    .await
                    .map(|_| ())
            }
        }
    }

    async fn shut(&self) {
        let Self::Stdio(s) = self else {
            return;
        };
        let child = s.child.lock().ok().and_then(|mut c| c.take());
        if let Some(mut child) = child {
            if let Some(pid) = child.id() {
                let _ =
                    tokio::task::spawn_blocking(move || parzi_providers::process::kill_tree(pid))
                        .await;
            }
            let _ = child.kill().await;
        }
    }
}

/// `initialize`, then `notifications/initialized`; the server's own info.
async fn handshake(conn: &Conn, name: &str, timeout: Duration) -> Result<Value> {
    let capabilities = match conn {
        Conn::Stdio(_) => json!({"roots": {"listChanged": false}}),
        Conn::Http { .. } => json!({}),
    };
    let init = json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": capabilities,
        "clientInfo": {"name": "parzi", "version": env!("CARGO_PKG_VERSION")},
    });
    let res = conn
        .request(name, "initialize", init, timeout)
        .await?
        .map_err(|m| conn_err(name, m))?;
    let version = res
        .get("protocolVersion")
        .and_then(Value::as_str)
        .ok_or_else(|| conn_err(name, "initialize answered without protocolVersion"))?;
    if !SUPPORTED.contains(&version) {
        return Err(conn_err(
            name,
            format!(
                "it speaks MCP {version}, which Parzi does not (Parzi speaks {})",
                SUPPORTED.join(", ")
            ),
        ));
    }
    if version != PROTOCOL_VERSION {
        tracing::debug!(server = name, version, "mcp: server answered {version}");
    }
    if let Conn::Http { conn: h, .. } = conn {
        if let Ok(mut c) = h.lock() {
            c.set_protocol(version);
        }
    }
    conn.notify(name, "notifications/initialized", timeout)
        .await?;
    Ok(res.get("serverInfo").cloned().unwrap_or(Value::Null))
}

/// What a stdio connector asks or tells Parzi on its own.
async fn serve_incoming(
    inner: Weak<Inner>,
    name: String,
    peer: Arc<Peer>,
    mut incoming: UnboundedReceiver<Incoming>,
) {
    while let Some(msg) = incoming.recv().await {
        match msg {
            Incoming::Request { id, method, .. } => {
                let answered = match method.as_str() {
                    "ping" => peer.respond(id, json!({})).await,
                    "roots/list" => {
                        let roots = inner.upgrade().map(|i| i.roots()).unwrap_or_default();
                        if roots.is_empty() {
                            peer.respond_error(id, -32601, "no session folder is open")
                                .await
                        } else {
                            peer.respond(id, json!({"roots": roots})).await
                        }
                    }
                    other => {
                        tracing::debug!(server = %name, method = other, "connector asked for something Parzi does not offer");
                        peer.respond_error(id, -32601, &format!("Parzi does not offer {other}"))
                            .await
                    }
                };
                if answered.is_err() {
                    break;
                }
            }
            Incoming::Notification { method, params } => match method.as_str() {
                "notifications/tools/list_changed" => {
                    tracing::info!(server = %name, "connector changed its tools; listing again");
                    if let Some(i) = inner.upgrade() {
                        Inner::start_listing(&i, &name);
                    }
                }
                "notifications/message" => {
                    tracing::debug!(server = %name, "connector log: {}", clip(&params.to_string(), 500));
                }
                other => tracing::trace!(server = %name, method = other, "connector notification"),
            },
            Incoming::Raw(line) => {
                tracing::debug!(server = %name, "connector wrote a line that is not JSON: {}", clip(&line, 200));
            }
        }
    }
}

impl McpManager {
    /// A few lines for an agent's instructions on which connectors work
    /// right now, from what is already known: a weaker model that cannot
    /// see a connector's tools would otherwise rebuild them by hand or
    /// guess. None when no connector is on. Never starts a connector.
    #[must_use]
    pub fn status_note(&self) -> Option<String> {
        let configs: Vec<(String, McpServerCfg)> = {
            let c = self.inner.configs.read().ok()?;
            let mut v: Vec<_> = c
                .iter()
                .filter(|(_, cfg)| cfg.enabled)
                .map(|(n, cfg)| (n.clone(), cfg.clone()))
                .collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        };
        if configs.is_empty() {
            return None;
        }
        let failed = self
            .inner
            .failed
            .lock()
            .ok()
            .map(|f| f.clone())
            .unwrap_or_default();
        let mut ready = vec![];
        let mut trouble = vec![];
        for (name, cfg) in &configs {
            let key = crate::mcp_cache::key(name, cfg);
            if let Some((_, _, why)) = failed.get(name).filter(|(k, _, _)| *k == key) {
                let why = why.lines().next().unwrap_or_default().trim();
                trouble.push(format!(
                    "- {name} is not working: {}",
                    parzi_core::text::clip(why, 160)
                ));
                continue;
            }
            match self.inner.cached(name, cfg) {
                Some((tools, _)) => {
                    let n = tools
                        .iter()
                        .filter(|t| cfg.is_tool_exposed(&t.name))
                        .count();
                    if n > 0 {
                        ready.push(format!("{name} ({n} tools)"));
                    }
                }
                None => trouble.push(format!(
                    "- {name} is still starting; its tools may appear on a later turn"
                )),
            }
        }
        let mut out = String::from(
            "# Connectors\n\nTools from the person's connectors come through the parzi server, named <connector>__<tool>.",
        );
        if !ready.is_empty() {
            out.push_str(&format!("\nWorking: {}.", ready.join(", ")));
        }
        if !trouble.is_empty() {
            out.push('\n');
            out.push_str(&trouble.join("\n"));
            out.push_str("\nWhen a task needs one that is not working, say so plainly so the person can fix it in Settings, Connectors. Do not rebuild what it does by hand.");
        }
        Some(out)
    }

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
                folders: std::sync::Mutex::default(),
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

    /// A folder a session works in: what a connector's `roots/list` answers.
    pub fn note_folder(&self, folder: &str) {
        let folder = folder.trim();
        if folder.is_empty() {
            return;
        }
        let scratch = parzi_core::paths::parzi_dir()
            .map(|d| d.join("scratch"))
            .ok();
        if scratch.is_some_and(|s| std::path::Path::new(folder).starts_with(s)) {
            return;
        }
        if let Ok(mut f) = self.inner.folders.lock() {
            f.retain(|x| x != folder);
            f.insert(0, folder.to_string());
            f.truncate(ROOTS_KEPT);
        }
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
        let info = self.inner.live(name).await?.info.clone();
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
                    (false, conn_err(server, text).to_string())
                }
            }
            Err(e) => (false, e.to_string()),
        }
    }

    /// A `tools/call` as the connector answered it, `isError` and all; `Err`
    /// when the connector could not be reached or broke the protocol.
    pub async fn call_raw(&self, server: &str, tool: &str, args: Value) -> Result<Value> {
        if !self.is_tool_exposed(server, tool) {
            return Err(conn_err(
                server,
                format!("`{tool}` is switched off for this connector"),
            ));
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

    fn roots(&self) -> Vec<Value> {
        self.folders
            .lock()
            .map(|f| {
                f.iter()
                    .map(|p| {
                        let name = std::path::Path::new(p)
                            .file_name()
                            .map_or_else(|| p.clone(), |n| n.to_string_lossy().into_owned());
                        json!({"uri": file_uri(p), "name": name})
                    })
                    .collect()
            })
            .unwrap_or_default()
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
                .is_some_and(|(k, at, _)| *k == key && at.elapsed() < FAIL_BACKOFF)
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
                    f.insert(name, (key, Instant::now(), e.to_string()));
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

    /// The live connection to `name`, started or reconnected if need be.
    /// Only starting holds the connector's slot; calls to a live one overlap.
    async fn live(self: &Arc<Self>, name: &str) -> Result<Arc<LiveServer>> {
        let slot = self.slot_for(name).await;
        let mut guard = slot.lock().await;
        if let Some(sv) = guard.as_ref() {
            if !sv.conn.dead() {
                sv.touch();
                return Ok(sv.clone());
            }
            Self::drop_live(&mut guard).await;
        }
        let sv = Arc::new(self.connect(name).await?);
        *guard = Some(sv.clone());
        Self::spawn_reaper(name.to_string(), slot.clone(), self.idle_kill, sv.epoch);
        Ok(sv)
    }

    async fn connect(self: &Arc<Self>, name: &str) -> Result<LiveServer> {
        let cfg = self
            .config_for(name)
            .ok_or_else(|| conn_err(name, "no such connector"))?;
        if !cfg.enabled {
            return Err(conn_err(name, "the connector is switched off"));
        }
        self.starts.fetch_add(1, Ordering::Relaxed);
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        let timeout = Duration::from_millis(cfg.timeout_ms.max(1_000));
        let wait = timeout.max(HANDSHAKE_FLOOR);
        let conn = if cfg.url.trim().is_empty() {
            self.spawn_stdio(name, &cfg)?
        } else {
            Conn::Http {
                conn: Arc::new(std::sync::Mutex::new(crate::mcp_http::HttpConn::new(
                    name, &cfg,
                ))),
                next_id: AtomicU64::new(0),
            }
        };
        let info = match handshake(&conn, name, wait).await {
            Ok(info) => info,
            Err(e) => {
                conn.shut().await;
                return Err(e);
            }
        };
        Ok(LiveServer {
            conn,
            info,
            tools: std::sync::Mutex::new(None),
            last_used: std::sync::Mutex::new(Instant::now()),
            busy: AtomicUsize::new(0),
            epoch,
        })
    }

    fn spawn_stdio(self: &Arc<Self>, name: &str, cfg: &McpServerCfg) -> Result<Conn> {
        let program = parzi_providers::process::which(&cfg.command).ok_or_else(|| {
            conn_err(
                name,
                format!("`{}` isn't installed on this computer", cfg.command),
            )
        })?;
        let mut cmd = Command::new(program);
        cmd.args(&cfg.args)
            .env_clear()
            .envs(child_env(&cfg.env))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
        let mut child: Child = cmd
            .spawn()
            .map_err(|e| conn_err(name, format!("could not start `{}`: {e}", cfg.command)))?;
        parzi_providers::process::adopt(&child);
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(conn_err(name, "its pipes did not open"));
        };
        let (stderr, done) = match child.stderr.take() {
            Some(pipe) => parzi_providers::process::keep_tail(pipe),
            None => (Arc::default(), tokio::spawn(async {})),
        };
        let (peer, incoming) = Peer::start(name.to_string(), stdout, stdin);
        tokio::spawn(serve_incoming(
            Arc::downgrade(self),
            name.to_string(),
            peer.clone(),
            incoming,
        ));
        Ok(Conn::Stdio(Stdio {
            peer,
            child: std::sync::Mutex::new(Some(child)),
            stderr,
            stderr_done: std::sync::Mutex::new(Some(done)),
            broken: AtomicBool::new(false),
        }))
    }

    fn spawn_reaper(name: String, slot: Slot, idle: Duration, epoch: u64) {
        let tick = std::cmp::max(idle / 2, Duration::from_secs(1));
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tick).await;
                let mut guard = slot.lock().await;
                match guard.as_ref() {
                    Some(sv) if sv.epoch == epoch => {
                        let gone = sv.conn.dead();
                        let idle_now = sv.busy.load(Ordering::SeqCst) == 0 && sv.idle_for() >= idle;
                        if gone || idle_now {
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

    async fn drop_live(guard: &mut Option<Arc<LiveServer>>) {
        if let Some(sv) = guard.take() {
            sv.conn.shut().await;
        }
    }

    fn timeout_for(&self, name: &str) -> Duration {
        Duration::from_millis(
            self.config_for(name)
                .map_or(30_000, |c| c.timeout_ms)
                .max(1_000),
        )
    }

    /// The connector's tools, every page of them; `fresh` asks again even
    /// when this connection listed them already.
    async fn list_tools(self: &Arc<Self>, name: &str, fresh: bool) -> Result<Vec<McpTool>> {
        let sv = self.live(name).await?;
        if !fresh {
            if let Some(t) = sv.tools.lock().ok().and_then(|t| t.clone()) {
                return Ok(t);
            }
        }
        let _busy = sv.busy();
        let timeout = self.timeout_for(name);
        let mut out: Vec<McpTool> = vec![];
        let mut cursor: Option<String> = None;
        let mut seen = HashSet::new();
        for _ in 0..MAX_PAGES {
            let params = cursor
                .as_ref()
                .map_or_else(|| json!({}), |c| json!({"cursor": c}));
            let page = sv
                .conn
                .request(name, "tools/list", params, timeout)
                .await?
                .map_err(|m| conn_err(name, m))?;
            out.extend(
                page.get("tools")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|t| McpTool::from_listing(name, t)),
            );
            match page.get("nextCursor").and_then(Value::as_str) {
                Some(c) if !c.is_empty() && seen.insert(c.to_string()) => {
                    cursor = Some(c.to_string());
                }
                _ => break,
            }
        }
        if let Ok(mut t) = sv.tools.lock() {
            *t = Some(out.clone());
        }
        self.remember(name, &out);
        Ok(out)
    }

    async fn call(self: &Arc<Self>, server: &str, tool: &str, args: Value) -> Result<Value> {
        let sv = self.live(server).await?;
        let _busy = sv.busy();
        let params = json!({"name": tool, "arguments": args});
        sv.conn
            .request(server, "tools/call", params, self.timeout_for(server))
            .await?
            .map_err(|m| conn_err(server, m))
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
    fn the_connector_note_says_what_works_and_what_does_not() {
        let mut servers = HashMap::new();
        for name in ["draw", "notes", "broken", "off"] {
            servers.insert(
                name.to_string(),
                McpServerCfg {
                    command: format!("{name}-mcp"),
                    enabled: name != "off",
                    ..McpServerCfg::default()
                },
            );
        }
        let m = McpManager::new(servers, 60);
        m.inner.remember(
            "draw",
            &[McpTool {
                server: "draw".into(),
                name: "render".into(),
                ..McpTool::default()
            }],
        );
        let key = crate::mcp_cache::key("broken", &m.inner.config_for("broken").unwrap());
        m.inner.failed.lock().unwrap().insert(
            "broken".into(),
            (
                key,
                Instant::now(),
                "initialize: server closed the pipe\nmore".into(),
            ),
        );
        let note = m.status_note().unwrap();
        assert!(note.contains("Working: draw (1 tools)."), "{note}");
        assert!(
            note.contains("- broken is not working: initialize: server closed the pipe\n"),
            "{note}"
        );
        assert!(note.contains("- notes is still starting"), "{note}");
        assert!(
            !note.contains("off"),
            "a connector that is off is not mentioned: {note}"
        );
        assert!(
            note.contains("Do not rebuild what it does by hand"),
            "{note}"
        );
        assert!(McpManager::new(HashMap::new(), 60).status_note().is_none());
    }

    #[test]
    fn a_connector_reaches_the_internet_the_way_this_machine_does() {
        let parent = map(&[
            ("PATH", "/usr/bin"),
            ("HTTPS_PROXY", "http://proxy.corp:8080"),
            ("no_proxy", "localhost,.corp"),
            ("SSL_CERT_FILE", "/etc/corp/ca.pem"),
            ("NODE_EXTRA_CA_CERTS", "/etc/corp/ca.pem"),
            ("UV_INDEX_URL", "https://pypi.corp/simple"),
            ("OPENAI_API_KEY", "sk-secret"),
        ]);
        let env = child_env_from(&parent, &HashMap::new());
        for k in [
            "HTTPS_PROXY",
            "no_proxy",
            "SSL_CERT_FILE",
            "NODE_EXTRA_CA_CERTS",
            "UV_INDEX_URL",
        ] {
            assert_eq!(env.get(k), parent.get(k), "{k} reaches the connector");
        }
        assert!(
            !env.contains_key("OPENAI_API_KEY"),
            "keys still stay behind"
        );
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

    #[test]
    fn an_error_carries_the_last_of_stderr() {
        let tail =
            "starting\n\nTraceback (most recent call last):\n  File x\nKeyError: 'API_KEY'\n";
        let got = with_tail("initialize: it stopped before it answered", tail);
        let first = got.lines().next().unwrap();
        assert_eq!(
            first,
            "initialize: it stopped before it answered: KeyError: 'API_KEY'"
        );
        assert!(got.contains("Traceback"), "{got}");
        assert_eq!(with_tail("quiet", "  \n"), "quiet");
        let shown = conn_err("diagonaldiagrams", got).to_string();
        assert!(shown.starts_with("tool `diagonaldiagrams`: "), "{shown}");
    }

    #[test]
    fn folders_read_as_file_uris() {
        assert_eq!(file_uri("/home/u/my app"), "file:///home/u/my%20app");
        assert_eq!(file_uri(r"C:\Repos\x"), "file:///C:/Repos/x");
    }

    #[test]
    fn the_offered_version_is_one_parzi_accepts() {
        assert!(SUPPORTED.contains(&PROTOCOL_VERSION));
        assert!(!SUPPORTED.contains(&"2099-01-01"));
    }
}
