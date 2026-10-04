use std::sync::Arc;

use parzi_runtime::desk::normalize_url;
use parzi_runtime::handler::RunEvent;
use parzi_runtime::tools::{Approver, AutoApprover};
use parzi_runtime::Orchestrator;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::{AppState, GuiApprover, Pending};

const MAX_REQUEST: u64 = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabSnap {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, alias = "sessionId")]
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
    if let Ok(dir) = parzi_core::paths::parzi_dir() {
        let body = json!({ "port": port, "token": desk.token });
        let _ = std::fs::write(dir.join("gui.json"), body.to_string());
    }
    loop {
        let Ok((sock, _)) = listener.accept().await else {
            continue;
        };
        let app = app.clone();
        let orch = orch.clone();
        let pending = pending.clone();
        let desk = desk.clone();
        tokio::spawn(async move {
            let _ = answer(sock, &app, &orch, &pending, &desk).await;
        });
    }
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
    BufReader::new(read.take(MAX_REQUEST))
        .read_line(&mut line)
        .await
        .map_err(|e| e.to_string())?;
    let reply = if line.len() as u64 >= MAX_REQUEST {
        json!({ "ok": false, "error": "request too large" })
    } else {
        let req: Value = serde_json::from_str(&line).unwrap_or(json!({}));
        if req.get("token").and_then(Value::as_str) == Some(desk.token.as_str()) {
            dispatch(app, orch, pending, desk, &req).await
        } else {
            json!({ "ok": false, "error": "bad token" })
        }
    };
    let mut out = serde_json::to_string(&reply)
        .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"encode\"}".into());
    out.push('\n');
    write
        .write_all(out.as_bytes())
        .await
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
        "tab.read" => tab_read(desk).await,
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

async fn tab_read(desk: &Desk) -> Value {
    let st = desk.state.lock().await;
    let tab = st
        .tabs
        .iter()
        .find(|t| t.id == st.active && t.kind == "browser")
        .or_else(|| st.tabs.iter().rev().find(|t| t.kind == "browser"));
    match tab {
        Some(t) => json!({ "ok": true, "id": t.id, "title": t.title, "url": t.url }),
        None => json!({ "ok": true, "id": "", "title": "", "url": "" }),
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
        if e.is_empty() {
            "medium".into()
        } else {
            e
        }
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
        orch.spawn(
            "default",
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
