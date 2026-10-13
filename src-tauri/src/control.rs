//! The window's control socket. One JSON line per connection on 127.0.0.1,
//! authorized by the token in `~/.parzi/gui.json`; the CLI and the agent
//! tools both come through here. The token is the person's authority, so
//! what a token alone must not do (run an agent unasked, delete a session,
//! read or drive a page tab no agent owns) waits for the person to allow
//! it in a dialog the window shows.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use parzi_core::store::RunCause;
use parzi_runtime::desk::{check_page_url, GuiFile};
use parzi_runtime::handler::{ui_event, RunEvent};
use parzi_runtime::tools::{humanize_tool_call, Approval, Approver, AutoApprover, ToolCallInfo};
use parzi_runtime::Orchestrator;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogResult};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};

use crate::{AppState, GuiApprover, Pending};

const MAX_REQUEST: u64 = 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CLIENTS: usize = 64;
/// Streams (`subscribe`, a running `session.send`) have their own pool, so
/// watchers can never starve plain requests.
const MAX_STREAMS: usize = 16;
/// How long a tab the window was told to open may be missing from its
/// syncs before it counts as gone.
const PENDING_FOR: Duration = Duration::from_secs(15);
const CONSENT_FOR: Duration = Duration::from_secs(10 * 60);
const KEEPALIVE: Duration = Duration::from_secs(30);
const SETTINGS: &[&str] = &[
    "general",
    "providers",
    "connections",
    "appearance",
    "system",
];
const HISTORY: &[&str] = &["sessions", "history", "bookmarks", "downloads"];
const NO_TAB: &str = "this session has no page tab yet: open one with browser.open";
const ALLOW_A_WHILE: &str = "Allow for 10 minutes";
const ALLOW_ONCE: &str = "Allow once";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabSnap {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub url: String,
    /// The session a session tab shows, or the agent that owns a page tab.
    #[serde(default)]
    pub session_id: String,
    /// A draft session tab's folder.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cwd: String,
}

impl TabSnap {
    fn page(&self) -> bool {
        self.kind == "browser"
    }
}

/// A tab the window was told to open and has not synced back yet.
struct Opened {
    rev: u64,
    tab: TabSnap,
    at: Instant,
}

#[derive(Default)]
struct DeskState {
    rev: u64,
    tabs: Vec<TabSnap>,
    active: String,
    pending: Vec<Opened>,
    /// The page tab each session used last.
    current: HashMap<String, String>,
}

pub struct Desk {
    state: Mutex<DeskState>,
    token: String,
    instance: String,
    started: u64,
    /// Approvals, tab changes and notices, for `subscribe` streams.
    events: broadcast::Sender<Value>,
    approvals: Mutex<BTreeMap<String, Value>>,
    /// Page tabs the person allowed for a while: tab id, until when.
    consents: Mutex<HashMap<String, Instant>>,
    /// One dialog at a time.
    dialogs: tokio::sync::Mutex<()>,
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Default for Desk {
    fn default() -> Self {
        Self::new()
    }
}

impl Desk {
    #[must_use]
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(512);
        Self {
            state: Mutex::default(),
            token: uuid::Uuid::new_v4().simple().to_string(),
            instance: uuid::Uuid::new_v4().simple().to_string(),
            started: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            events,
            approvals: Mutex::default(),
            consents: Mutex::default(),
            dialogs: tokio::sync::Mutex::new(()),
        }
    }

    fn file(&self, port: u16) -> GuiFile {
        GuiFile {
            port,
            token: self.token.clone(),
            pid: std::process::id(),
            started: self.started,
            instance: self.instance.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    /// The window's own list wins, except for tabs it was just told to open
    /// and has not shown yet: a sync racing an open must not drop them. A
    /// window that reloaded starts again from rev 0 and is still heard.
    pub fn sync(&self, tabs: Vec<TabSnap>, rev: u64, active: String) {
        let mut st = locked(&self.state);
        st.pending.retain(|p| {
            p.rev > rev && p.at.elapsed() < PENDING_FOR && !tabs.iter().any(|t| t.id == p.tab.id)
        });
        let mut next = tabs;
        for p in &st.pending {
            next.push(p.tab.clone());
        }
        let changed = next != st.tabs || active != st.active;
        st.rev = st.rev.max(rev);
        st.tabs = next;
        st.active = active;
        let alive: Vec<String> = st.tabs.iter().map(|t| t.id.clone()).collect();
        st.current.retain(|_, id| alive.contains(id));
        if changed {
            let _ = self.events.send(json!({
                "kind": "tabs",
                "active": st.active,
                "tabs": st.tabs,
            }));
        }
    }

    /// Records a tab the window is about to open; returns the rev it carries.
    fn record_open(&self, tab: TabSnap, focus: bool) -> u64 {
        let mut st = locked(&self.state);
        st.rev += 1;
        let rev = st.rev;
        if focus {
            st.active.clone_from(&tab.id);
        }
        if !tab.session_id.is_empty() {
            st.current.insert(tab.session_id.clone(), tab.id.clone());
        }
        st.tabs.push(tab.clone());
        st.pending.push(Opened {
            rev,
            tab,
            at: Instant::now(),
        });
        rev
    }

    fn page_with_url(&self, url: &str) -> Option<String> {
        locked(&self.state)
            .tabs
            .iter()
            .find(|t| t.page() && t.url == url)
            .map(|t| t.id.clone())
    }

    fn set_active(&self, id: &str) -> bool {
        let mut st = locked(&self.state);
        if !st.tabs.iter().any(|t| t.id == id) {
            return false;
        }
        st.active = id.to_string();
        true
    }

    fn navigated(&self, id: &str, url: &str, session: &str) {
        let mut st = locked(&self.state);
        if let Some(t) = st.tabs.iter_mut().find(|t| t.id == id) {
            t.url = url.to_string();
        }
        st.current.insert(session.to_string(), id.to_string());
    }

    fn used(&self, session: &str, id: &str) {
        if !session.is_empty() {
            locked(&self.state)
                .current
                .insert(session.to_string(), id.to_string());
        }
    }

    /// Removes a tab. With a session, only one of its own.
    fn close(&self, id: &str, session: &str) -> Result<(), String> {
        let mut st = locked(&self.state);
        let Some(tab) = st.tabs.iter().find(|t| t.id == id) else {
            return Err(format!("no tab `{id}`"));
        };
        if !session.is_empty() && tab.session_id != session {
            return Err(format!("tab `{id}` is not one of yours"));
        }
        st.tabs.retain(|t| t.id != id);
        st.pending.retain(|p| p.tab.id != id);
        st.current.retain(|_, t| t != id);
        if st.active == id {
            st.active.clear();
        }
        Ok(())
    }

    /// The page a request is about, and whether the person must allow it.
    /// An agent names one of its tabs or `active` (the person's own), else
    /// gets the tab it used last, else its newest. The CLI names a tab or
    /// gets the one in front. Consent: a tab no agent owns, or another
    /// agent's.
    fn find_page(&self, session: &str, tab: &str) -> Result<(TabSnap, bool), String> {
        let st = locked(&self.state);
        let page = |id: &str| st.tabs.iter().find(|t| t.id == id && t.page()).cloned();
        let picked = if tab == "active" || (tab.is_empty() && session.is_empty()) {
            page(&st.active).ok_or_else(|| {
                "the tab in front is not a page: name one with its tab id".to_string()
            })?
        } else if !tab.is_empty() {
            page(tab).ok_or_else(|| format!("no page tab `{tab}`"))?
        } else {
            st.current
                .get(session)
                .and_then(|id| page(id))
                .filter(|t| t.session_id == session)
                .or_else(|| {
                    st.tabs
                        .iter()
                        .rev()
                        .find(|t| t.page() && t.session_id == session)
                        .cloned()
                })
                .ok_or_else(|| NO_TAB.to_string())?
        };
        let foreign =
            picked.session_id.is_empty() || (!session.is_empty() && picked.session_id != session);
        Ok((picked, foreign))
    }

    /// An agent's tab to navigate: the one it names, else its last one.
    fn own_tab(&self, session: &str, tab: &str) -> Result<Option<TabSnap>, String> {
        if tab.is_empty() {
            return Ok(self.find_page(session, "").ok().map(|(t, _)| t));
        }
        match self.find_page(session, tab)? {
            (t, false) => Ok(Some(t)),
            _ => Err(format!(
                "tab `{tab}` is not one of yours: open your own with new=true"
            )),
        }
    }

    /// The CLI sees every tab. An agent sees its own page tabs and only
    /// the title of the one in front, unless `all` (which the agent's
    /// approval card covered).
    fn tab_list(&self, session: &str, all: bool) -> Value {
        let st = locked(&self.state);
        let active_title = st
            .tabs
            .iter()
            .find(|t| t.id == st.active)
            .map(|t| t.title.clone());
        if session.is_empty() || all {
            return json!({
                "ok": true,
                "tabs": st.tabs,
                "active": st.active,
                "active_title": active_title,
            });
        }
        let own: Vec<&TabSnap> = st
            .tabs
            .iter()
            .filter(|t| t.page() && t.session_id == session)
            .collect();
        json!({ "ok": true, "tabs": own, "active_title": active_title })
    }

    fn snapshot(&self) -> (Vec<TabSnap>, String) {
        let st = locked(&self.state);
        (st.tabs.clone(), st.active.clone())
    }

    fn hello(&self) -> Value {
        json!({
            "ok": true,
            "app": "parzi",
            "instance": self.instance,
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
        })
    }

    pub fn approval_opened(&self, key: &str, call: &ToolCallInfo) {
        let row = json!({
            "key": key,
            "session": call.session,
            "name": call.name,
            "lane": call.lane,
            "label": humanize_tool_call(&call.name, &call.args),
        });
        locked(&self.approvals).insert(key.to_string(), row.clone());
        let mut event = row;
        event["kind"] = json!("approval");
        let _ = self.events.send(event);
    }

    pub fn approval_closed(&self, key: &str) {
        let row = locked(&self.approvals).remove(key);
        if let Some(row) = row {
            let _ = self.events.send(json!({
                "kind": "approval_done",
                "key": key,
                "session": row.get("session").cloned().unwrap_or_default(),
            }));
        }
    }

    fn approvals(&self) -> Vec<Value> {
        locked(&self.approvals).values().cloned().collect()
    }

    /// A full key, or a prefix only one waiting approval has.
    fn approval_key(&self, asked: &str) -> Result<(String, Value), String> {
        let rows = locked(&self.approvals);
        if let Some(row) = rows.get(asked) {
            return Ok((asked.to_string(), row.clone()));
        }
        let hits: Vec<_> = rows
            .iter()
            .filter(|(k, _)| asked.len() >= 4 && k.starts_with(asked))
            .collect();
        match hits.as_slice() {
            [(k, row)] => Ok(((*k).clone(), (*row).clone())),
            [] => Err("approval expired or unknown".into()),
            _ => Err("that prefix fits more than one approval".into()),
        }
    }

    fn consented(&self, tab: &str) -> bool {
        let mut map = locked(&self.consents);
        map.retain(|_, until| *until > Instant::now());
        map.contains_key(tab)
    }

    fn allow_for_a_while(&self, tab: &str) {
        locked(&self.consents).insert(tab.to_string(), Instant::now() + CONSENT_FOR);
    }
}

#[tauri::command]
pub async fn desk_sync(
    state: State<'_, AppState>,
    tabs: Vec<TabSnap>,
    rev: u64,
    active: String,
) -> Result<(), String> {
    state.desk.sync(tabs, rev, active);
    Ok(())
}

static WRITTEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// On exit: the file goes only if it is still this window's.
pub fn clear_gui_file() {
    let Some(path) = parzi_runtime::desk::gui_path() else {
        return;
    };
    match WRITTEN.get() {
        Some(mine) if std::fs::read_to_string(&path).ok().as_ref() != Some(mine) => {}
        _ => {
            let _ = std::fs::remove_file(path);
        }
    }
}

struct Ctx {
    app: AppHandle,
    orch: Arc<Orchestrator>,
    pending: Pending,
    desk: Arc<Desk>,
}

pub async fn serve(app: AppHandle, orch: Arc<Orchestrator>, pending: Pending, desk: Arc<Desk>) {
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", 0)).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::warn!("desk control did not bind: {e}");
            return;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    if let Some(path) = parzi_runtime::desk::gui_path() {
        let body = serde_json::to_string(&desk.file(port)).unwrap_or_default();
        let _ = WRITTEN.set(body.clone());
        let written =
            tauri::async_runtime::spawn_blocking(move || write_private(&path, &body)).await;
        if let Ok(Err(e)) = written {
            tracing::warn!("desk control file not written: {e}");
        }
    }
    parzi_runtime::desk::attach();
    let ctx = Arc::new(Ctx {
        app,
        orch,
        pending,
        desk,
    });
    let slots = Arc::new(Semaphore::new(MAX_CLIENTS));
    let streams = Arc::new(Semaphore::new(MAX_STREAMS));
    loop {
        let (sock, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                // Persistent errors (fd exhaustion) would otherwise spin.
                tracing::debug!("desk control accept: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        let (ctx, streams) = (ctx.clone(), streams.clone());
        // Full means a quick "busy", never an accept loop that waits.
        let slot = slots.clone().try_acquire_owned();
        tokio::spawn(async move {
            let _ = match slot {
                Ok(slot) => answer(sock, &ctx, slot, &streams).await,
                Err(_) => busy(sock).await,
            };
        });
    }
}

async fn busy(sock: TcpStream) -> Result<(), String> {
    let (read, mut write) = sock.into_split();
    // Read the request first: closing on unread bytes resets the
    // connection, and the client would lose this answer.
    let mut sink = String::new();
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        BufReader::new(read.take(MAX_REQUEST)).read_line(&mut sink),
    )
    .await;
    let line = "{\"ok\":false,\"error\":\"Parzi is busy with other requests; try again\"}\n";
    tokio::time::timeout(IO_TIMEOUT, write.write_all(line.as_bytes()))
        .await
        .map_err(|_| "reply timed out".to_string())?
        .map_err(|e| e.to_string())
}

/// gui.json carries the bridge token: born owner-only (0600 on unix; on
/// Windows inheritance cut and the file granted to this account alone) and
/// swapped in whole, never half-written.
fn write_private(path: &std::path::Path, body: &str) -> Result<(), String> {
    let tmp = path.with_file_name(format!(".gui.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        file.write_all(body.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        restrict_to_owner(&tmp, body);
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Best effort: icacls with this account's SID. If the file cannot be read
/// back afterwards it is written again with the folder's own ACL.
#[cfg(windows)]
fn restrict_to_owner(path: &std::path::Path, body: &str) {
    use std::os::windows::process::CommandExt as _;
    const NO_WINDOW: u32 = 0x0800_0000;
    let sid = std::process::Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .creation_flags(NO_WINDOW)
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .find(|s| s.starts_with("S-1-"))
        });
    let Some(sid) = sid else {
        return;
    };
    let done = std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r", &format!("*{sid}:F")])
        .creation_flags(NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !done || std::fs::read_to_string(path).ok().as_deref() != Some(body) {
        tracing::warn!("gui.json keeps its folder's permissions");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::write(path, body);
    }
}

/// Constant-time: the reply time must not reveal how much of a guess matched.
fn tokens_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

enum Gate {
    Reply(Value),
    Run(Value),
}

/// What a request line gets before any op runs: `hello` is answered for
/// anyone (it only names this window), everything else needs the token.
fn gate_line(desk: &Desk, line: &str) -> Gate {
    if line.len() as u64 >= MAX_REQUEST {
        return Gate::Reply(fail("request too large"));
    }
    let req: Value = serde_json::from_str(line.trim()).unwrap_or_else(|_| json!({}));
    if req.get("op").and_then(Value::as_str) == Some("hello") {
        return Gate::Reply(desk.hello());
    }
    let token = req.get("token").and_then(Value::as_str).unwrap_or("");
    if tokens_equal(token, &desk.token) {
        Gate::Run(req)
    } else {
        Gate::Reply(fail("bad token"))
    }
}

/// The write half of one connection. Progress lines (no `ok` field) go out
/// only to clients that asked for them; old clients read one line.
struct Out {
    write: tokio::sync::Mutex<OwnedWriteHalf>,
    progress: bool,
}

impl Out {
    async fn line(&self, v: &Value) -> Result<(), String> {
        let mut text = serde_json::to_string(v)
            .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"encode\"}".into());
        text.push('\n');
        let mut write = self.write.lock().await;
        tokio::time::timeout(IO_TIMEOUT, write.write_all(text.as_bytes()))
            .await
            .map_err(|_| "reply timed out".to_string())?
            .map_err(|e| e.to_string())
    }

    async fn note(&self, v: Value) {
        if self.progress {
            let _ = self.line(&v).await;
        }
    }
}

async fn answer(
    sock: TcpStream,
    ctx: &Ctx,
    slot: OwnedSemaphorePermit,
    streams: &Arc<Semaphore>,
) -> Result<(), String> {
    let (read, write) = sock.into_split();
    let mut line = String::new();
    // A client that connects and never finishes its line must not hold a slot.
    tokio::time::timeout(
        IO_TIMEOUT,
        BufReader::new(read.take(MAX_REQUEST)).read_line(&mut line),
    )
    .await
    .map_err(|_| "request timed out".to_string())?
    .map_err(|e| e.to_string())?;
    let req = match gate_line(&ctx.desk, &line) {
        Gate::Reply(v) => {
            let out = Out {
                write: tokio::sync::Mutex::new(write),
                progress: false,
            };
            return out.line(&v).await;
        }
        Gate::Run(req) => req,
    };
    let out = Out {
        write: tokio::sync::Mutex::new(write),
        progress: bool_arg(&req, "progress", false),
    };
    let op = req.get("op").and_then(Value::as_str).unwrap_or("");
    if op == "subscribe" || op == "session.send" {
        let Ok(stream) = streams.clone().try_acquire_owned() else {
            return out
                .line(&fail("Parzi is busy with other streams; try again"))
                .await;
        };
        drop(slot);
        let done = if op == "subscribe" {
            subscribe(ctx, &out, &req).await
        } else {
            let v = session_send(ctx, &out, &req).await;
            out.line(&v).await
        };
        drop(stream);
        return done;
    }
    let reply = dispatch(ctx, &out, &req).await;
    drop(slot);
    out.line(&reply).await
}

async fn dispatch(ctx: &Ctx, out: &Out, req: &Value) -> Value {
    let op = req.get("op").and_then(Value::as_str).unwrap_or("");
    match op {
        "ping" => json!({ "ok": true, "instance": ctx.desk.instance }),
        "desk.state" => desk_state(ctx, req).await,
        "desk.open" => desk_open(ctx, req).await,
        "desk.notify" => desk_notify(ctx, req),
        "desk.screenshot" => desk_screenshot(ctx).await,
        "tab.list" => ctx
            .desk
            .tab_list(&str_arg(req, "session"), bool_arg(req, "all", false)),
        "tab.open" => tab_open(ctx, req).await,
        "tab.focus" => tab_focus(ctx, req),
        "tab.close" => tab_close(ctx, req),
        "tab.read" | "tab.click" | "tab.type" | "tab.press" | "tab.select" | "tab.scroll"
        | "tab.wait" | "tab.shot" => page_op(ctx, out, req, op).await,
        "approval.list" => json!({ "ok": true, "approvals": ctx.desk.approvals() }),
        "approval.answer" => approval_answer(ctx, req).await,
        "providers" => providers(ctx, req).await,
        "session.list" => session_list(&ctx.orch),
        "session.show" | "session.export" => session_show(&ctx.orch, req, op == "session.export"),
        "session.kill" => session_kill(&ctx.orch, req).await,
        "session.fork" => session_fork(&ctx.orch, req),
        "session.rename" => session_rename(&ctx.orch, req),
        "session.delete" => session_delete(ctx, out, req).await,
        _ => fail(format!("unknown op `{op}`")),
    }
}

fn fail(error: impl Into<String>) -> Value {
    json!({ "ok": false, "error": error.into() })
}

fn str_arg(req: &Value, key: &str) -> String {
    req.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn bool_arg(req: &Value, key: &str, dflt: bool) -> bool {
    req.get(key).and_then(Value::as_bool).unwrap_or(dflt)
}

fn raise(app: &AppHandle) {
    if let Some(w) = app.get_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn host_of(url: &str) -> String {
    let bare = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    bare.split(['/', '?', '#']).next().unwrap_or("").to_string()
}

fn short_title(url: &str) -> String {
    host_of(url).chars().take(24).collect()
}

impl Ctx {
    fn tell(&self, payload: Value) {
        let _ = self.app.emit("parzi://desk", payload);
    }

    /// A native dialog over the window, which comes forward for it.
    async fn ask_person(
        &self,
        title: &str,
        text: String,
        buttons: MessageDialogButtons,
    ) -> MessageDialogResult {
        let _one = self.desk.dialogs.lock().await;
        let (app, title) = (self.app.clone(), title.to_string());
        tauri::async_runtime::spawn_blocking(move || {
            use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
            let mut dialog = app
                .dialog()
                .message(text)
                .title(title)
                .kind(MessageDialogKind::Warning)
                .buttons(buttons);
            if let Some(w) = app.get_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
                dialog = dialog.parent(&w);
            }
            dialog.blocking_show_with_result()
        })
        .await
        .unwrap_or(MessageDialogResult::Cancel)
    }

    async fn confirm(&self, out: &Out, title: &str, text: String, ok: &str) -> bool {
        out.note(json!({
            "kind": "confirm",
            "text": format!("waiting for you to answer in the Parzi window: {title}"),
        }))
        .await;
        let answer = self
            .ask_person(
                title,
                text,
                MessageDialogButtons::OkCancelCustom(ok.into(), "Cancel".into()),
            )
            .await;
        matches!(answer, MessageDialogResult::Custom(ref s) if s == ok)
            || answer == MessageDialogResult::Ok
    }

    /// A page tab no agent owns is the person's browsing, signed in where
    /// they are: reading or driving it waits for them to allow it.
    async fn consent_page(&self, out: &Out, tab: &TabSnap, verb: &str) -> bool {
        if self.desk.consented(&tab.id) {
            return true;
        }
        let title: String = tab.title.chars().take(80).collect();
        out.note(json!({
            "kind": "confirm",
            "text": format!("waiting for you to allow this in the Parzi window: {verb} \"{title}\""),
        }))
        .await;
        let text = format!(
            "A command asks to {verb} your page \"{title}\" ({}).\n\nIt sees what the page shows, including anything you are signed in to there. Allow it only if you started this yourself.",
            host_of(&tab.url)
        );
        let answer = self
            .ask_person(
                "Allow access to your page?",
                text,
                MessageDialogButtons::YesNoCancelCustom(
                    ALLOW_A_WHILE.into(),
                    ALLOW_ONCE.into(),
                    "Deny".into(),
                ),
            )
            .await;
        match answer {
            MessageDialogResult::Custom(s) if s == ALLOW_A_WHILE => {
                self.desk.allow_for_a_while(&tab.id);
                true
            }
            MessageDialogResult::Custom(s) => s == ALLOW_ONCE,
            MessageDialogResult::Yes => {
                self.desk.allow_for_a_while(&tab.id);
                true
            }
            MessageDialogResult::No => true,
            _ => false,
        }
    }
}

async fn page_op(ctx: &Ctx, out: &Out, req: &Value, op: &str) -> Value {
    let session = str_arg(req, "session");
    let (tab, foreign) = match ctx.desk.find_page(&session, &str_arg(req, "tab")) {
        Ok(found) => found,
        Err(e) => return fail(e),
    };
    let verb = match op {
        "tab.read" | "tab.wait" => "read",
        "tab.shot" => "take a screenshot of",
        _ => "click and type in",
    };
    if foreign && !ctx.consent_page(out, &tab, verb).await {
        return fail("not allowed: the person declined in the Parzi window");
    }
    if !foreign {
        ctx.desk.used(&session, &tab.id);
    }
    let args = req.get("args").cloned().unwrap_or_else(|| json!({}));
    let app = &ctx.app;
    let done = match op {
        "tab.read" => {
            let mut window = json!({});
            for k in ["offset", "limit"] {
                if let Some(n) = req.get(k).and_then(Value::as_u64) {
                    window[k] = json!(n);
                }
            }
            crate::pagectl::read(app, &tab.id, &window).await
        }
        "tab.wait" => crate::pagectl::wait(app, &tab.id, &args).await,
        "tab.shot" => crate::pagectl::shot(app, &tab.id)
            .await
            .map(|jpeg| json!({ "jpeg": jpeg })),
        _ => crate::pagectl::act(app, &tab.id, op.trim_start_matches("tab."), &args).await,
    };
    match done {
        Ok(mut v) => {
            if let Value::Object(map) = &mut v {
                map.insert("ok".into(), json!(true));
                map.insert("id".into(), json!(tab.id));
            }
            v
        }
        Err(e) => fail(e),
    }
}

async fn tab_open(ctx: &Ctx, req: &Value) -> Value {
    // Checked before anything is recorded: a refused URL leaves no tab.
    let url = match check_page_url(&str_arg(req, "url")) {
        Ok(u) => u,
        Err(e) => return fail(e),
    };
    let session = str_arg(req, "session");
    // The CLI is the person: its tab comes forward. An agent's opens behind.
    let focus = bool_arg(req, "focus", session.is_empty());
    if bool_arg(req, "raise", false) {
        raise(&ctx.app);
    }
    if !session.is_empty() {
        return open_for_agent(ctx, &session, &url, req, focus);
    }
    if let Some(id) = ctx.desk.page_with_url(&url) {
        if focus && ctx.desk.set_active(&id) {
            ctx.tell(json!({ "op": "focus", "id": id }));
        }
        return json!({ "ok": true, "id": id, "url": url, "reused": true });
    }
    let id = format!("tab-{}", uuid::Uuid::new_v4().simple());
    let rev = ctx.desk.record_open(
        TabSnap {
            id: id.clone(),
            kind: "browser".into(),
            title: short_title(&url),
            url: url.clone(),
            ..TabSnap::default()
        },
        focus,
    );
    ctx.tell(json!({ "op": "open", "id": id, "url": url, "rev": rev, "focus": focus }));
    json!({ "ok": true, "id": id, "url": url, "rev": rev })
}

fn open_for_agent(ctx: &Ctx, session: &str, url: &str, req: &Value, focus: bool) -> Value {
    let reuse = if bool_arg(req, "new", false) {
        None
    } else {
        match ctx.desk.own_tab(session, &str_arg(req, "tab")) {
            Ok(t) => t,
            Err(e) => return fail(e),
        }
    };
    if let Some(tab) = reuse {
        if let Err(e) = crate::browser::browser_navigate(ctx.app.clone(), &tab.id, url) {
            return fail(e);
        }
        ctx.desk.navigated(&tab.id, url, session);
        ctx.tell(json!({ "op": "navigate", "id": tab.id, "url": url }));
        if focus && ctx.desk.set_active(&tab.id) {
            ctx.tell(json!({ "op": "focus", "id": tab.id }));
        }
        return json!({ "ok": true, "id": tab.id, "url": url });
    }
    let id = format!("tab-{}", uuid::Uuid::new_v4().simple());
    // The page comes up (and the URL is checked again) before the tab is
    // recorded or the window hears of it.
    if let Err(e) = crate::browser::prepare_for_agent(ctx.app.clone(), id.clone(), url) {
        return fail(e);
    }
    let rev = ctx.desk.record_open(
        TabSnap {
            id: id.clone(),
            kind: "browser".into(),
            title: short_title(url),
            url: url.to_string(),
            session_id: session.to_string(),
            cwd: String::new(),
        },
        focus,
    );
    ctx.tell(json!({
        "op": "open", "id": id, "url": url, "rev": rev, "owner": session, "focus": focus,
    }));
    json!({ "ok": true, "id": id, "url": url, "rev": rev })
}

fn tab_focus(ctx: &Ctx, req: &Value) -> Value {
    let id = str_arg(req, "id");
    if !ctx.desk.set_active(&id) {
        return fail(format!("no tab `{id}`"));
    }
    if bool_arg(req, "raise", false) {
        raise(&ctx.app);
    }
    ctx.tell(json!({ "op": "focus", "id": id }));
    json!({ "ok": true, "id": id })
}

fn tab_close(ctx: &Ctx, req: &Value) -> Value {
    let id = str_arg(req, "id");
    if let Err(e) = ctx.desk.close(&id, &str_arg(req, "session")) {
        return fail(e);
    }
    ctx.tell(json!({ "op": "close", "id": id }));
    json!({ "ok": true, "id": id })
}

/// The folder and project of the tab in front.
async fn front_folder(ctx: &Ctx, tab: Option<&TabSnap>) -> (String, Value) {
    let Some(tab) = tab else {
        return (String::new(), Value::Null);
    };
    let folder = if tab.kind == "session" && !tab.session_id.is_empty() {
        ctx.orch
            .store()
            .get(&tab.session_id)
            .map(|m| m.cwd)
            .unwrap_or_default()
    } else {
        tab.cwd.clone()
    };
    if folder.trim().is_empty() || crate::files::remote_or_device(&folder) {
        return (folder, Value::Null);
    }
    let dir = folder.clone();
    let project =
        tauri::async_runtime::spawn_blocking(move || parzi_core::brain::project_for_folder(&dir))
            .await
            .ok()
            .flatten();
    (
        folder,
        project.map_or(Value::Null, |slug| json!({ "slug": slug })),
    )
}

async fn desk_state(ctx: &Ctx, req: &Value) -> Value {
    let session = str_arg(req, "session");
    let (tabs, active) = ctx.desk.snapshot();
    let front = tabs.iter().find(|t| t.id == active);
    let (folder, project) = front_folder(ctx, front).await;
    let focused = ctx
        .app
        .get_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    let running: Vec<Value> = ctx
        .orch
        .store()
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| {
            matches!(
                m.status,
                parzi_core::store::SessionStatus::Active | parzi_core::store::SessionStatus::Queued
            )
        })
        .map(|m| json!({ "id": m.id, "title": m.title, "lane": m.lane, "status": m.status }))
        .collect();
    if !session.is_empty() {
        let own: Vec<&TabSnap> = tabs
            .iter()
            .filter(|t| t.page() && t.session_id == session)
            .collect();
        let active = front.map(|t| {
            json!({
                "kind": if t.page() { "page" } else { t.kind.as_str() },
                "title": t.title,
            })
        });
        return json!({
            "ok": true,
            "focused": focused,
            "active": active,
            "tabs": own,
            "running": running,
            "folder": folder,
            "project": project,
        });
    }
    json!({
        "ok": true,
        "focused": focused,
        "active": active,
        "tabs": tabs,
        "running": running,
        "folder": folder,
        "project": project,
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
        "started": ctx.desk.started,
    })
}

/// A vault-relative note path: no climbing out, no drive, `.md` added.
fn note_path(raw: &str) -> Result<String, String> {
    let p = raw.trim().replace('\\', "/");
    if p.is_empty() {
        return Ok(String::new());
    }
    if p.starts_with('/') || p.contains(':') || p.split('/').any(|s| s == "..") {
        return Err("a brain note is a path inside the vault, like projects/parzi.md".into());
    }
    Ok(if p.to_ascii_lowercase().ends_with(".md") {
        p
    } else {
        format!("{p}.md")
    })
}

async fn desk_open(ctx: &Ctx, req: &Value) -> Value {
    let session = str_arg(req, "session");
    let focus = bool_arg(req, "focus", session.is_empty());
    let what = str_arg(req, "what");
    let (payload, done) = match what.as_str() {
        "url" | "page" => return tab_open(ctx, req).await,
        "session" => {
            let id = match ctx.orch.store().resolve_id(&str_arg(req, "id")) {
                Ok(id) => id,
                Err(e) => return fail(e.to_string()),
            };
            let title = ctx
                .orch
                .store()
                .get(&id)
                .map(|m| m.title)
                .unwrap_or_default();
            (
                json!({ "op": "show", "what": "session", "id": id, "title": title, "focus": focus }),
                format!("opened session {}", &id[..8.min(id.len())]),
            )
        }
        "settings" => {
            let section = Some(str_arg(req, "section"))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "general".into());
            if !SETTINGS.contains(&section.as_str()) {
                return fail(format!(
                    "no settings section `{section}`; one of {}",
                    SETTINGS.join(", ")
                ));
            }
            (
                json!({ "op": "show", "what": "settings", "section": section, "focus": true }),
                format!("opened settings, {section}"),
            )
        }
        "brain" => {
            let path = match note_path(&str_arg(req, "path")) {
                Ok(p) => p,
                Err(e) => return fail(e),
            };
            (
                json!({ "op": "show", "what": "brain", "path": path, "focus": true }),
                "opened the brain".to_string(),
            )
        }
        "history" => {
            let view = Some(str_arg(req, "view"))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "sessions".into());
            if !HISTORY.contains(&view.as_str()) {
                return fail(format!("history shows {}", HISTORY.join(", ")));
            }
            (
                json!({ "op": "show", "what": "history", "view": view, "focus": true }),
                "opened history".to_string(),
            )
        }
        // The plan panel beside the session; "work" and "tasks" are what
        // it was called before.
        "plan" | "work" | "tasks" => (
            json!({ "op": "show", "what": "plan", "focus": true }),
            "opened the plan".to_string(),
        ),
        "home" => (
            json!({ "op": "show", "what": "home", "focus": true }),
            "opened home".to_string(),
        ),
        _ => {
            return fail("what must be session, settings, brain, history, plan, home or url");
        }
    };
    if bool_arg(req, "raise", false) {
        raise(&ctx.app);
    }
    ctx.tell(payload);
    json!({ "ok": true, "done": done })
}

fn desk_notify(ctx: &Ctx, req: &Value) -> Value {
    let text: String = str_arg(req, "text").chars().take(300).collect();
    if text.is_empty() {
        return fail("a notification needs text");
    }
    let session = str_arg(req, "session");
    ctx.tell(json!({ "op": "notify", "text": text, "session": session }));
    let _ = ctx
        .desk
        .events
        .send(json!({ "kind": "notify", "text": text, "session": session }));
    // In the background the taskbar button flashes until the window is used.
    if let Some(w) = ctx.app.get_window("main") {
        if !w.is_focused().unwrap_or(false) {
            let _ = w.request_user_attention(Some(tauri::UserAttentionType::Informational));
        }
    }
    json!({ "ok": true })
}

async fn desk_screenshot(ctx: &Ctx) -> Value {
    let Some(wv) = ctx.app.get_webview("main") else {
        return fail("the window is not up");
    };
    #[cfg(windows)]
    {
        use base64::Engine as _;
        match tauri::async_runtime::spawn_blocking(move || crate::dwm::capture_page(&wv)).await {
            Ok(Ok(jpeg)) => json!({
                "ok": true,
                "jpeg": base64::engine::general_purpose::STANDARD.encode(jpeg),
            }),
            Ok(Err(e)) => fail(e),
            Err(e) => fail(e.to_string()),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = wv;
        fail("window screenshots need Windows")
    }
}

async fn approval_answer(ctx: &Ctx, req: &Value) -> Value {
    let Some(allow) = req.get("allow").and_then(Value::as_bool) else {
        return fail("allow true or false is required");
    };
    let (key, row) = match ctx.desk.approval_key(&str_arg(req, "key")) {
        Ok(found) => found,
        Err(e) => return fail(e),
    };
    let entry = ctx.pending.lock().await.remove(&key);
    let Some((session, tx)) = entry else {
        ctx.desk.approval_closed(&key);
        return fail("approval expired or unknown");
    };
    let decision = if allow {
        Approval::Allow
    } else {
        Approval::Deny
    };
    if tx.send(decision).is_err() {
        return fail("the run is no longer waiting");
    }
    ctx.desk.approval_closed(&key);
    ctx.tell(json!({
        "op": "approval",
        "key": key,
        "allow": allow,
        "label": row.get("label").cloned().unwrap_or_default(),
    }));
    json!({ "ok": true, "key": key, "session": session })
}

async fn providers(ctx: &Ctx, req: &Value) -> Value {
    let all = if bool_arg(req, "refresh", false) {
        let ids: Vec<String> = req
            .get("ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        ctx.orch.refresh_providers(&ids).await
    } else {
        ctx.orch.provider_statuses()
    };
    json!({ "ok": true, "providers": crate::settings::in_roster_order(all) })
}

fn session_list(orch: &Orchestrator) -> Value {
    match orch.store().list() {
        Ok(rows) => json!({ "ok": true, "sessions": rows }),
        Err(e) => fail(e.to_string()),
    }
}

fn resolve_id(orch: &Orchestrator, id: &str) -> Result<String, String> {
    orch.store().resolve_id(id).map_err(|e| e.to_string())
}

fn session_show(orch: &Orchestrator, req: &Value, export: bool) -> Value {
    let id = match resolve_id(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(e) => return fail(e),
    };
    let meta = match orch.store().get(&id) {
        Ok(m) => m,
        Err(e) => return fail(e.to_string()),
    };
    let md = parzi_core::paths::sessions_dir()
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join(&id).join("session.md")).ok())
        .unwrap_or_default();
    let text: String = md.chars().take(20_000).collect();
    if export {
        json!({ "ok": true, "id": id, "text": text })
    } else {
        json!({ "ok": true, "session": meta, "text": text })
    }
}

async fn session_kill(orch: &Orchestrator, req: &Value) -> Value {
    let id = match resolve_id(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(e) => return fail(e),
    };
    match orch.kill(&id).await {
        Ok(()) => json!({ "ok": true, "id": id }),
        Err(e) => fail(e.to_string()),
    }
}

fn session_fork(orch: &Orchestrator, req: &Value) -> Value {
    let id = match resolve_id(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(e) => return fail(e),
    };
    let at = req
        .get("at")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok());
    match orch.store().fork(&id, at) {
        Ok(meta) => json!({ "ok": true, "id": meta.id }),
        Err(e) => fail(e.to_string()),
    }
}

fn session_rename(orch: &Orchestrator, req: &Value) -> Value {
    let id = match resolve_id(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(e) => return fail(e),
    };
    let title = str_arg(req, "title");
    if title.is_empty() {
        return fail("title is required");
    }
    match orch.store().set_title(&id, &title) {
        Ok(()) => json!({ "ok": true, "id": id }),
        Err(e) => fail(e.to_string()),
    }
}

async fn session_delete(ctx: &Ctx, out: &Out, req: &Value) -> Value {
    let id = match resolve_id(&ctx.orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(e) => return fail(e),
    };
    let title = ctx
        .orch
        .store()
        .get(&id)
        .map(|m| m.title)
        .unwrap_or_default();
    let text = format!(
        "A command sent to Parzi asks to delete the session \"{title}\" and its transcript, with the subsessions it started.\n\nAllow it only if you just ran this yourself."
    );
    if !ctx.confirm(out, "Delete a session?", text, "Delete").await {
        return fail("not deleted: the person declined in the Parzi window");
    }
    match crate::sessions::delete_with_runs(&ctx.orch, &id).await {
        Ok(n) => {
            ctx.tell(json!({ "op": "deleted", "id": id }));
            json!({ "ok": true, "id": id, "removed": n })
        }
        Err(e) => fail(e),
    }
}

/// `yes` makes a run approve its own shell commands and file writes: the
/// person has to say so in the window, whoever holds the token.
async fn session_send(ctx: &Ctx, out: &Out, req: &Value) -> Value {
    let message = str_arg(req, "message");
    if message.is_empty() {
        return fail("empty message");
    }
    let target = str_arg(req, "target");
    let fresh = target.is_empty() || target == "new";
    // No automatic models: a new thread needs one, a continued thread keeps its own.
    let model = str_arg(req, "model");
    if fresh && model.is_empty() {
        return fail(
            "no model: pass one with --model (claude, claude/opus, codex, ...); `parzi providers` lists them",
        );
    }
    let cwd = str_arg(req, "cwd");
    // A relative folder would resolve against the window's own directory,
    // not the caller's: the agent would work somewhere nobody chose.
    if !cwd.is_empty() && !std::path::Path::new(&cwd).is_absolute() {
        return fail(format!(
            "`{cwd}` is not a full path: pass the folder as an absolute path"
        ));
    }
    let effort = {
        let e = str_arg(req, "effort");
        parzi_runtime::orchestrator::normalize_effort(if e.is_empty() { "medium" } else { &e })
    };
    let continued = if fresh {
        None
    } else {
        match resolve_id(&ctx.orch, &target) {
            Ok(id) => Some(id),
            Err(e) => return fail(e),
        }
    };
    let yes = bool_arg(req, "yes", false);
    if yes {
        let thread = match &continued {
            Some(id) => format!(
                "the session \"{}\"",
                ctx.orch
                    .store()
                    .get(id)
                    .map(|m| m.title)
                    .unwrap_or_default()
            ),
            None => "a new session".into(),
        };
        let excerpt: String = message.chars().take(300).collect();
        let text = format!(
            "A command sent to Parzi asks to run {thread} with every tool call approved: it may run shell commands and change files without asking you.\n\nMessage: {excerpt}\nFolder: {}\nModel: {}\n\nAllow it only if you just ran this yourself.",
            if cwd.is_empty() { "a scratch folder" } else { &cwd },
            if model.is_empty() { "the session's own" } else { &model },
        );
        if !ctx
            .confirm(
                out,
                "Run an agent without asking?",
                text,
                "Run without asking",
            )
            .await
        {
            return fail("not run: the person declined in the Parzi window");
        }
    }
    let approver: Arc<dyn Approver> = if yes {
        Arc::new(AutoApprover)
    } else {
        Arc::new(GuiApprover {
            app: ctx.app.clone(),
            pending: ctx.pending.clone(),
        })
    };
    // Subscribed before the run starts, so nothing it says is missed. The
    // bus, not the run's own channel: a queued run reports there once it
    // gets its turn.
    let mut runs = ctx.orch.subscribe();
    let mut side = ctx.desk.events.subscribe();
    let sent = match continued {
        None => {
            // Same filing as the desk: a thread belongs to its folder's project.
            let project = crate::sessions::project_for(&cwd).await;
            ctx.orch
                .spawn_as(
                    RunCause::Cli,
                    &project,
                    "",
                    &model,
                    &message,
                    Some(approver),
                    &cwd,
                    &effort,
                    vec![],
                    None,
                )
                .await
                .map(|(meta, rx)| (meta.id, rx))
        }
        Some(id) => ctx
            .orch
            .send_as(
                RunCause::Cli,
                &id,
                &message,
                Some(approver),
                &cwd,
                &effort,
                vec![],
                Some(model).filter(|m| !m.is_empty()),
                None,
            )
            .await
            .map(|rx| (id, rx)),
    };
    let id = match sent {
        // Everything the channel carries also goes over the bus.
        Ok((id, rx)) => {
            drop(rx);
            id
        }
        Err(e) => return fail(e.to_string()),
    };
    let stream = out.progress && bool_arg(req, "stream", false);
    if stream
        && out
            .line(&json!({ "kind": "started", "session": id }))
            .await
            .is_err()
    {
        return json!({ "ok": true, "id": id, "detached": true });
    }
    let detached = || json!({ "ok": true, "id": id, "detached": true });
    let mut text = String::new();
    let mut failed = None;
    let mut side_open = stream;
    // A backstop: an event lost to a lagging bus must not leave this
    // waiting on a run that already ended.
    let mut check = tokio::time::interval(Duration::from_secs(15));
    check.tick().await;
    loop {
        tokio::select! {
            run = runs.recv() => match run {
                Ok((sid, ev)) if sid == id => {
                    match &ev {
                        RunEvent::Text(t) => text.push_str(t),
                        RunEvent::Error(e) => failed = Some(e.clone()),
                        _ => {}
                    }
                    let end = matches!(
                        ev,
                        RunEvent::Done { .. } | RunEvent::Paused { .. } | RunEvent::Error(_)
                    );
                    if stream {
                        if let Some(v) = ui_event(&id, &ev) {
                            // The client left: the run goes on in the window.
                            if out.line(&v).await.is_err() {
                                return detached();
                            }
                        }
                    }
                    if end {
                        break;
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    if run_over(&ctx.orch, &id) {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            side_ev = side.recv(), if side_open => match side_ev {
                Ok(v) if v.get("session").and_then(Value::as_str) == Some(id.as_str())
                    && v.get("kind").and_then(Value::as_str).is_some_and(|k| k.starts_with("approval")) =>
                {
                    if out.line(&v).await.is_err() {
                        return detached();
                    }
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => side_open = false,
            },
            _ = check.tick() => {
                if run_over(&ctx.orch, &id) {
                    break;
                }
            }
        }
    }
    let status = ctx
        .orch
        .store()
        .get(&id)
        .ok()
        .and_then(|m| serde_json::to_value(m.status).ok())
        .unwrap_or(Value::Null);
    if let Some(e) = failed {
        return json!({ "ok": false, "id": id, "error": e, "text": text, "status": status });
    }
    json!({ "ok": true, "id": id, "text": text, "status": status })
}

/// The session is neither running nor waiting for its turn.
fn run_over(orch: &Orchestrator, id: &str) -> bool {
    use parzi_core::store::SessionStatus;
    orch.store()
        .get(id)
        .is_ok_and(|m| !matches!(m.status, SessionStatus::Active | SessionStatus::Queued))
}

/// Every run event, approval, tab change and notice as a line, until the
/// client leaves. A keepalive finds clients that left quietly.
async fn subscribe(ctx: &Ctx, out: &Out, req: &Value) -> Result<(), String> {
    let session = str_arg(req, "session");
    let mut runs = ctx.orch.subscribe();
    let mut side = ctx.desk.events.subscribe();
    let mut tick = tokio::time::interval(KEEPALIVE);
    tick.tick().await;
    out.line(&json!({ "kind": "subscribed", "instance": ctx.desk.instance }))
        .await?;
    let ours = |v: &Value| {
        session.is_empty()
            || v.get("session")
                .and_then(Value::as_str)
                .is_none_or(|s| s == session)
    };
    loop {
        let next = tokio::select! {
            run = runs.recv() => match run {
                Ok((sid, ev)) if session.is_empty() || sid == session => ui_event(&sid, &ev),
                Ok(_) => None,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    Some(json!({ "kind": "lagged", "missed": n }))
                }
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            other = side.recv() => match other {
                Ok(v) if ours(&v) => Some(v),
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => None,
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            _ = tick.tick() => Some(json!({ "kind": "ping" })),
        };
        if let Some(v) = next {
            out.line(&v).await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(id: &str, url: &str, owner: &str) -> TabSnap {
        TabSnap {
            id: id.into(),
            kind: "browser".into(),
            title: format!("title {id}"),
            url: url.into(),
            session_id: owner.into(),
            cwd: String::new(),
        }
    }

    /// Rows in the shape `deskRows` in ui/src/lib/desk.ts sends, through
    /// the same serde path `desk_sync` takes: a page carries its owner.
    fn ui_rows(tabs: &[TabSnap]) -> Vec<TabSnap> {
        let rows: Vec<Value> = tabs
            .iter()
            .map(|t| {
                json!({
                    "id": t.id, "kind": t.kind, "title": t.title, "url": t.url,
                    "session_id": t.session_id, "cwd": t.cwd,
                })
            })
            .collect();
        serde_json::from_value(Value::Array(rows)).unwrap()
    }

    #[test]
    fn the_token_gate_lets_only_hello_through() {
        let desk = Desk::new();
        let Gate::Reply(v) = gate_line(&desk, "{\"op\":\"hello\"}") else {
            panic!("hello needs no token");
        };
        assert_eq!(v["instance"], json!(desk.instance));
        assert!(v.get("token").is_none(), "hello never says the token");
        for bad in [
            "{\"op\":\"tab.list\"}".to_string(),
            "{\"op\":\"tab.list\",\"token\":\"nope\"}".to_string(),
            "not json".to_string(),
            format!("{{\"op\":\"tab.list\",\"token\":\"{}x\"}}", desk.token),
        ] {
            match gate_line(&desk, &bad) {
                Gate::Reply(v) => assert_eq!(v["error"], "bad token", "{bad}"),
                Gate::Run(_) => panic!("{bad} got through"),
            }
        }
        let good = format!("{{\"op\":\"tab.list\",\"token\":\"{}\"}}", desk.token);
        assert!(matches!(gate_line(&desk, &good), Gate::Run(_)));
        let huge = "x".repeat(MAX_REQUEST as usize);
        assert!(
            matches!(gate_line(&desk, &huge), Gate::Reply(v) if v["error"] == "request too large")
        );
    }

    #[test]
    fn a_synced_page_keeps_its_owner() {
        // The bug this guards: the window synced session_id from
        // t.sessionId only, so after the first sync an agent lost its tab.
        let desk = Desk::new();
        desk.record_open(page("t1", "https://a.example/", "s1"), false);
        desk.sync(
            ui_rows(&[
                page("t1", "https://a.example/", "s1"),
                page("t2", "https://bank.example/", ""),
            ]),
            1,
            "t2".into(),
        );
        let (tab, foreign) = desk.find_page("s1", "").unwrap();
        assert_eq!(tab.id, "t1");
        assert!(!foreign, "an agent's own tab needs no consent");
        let (tab, foreign) = desk.find_page("s1", "active").unwrap();
        assert_eq!(tab.id, "t2");
        assert!(foreign, "the person's own tab needs consent");
        assert!(
            desk.find_page("s2", "").is_err(),
            "another session has none"
        );
        let (_, foreign) = desk.find_page("s2", "t1").unwrap();
        assert!(foreign, "another agent's tab needs consent");
        let (tab, foreign) = desk.find_page("", "").unwrap();
        assert_eq!(tab.id, "t2", "the CLI gets the tab in front");
        assert!(foreign);
        let (_, foreign) = desk.find_page("", "t1").unwrap();
        assert!(!foreign, "the CLI on an agent's tab");
    }

    #[test]
    fn an_open_survives_a_racing_sync_and_a_reload_is_heard() {
        let desk = Desk::new();
        desk.sync(vec![page("u1", "https://x.example/", "")], 0, "u1".into());
        let rev = desk.record_open(page("t1", "https://a.example/", "s1"), false);
        // A sync sent before the window saw the open lacks the tab.
        desk.sync(
            vec![page("u1", "https://x.example/", "")],
            rev - 1,
            "u1".into(),
        );
        assert!(desk.find_page("s1", "").is_ok(), "kept while pending");
        // The window has seen it (rev) and no longer lists it: closed.
        desk.sync(vec![page("u1", "https://x.example/", "")], rev, "u1".into());
        assert!(desk.find_page("s1", "").is_err(), "closed by the person");
        // A reloaded window starts from rev 0 and is still heard.
        desk.sync(vec![page("u2", "https://y.example/", "")], 0, "u2".into());
        let (tab, _) = desk.find_page("", "").unwrap();
        assert_eq!(tab.id, "u2");
    }

    #[test]
    fn agents_see_their_own_tabs_and_only_a_title_of_the_rest() {
        let desk = Desk::new();
        desk.sync(
            vec![
                page("t1", "https://mine.example/", "s1"),
                page("t2", "https://bank.example/account", ""),
                page("t3", "https://other.example/", "s2"),
            ],
            0,
            "t2".into(),
        );
        let v = desk.tab_list("s1", false);
        let text = v.to_string();
        assert!(text.contains("mine.example"));
        assert!(!text.contains("bank.example"), "{text}");
        assert!(!text.contains("other.example"), "{text}");
        assert_eq!(v["active_title"], "title t2");
        let all = desk.tab_list("s1", true).to_string();
        assert!(all.contains("bank.example") && all.contains("other.example"));
        let cli = desk.tab_list("", false).to_string();
        assert!(cli.contains("bank.example"));
    }

    #[test]
    fn opens_focus_only_when_asked_and_tabs_close_only_for_their_owner() {
        let desk = Desk::new();
        desk.sync(vec![page("u1", "https://x.example/", "")], 0, "u1".into());
        desk.record_open(page("t1", "https://a.example/", "s1"), false);
        assert_eq!(
            desk.snapshot().1,
            "u1",
            "a background open keeps the front tab"
        );
        desk.record_open(page("t2", "https://b.example/", ""), true);
        assert_eq!(desk.snapshot().1, "t2");
        assert_eq!(
            desk.page_with_url("https://a.example/").as_deref(),
            Some("t1")
        );
        assert!(desk.close("t1", "s2").is_err(), "not s2's tab");
        assert!(desk.close("u1", "s1").is_err(), "not an agent's tab");
        assert!(desk.close("t1", "s1").is_ok());
        assert!(desk.close("t1", "s1").is_err(), "already gone");
        assert!(desk.find_page("s1", "").is_err());
        assert!(
            desk.own_tab("s1", "t2").is_err(),
            "the person's tab is not the agent's to navigate"
        );
    }

    #[test]
    fn refused_urls_never_become_tabs() {
        // tab_open checks with check_page_url before record_open; the same
        // check, here, keeps the list empty.
        let desk = Desk::new();
        for bad in [
            "file:///C:/x",
            "http://tauri.localhost/",
            "ftp://a.example/",
        ] {
            assert!(check_page_url(bad).is_err(), "{bad}");
        }
        assert!(desk.snapshot().0.is_empty());
    }

    #[test]
    fn approvals_are_listed_until_answered_and_found_by_prefix() {
        let desk = Desk::new();
        let mut events = desk.events.subscribe();
        let call = ToolCallInfo {
            id: "c1".into(),
            name: "shell.exec".into(),
            args: json!({ "cmd": "cargo test" }),
            lane: "build".into(),
            session: "s1".into(),
        };
        desk.approval_opened("abcd-1234", &call);
        desk.approval_opened("abcd-5678", &call);
        assert_eq!(desk.approvals().len(), 2);
        assert_eq!(desk.approvals()[0]["label"], "Running `cargo test`");
        assert!(desk.approval_key("abcd").is_err(), "ambiguous");
        assert!(desk.approval_key("ab").is_err(), "too short");
        assert_eq!(desk.approval_key("abcd-1").unwrap().0, "abcd-1234");
        desk.approval_closed("abcd-1234");
        assert_eq!(desk.approvals().len(), 1);
        assert_eq!(events.try_recv().unwrap()["kind"], "approval");
        assert_eq!(events.try_recv().unwrap()["kind"], "approval");
        assert_eq!(events.try_recv().unwrap()["kind"], "approval_done");
    }

    #[test]
    fn consent_lasts_a_while_per_tab() {
        let desk = Desk::new();
        assert!(!desk.consented("t1"));
        desk.allow_for_a_while("t1");
        assert!(desk.consented("t1"));
        assert!(!desk.consented("t2"));
    }

    #[test]
    fn note_paths_stay_in_the_vault() {
        assert_eq!(note_path("projects/parzi").unwrap(), "projects/parzi.md");
        assert_eq!(note_path("a\\b.MD").unwrap(), "a/b.MD");
        assert_eq!(note_path("").unwrap(), "");
        for bad in ["../x", "/etc/x", "C:/x", "a/../../b"] {
            assert!(note_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_gui_file_names_this_window() {
        let desk = Desk::new();
        let f = desk.file(4242);
        assert_eq!(f.port, 4242);
        assert_eq!(f.pid, std::process::id());
        assert_eq!(f.instance, desk.instance);
        let raw = serde_json::to_string(&f).unwrap();
        assert_eq!(GuiFile::parse(&raw), Some(f));
    }
}
