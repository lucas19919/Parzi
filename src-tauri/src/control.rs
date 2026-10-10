use std::sync::Arc;
use std::time::Duration;

use parzi_runtime::desk::normalize_url;
use parzi_runtime::handler::RunEvent;
use parzi_runtime::tools::{Approver, AutoApprover};
use parzi_runtime::Orchestrator;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, Semaphore};

use crate::{AppState, GuiApprover, Pending};

const MAX_REQUEST: u64 = 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CLIENTS: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabSnap {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub session_id: String,
}

#[derive(Default)]
struct DeskState {
    rev: u64,
    tabs: Vec<TabSnap>,
    active: String,
}

pub struct Desk {
    state: Mutex<DeskState>,
    token: String,
}

impl Desk {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(DeskState::default()),
            token: uuid::Uuid::new_v4().simple().to_string(),
        }
    }

    pub async fn sync(&self, tabs: Vec<TabSnap>, rev: u64, active: String) {
        let mut st = self.state.lock().await;
        if rev < st.rev {
            return;
        }
        st.rev = rev;
        st.tabs = tabs;
        st.active = active;
    }
}

#[tauri::command]
pub async fn desk_sync(
    state: State<'_, AppState>,
    tabs: Vec<TabSnap>,
    rev: u64,
    active: String,
) -> Result<(), String> {
    state.desk.sync(tabs, rev, active).await;
    Ok(())
}

pub fn clear_gui_file() {
    if let Some(path) = parzi_runtime::desk::gui_path() {
        let _ = std::fs::remove_file(path);
    }
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
        let body = json!({ "port": port, "token": desk.token });
        if let Err(e) = write_private(&path, &body.to_string()) {
            tracing::warn!("desk control file not written: {e}");
        }
    }
    let slots = Arc::new(Semaphore::new(MAX_CLIENTS));
    loop {
        let (sock, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                // Persistent errors (fd exhaustion) would otherwise spin.
                tracing::debug!("desk control accept: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let Ok(slot) = slots.clone().acquire_owned().await else {
            return;
        };
        let app = app.clone();
        let orch = orch.clone();
        let pending = pending.clone();
        let desk = desk.clone();
        tokio::spawn(async move {
            let _ = answer(sock, &app, &orch, &pending, &desk).await;
            drop(slot);
        });
    }
}

/// gui.json carries the bridge token: born owner-only (0600 on unix, the
/// private profile folder on Windows) and swapped in whole, never half-written.
fn write_private(path: &std::path::Path, body: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let tmp = path.with_file_name(format!(".gui.{}.tmp", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        file.write_all(body.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        parzi_core::atomic_write(path, body.as_bytes()).map_err(|e| e.to_string())
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

async fn answer(
    sock: TcpStream,
    app: &AppHandle,
    orch: &Orchestrator,
    pending: &Pending,
    desk: &Desk,
) -> Result<(), String> {
    let (read, mut write) = sock.into_split();
    let mut line = String::new();
    // A client that connects and never finishes its line must not hold a slot.
    tokio::time::timeout(
        IO_TIMEOUT,
        BufReader::new(read.take(MAX_REQUEST)).read_line(&mut line),
    )
    .await
    .map_err(|_| "request timed out".to_string())?
    .map_err(|e| e.to_string())?;
    let reply = if line.len() as u64 >= MAX_REQUEST {
        json!({ "ok": false, "error": "request too large" })
    } else {
        let req: Value = serde_json::from_str(&line).unwrap_or(json!({}));
        let token = req.get("token").and_then(Value::as_str).unwrap_or("");
        if tokens_equal(token, &desk.token) {
            dispatch(app, orch, pending, desk, &req).await
        } else {
            json!({ "ok": false, "error": "bad token" })
        }
    };
    let mut out = serde_json::to_string(&reply)
        .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"encode\"}".into());
    out.push('\n');
    tokio::time::timeout(IO_TIMEOUT, write.write_all(out.as_bytes()))
        .await
        .map_err(|_| "reply timed out".to_string())?
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn dispatch(
    app: &AppHandle,
    orch: &Orchestrator,
    pending: &Pending,
    desk: &Desk,
    req: &Value,
) -> Value {
    let op = req.get("op").and_then(Value::as_str).unwrap_or("");
    match op {
        "tab.list" => tab_list(desk).await,
        "tab.open" => tab_open(app, desk, req).await,
        "tab.focus" => tab_focus(app, desk, req).await,
        "tab.close" => tab_close(app, desk, req).await,
        "tab.read" => tab_read(app, desk, req).await,
        "tab.click" => tab_act(app, desk, req, "click").await,
        "tab.type" => tab_act(app, desk, req, "type").await,
        "tab.shot" => tab_shot(app, desk, req).await,
        "session.list" => session_list(orch),
        "session.show" | "session.export" => session_show(orch, req, op == "session.export"),
        "session.kill" => session_kill(orch, req).await,
        "session.fork" => session_fork(orch, req),
        "session.rename" => session_rename(orch, req),
        "session.delete" => session_delete(orch, req).await,
        "session.send" => session_send(app, orch, pending, req).await,
        _ => json!({ "ok": false, "error": format!("unknown op `{op}`") }),
    }
}

fn str_arg(req: &Value, key: &str) -> String {
    req.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

async fn tab_list(desk: &Desk) -> Value {
    let st = desk.state.lock().await;
    json!({ "ok": true, "tabs": st.tabs, "active": st.active })
}

async fn connected(desk: &Desk, session: &str) -> Option<TabSnap> {
    let st = desk.state.lock().await;
    st.tabs
        .iter()
        .find(|t| t.kind == "browser" && !session.is_empty() && t.session_id == session)
        .cloned()
}

async fn page_for(desk: &Desk, req: &Value) -> Option<TabSnap> {
    let session = str_arg(req, "session");
    if !session.is_empty() {
        return connected(desk, &session).await;
    }
    let st = desk.state.lock().await;
    st.tabs
        .iter()
        .find(|t| t.id == st.active && t.kind == "browser")
        .or_else(|| st.tabs.iter().rev().find(|t| t.kind == "browser"))
        .cloned()
}

const NO_TAB: &str = "this session has no tab yet: open one with browser_open";

async fn tab_read(app: &AppHandle, desk: &Desk, req: &Value) -> Value {
    let Some(tab) = page_for(desk, req).await else {
        return json!({ "ok": true, "id": "", "title": "", "url": "" });
    };
    if str_arg(req, "session").is_empty() {
        return json!({ "ok": true, "id": tab.id, "title": tab.title, "url": tab.url });
    }
    match crate::pagectl::read(app, &tab.id).await {
        Ok(page) => json!({
            "ok": true,
            "id": tab.id,
            "title": page.get("title").cloned().unwrap_or_default(),
            "url": page.get("url").cloned().unwrap_or_default(),
            "text": page.get("text").cloned().unwrap_or_default(),
            "controls": page.get("controls").cloned().unwrap_or_default(),
        }),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

async fn tab_act(app: &AppHandle, desk: &Desk, req: &Value, kind: &str) -> Value {
    let Some(tab) = connected(desk, &str_arg(req, "session")).await else {
        return json!({ "ok": false, "error": NO_TAB });
    };
    let args = req.get("args").cloned().unwrap_or_else(|| json!({}));
    match crate::pagectl::act(app, &tab.id, kind, &args).await {
        Ok(done) => json!({ "ok": true, "done": done }),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

async fn tab_shot(app: &AppHandle, desk: &Desk, req: &Value) -> Value {
    let Some(tab) = connected(desk, &str_arg(req, "session")).await else {
        return json!({ "ok": false, "error": NO_TAB });
    };
    match crate::pagectl::shot(app, &tab.id).await {
        Ok(jpeg) => json!({ "ok": true, "jpeg": jpeg }),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

fn host_title(url: &str) -> String {
    let bare = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    bare.chars().take(24).collect()
}

async fn tab_open(app: &AppHandle, desk: &Desk, req: &Value) -> Value {
    let url = normalize_url(&str_arg(req, "url"));
    if url.is_empty() {
        return json!({ "ok": false, "error": "url is required" });
    }
    let session = str_arg(req, "session");
    if !session.is_empty() {
        return open_connected(app, desk, &session, &url).await;
    }
    let mut st = desk.state.lock().await;
    if let Some(id) = st
        .tabs
        .iter()
        .find(|t| t.kind == "browser" && t.url == url)
        .map(|t| t.id.clone())
    {
        st.active.clone_from(&id);
        drop(st);
        let _ = app.emit(
            "parzi://desk",
            json!({ "op": "focus", "id": id, "url": url }),
        );
        return json!({ "ok": true, "id": id, "url": url });
    }
    let id = format!("tab-{}", uuid::Uuid::new_v4().simple());
    st.rev += 1;
    let rev = st.rev;
    st.tabs.push(TabSnap {
        id: id.clone(),
        kind: "browser".into(),
        title: host_title(&url),
        url: url.clone(),
        session_id: String::new(),
    });
    st.active.clone_from(&id);
    drop(st);
    let _ = app.emit(
        "parzi://desk",
        json!({ "op": "open", "id": id, "url": url, "rev": rev }),
    );
    json!({ "ok": true, "id": id, "url": url, "rev": rev })
}

async fn open_connected(app: &AppHandle, desk: &Desk, session: &str, url: &str) -> Value {
    if let Some(tab) = connected(desk, session).await {
        if let Err(e) = crate::browser::browser_navigate(app.clone(), &tab.id, url) {
            return json!({ "ok": false, "error": e });
        }
        let _ = app.emit(
            "parzi://desk",
            json!({ "op": "navigate", "id": tab.id, "url": url }),
        );
        return json!({ "ok": true, "id": tab.id, "url": url });
    }
    let id = format!("tab-{}", uuid::Uuid::new_v4().simple());
    let rev = {
        let mut st = desk.state.lock().await;
        st.rev += 1;
        st.tabs.push(TabSnap {
            id: id.clone(),
            kind: "browser".into(),
            title: host_title(url),
            url: url.to_string(),
            session_id: session.to_string(),
        });
        st.rev
    };
    let _ = app.emit(
        "parzi://desk",
        json!({ "op": "open", "id": id, "url": url, "rev": rev, "owner": session }),
    );
    if let Err(e) = crate::browser::prepare_for_agent(app.clone(), id.clone(), url) {
        return json!({ "ok": false, "error": e });
    }
    json!({ "ok": true, "id": id, "url": url, "rev": rev })
}

async fn tab_focus(app: &AppHandle, desk: &Desk, req: &Value) -> Value {
    let id = str_arg(req, "id");
    {
        let mut st = desk.state.lock().await;
        if !st.tabs.iter().any(|t| t.id == id) {
            return json!({ "ok": false, "error": format!("no tab `{id}`") });
        }
        st.active.clone_from(&id);
    }
    let _ = app.emit("parzi://desk", json!({ "op": "focus", "id": id }));
    json!({ "ok": true, "id": id })
}

async fn tab_close(app: &AppHandle, desk: &Desk, req: &Value) -> Value {
    let id = str_arg(req, "id");
    {
        let mut st = desk.state.lock().await;
        let before = st.tabs.len();
        st.tabs.retain(|t| t.id != id);
        if st.tabs.len() == before {
            return json!({ "ok": false, "error": format!("no tab `{id}`") });
        }
        if st.active == id {
            st.active.clear();
        }
    }
    let _ = app.emit("parzi://desk", json!({ "op": "close", "id": id }));
    json!({ "ok": true })
}

fn session_list(orch: &Orchestrator) -> Value {
    match orch.store().list() {
        Ok(rows) => json!({ "ok": true, "sessions": rows }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

fn resolve_id(orch: &Orchestrator, id: &str) -> Result<String, String> {
    orch.store().resolve_id(id).map_err(|e| e.to_string())
}

fn session_show(orch: &Orchestrator, req: &Value, export: bool) -> Value {
    let id = match resolve_id(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(e) => return json!({ "ok": false, "error": e }),
    };
    let meta = match orch.store().get(&id) {
        Ok(m) => m,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }),
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
        Err(e) => return json!({ "ok": false, "error": e }),
    };
    match orch.kill(&id).await {
        Ok(()) => json!({ "ok": true, "id": id }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

fn session_fork(orch: &Orchestrator, req: &Value) -> Value {
    let id = match resolve_id(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(e) => return json!({ "ok": false, "error": e }),
    };
    let at = req
        .get("at")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok());
    match orch.store().fork(&id, at) {
        Ok(meta) => json!({ "ok": true, "id": meta.id }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

fn session_rename(orch: &Orchestrator, req: &Value) -> Value {
    let id = match resolve_id(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(e) => return json!({ "ok": false, "error": e }),
    };
    let title = str_arg(req, "title");
    if title.is_empty() {
        return json!({ "ok": false, "error": "title is required" });
    }
    match orch.store().set_title(&id, &title) {
        Ok(()) => json!({ "ok": true, "id": id }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn session_delete(orch: &Orchestrator, req: &Value) -> Value {
    let id = match resolve_id(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(e) => return json!({ "ok": false, "error": e }),
    };
    match crate::sessions::delete_with_runs(orch, &id).await {
        Ok(n) => json!({ "ok": true, "id": id, "removed": n }),
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

async fn session_send(
    app: &AppHandle,
    orch: &Orchestrator,
    pending: &Pending,
    req: &Value,
) -> Value {
    let message = str_arg(req, "message");
    if message.is_empty() {
        return json!({ "ok": false, "error": "empty message" });
    }
    let target = str_arg(req, "target");
    let model = {
        let m = str_arg(req, "model");
        if m.is_empty() {
            "auto".into()
        } else {
            m
        }
    };
    let cwd = str_arg(req, "cwd");
    let effort = {
        let e = str_arg(req, "effort");
        parzi_runtime::orchestrator::normalize_effort(if e.is_empty() { "medium" } else { &e })
    };
    let yes = req.get("yes").and_then(Value::as_bool).unwrap_or(false);
    let approver: Arc<dyn Approver> = if yes {
        Arc::new(AutoApprover)
    } else {
        Arc::new(GuiApprover {
            app: app.clone(),
            pending: pending.clone(),
        })
    };
    let sent = if target.is_empty() || target == "new" {
        // Same filing as the desk: a thread belongs to its folder's project.
        let project = crate::sessions::project_for(&cwd).await;
        orch.spawn(
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
    } else {
        match resolve_id(orch, &target) {
            Ok(id) => orch
                .send_to(
                    &id,
                    &message,
                    Some(approver),
                    &cwd,
                    &effort,
                    vec![],
                    Some(model),
                    None,
                )
                .await
                .map(|rx| (id, rx)),
            Err(e) => return json!({ "ok": false, "error": e }),
        }
    };
    let (id, mut rx) = match sent {
        Ok(pair) => pair,
        Err(e) => return json!({ "ok": false, "error": e.to_string() }),
    };
    let mut text = String::new();
    let mut failed = None;
    while let Some(ev) = rx.recv().await {
        match ev {
            RunEvent::Text(t) => text.push_str(&t),
            RunEvent::Error(e) => {
                failed = Some(e);
                break;
            }
            RunEvent::Done { .. } => break,
            _ => {}
        }
    }
    if let Some(e) = failed {
        return json!({ "ok": false, "id": id, "error": e, "text": text });
    }
    json!({ "ok": true, "id": id, "text": text })
}
