//! Headless ParziOS control socket.
//!
//! Bound to 127.0.0.1. A remote client arrives through an SSH tunnel.
//! The desk and this process must not both own `~/.parzi`.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs4::fs_std::FileExt;
use parzi_core::store::{Event, SessionMeta, SessionStore};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::desk;
use crate::orchestrator::Orchestrator;
use crate::tools::{humanize_tool_call, Approval, Approver, AutoApprover, ToolCallInfo};

const MAX_REQUEST: u64 = 1024 * 1024;

struct Pending {
    call: ToolCallInfo,
    tx: oneshot::Sender<Approval>,
}

struct SlotGuard {
    key: String,
    map: Arc<std::sync::Mutex<HashMap<String, Pending>>>,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.key);
    }
}

/// Approvals that wait until a client answers on the socket.
pub struct ParkedApprover {
    pending: Arc<std::sync::Mutex<HashMap<String, Pending>>>,
    store: SessionStore,
}

impl ParkedApprover {
    fn new(store: SessionStore) -> Arc<Self> {
        Arc::new(Self {
            pending: Arc::default(),
            store,
        })
    }

    #[must_use]
    pub fn list(&self) -> Vec<Value> {
        let map = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let mut rows: Vec<Value> = map.iter().map(|(k, p)| listed(k, &p.call)).collect();
        rows.sort_by(|a, b| {
            a.get("key")
                .and_then(Value::as_str)
                .unwrap_or("")
                .cmp(b.get("key").and_then(Value::as_str).unwrap_or(""))
        });
        rows
    }

    pub fn answer(&self, key: &str, allow: bool) -> Result<(), String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("an approval key is required".into());
        }
        let pending = {
            let mut map = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            map.remove(key)
        };
        let Some(pending) = pending else {
            return Err("approval expired or unknown".into());
        };
        let decision = if allow {
            Approval::Allow
        } else {
            Approval::Deny
        };
        if pending.tx.send(decision).is_err() {
            return Err("the run is no longer waiting".into());
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Approver for ParkedApprover {
    async fn approve(&self, call: &ToolCallInfo) -> Approval {
        let key = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            map.insert(
                key.clone(),
                Pending {
                    call: call.clone(),
                    tx,
                },
            );
        }
        let _slot = SlotGuard {
            key: key.clone(),
            map: self.pending.clone(),
        };
        let text = format!(
            "waiting for approval {key}: {}",
            humanize_tool_call(&call.name, &call.args)
        );
        let _ = self.store.append(&call.session, &Event::System { text });
        rx.await.unwrap_or(Approval::Deny)
    }
}

fn listed(key: &str, call: &ToolCallInfo) -> Value {
    json!({
        "key": key,
        "session": call.session,
        "name": call.name,
        "lane": call.lane,
        "label": humanize_tool_call(&call.name, &call.args),
    })
}

struct State {
    orch: Arc<Orchestrator>,
    token: String,
    approver: Arc<ParkedApprover>,
}

/// A running localhost socket. Dropping it stops the accept loop and the queue.
#[must_use]
pub struct Daemon {
    pub port: u16,
    pub approver: Arc<ParkedApprover>,
    cancel: CancellationToken,
    accept: Option<JoinHandle<()>>,
    pump: Option<JoinHandle<()>>,
    file: PathBuf,
    _lock: std::fs::File,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(task) = self.accept.take() {
            task.abort();
        }
        if let Some(task) = self.pump.take() {
            task.abort();
        }
        let _ = std::fs::remove_file(&self.file);
    }
}

/// Listen on 127.0.0.1 and run the orchestrator queue.
///
/// Refuses when the desk's socket is already answering, and when another
/// `parzi serve` holds the lock.
pub async fn start(orch: Arc<Orchestrator>) -> Result<Daemon, String> {
    if desk::desk_is_open().await {
        return Err("the desk is open. Close Parzi before `parzi serve`.".into());
    }
    let root = parzi_core::paths::ensure_dirs().map_err(|e| e.to_string())?;
    let lock = lock_serve(&root.join("serve.lock"))?;
    if desk::desk_is_open().await {
        return Err("the desk is open. Close Parzi before `parzi serve`.".into());
    }
    let recovered = orch.recover().map_err(|e| e.to_string())?;
    if recovered > 0 {
        tracing::info!("marked {recovered} interrupted run(s) idle");
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("could not listen on 127.0.0.1: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("socket has no address: {e}"))?;
    if !addr.ip().is_loopback() {
        return Err("refusing to listen on a non-loopback address".into());
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    let file = root.join("serve.json");
    write_serve_file(
        &file,
        &json!({ "port": addr.port(), "token": token }).to_string(),
    )?;
    let approver = ParkedApprover::new(orch.store().clone());
    let cancel = CancellationToken::new();
    let state = Arc::new(State {
        orch: orch.clone(),
        token,
        approver: approver.clone(),
    });
    let accept_cancel = cancel.clone();
    let accept = tokio::spawn(async move {
        loop {
            tokio::select! {
                () = accept_cancel.cancelled() => break,
                incoming = listener.accept() => {
                    let Ok((sock, peer)) = incoming else { continue };
                    if !peer.ip().is_loopback() {
                        continue;
                    }
                    let state = state.clone();
                    tokio::spawn(async move {
                        let _ = answer(sock, &state).await;
                    });
                }
            }
        }
    });
    let pump_orch = orch.clone();
    let pump = tokio::spawn(async move {
        if let Err(e) = pump_orch.recover_queue().await {
            tracing::warn!("queued runs not recovered: {e}");
        }
        pump_orch.kick().await;
        pump_orch.pump_loop().await;
    });
    Ok(Daemon {
        port: addr.port(),
        approver,
        cancel,
        accept: Some(accept),
        pump: Some(pump),
        file,
        _lock: lock,
    })
}

/// User-service unit. Setup writes it on Linux and does not enable it.
#[must_use]
pub fn systemd_unit(exe: &Path) -> String {
    let quoted = quote_exec(&exe.display().to_string());
    format!(
        "[Unit]\n\
         Description=ParziOS\n\
         After=default.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={quoted} serve\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         NoNewPrivileges=true\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

fn quote_exec(path: &str) -> String {
    let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn lock_serve(path: &Path) -> Result<std::fs::File, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("serve lock: {e}"))?;
    match file.try_lock_exclusive() {
        Ok(true) => Ok(file),
        Ok(false) => Err("parzi serve is already running".into()),
        Err(e) => Err(format!("serve lock: {e}")),
    }
}

fn write_serve_file(path: &Path, body: &str) -> Result<(), String> {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    parzi_core::atomic_write(path, body.as_bytes()).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

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

async fn answer(sock: tokio::net::TcpStream, state: &State) -> Result<(), String> {
    let (read, mut write) = sock.into_split();
    let mut line = String::new();
    BufReader::new(read.take(MAX_REQUEST))
        .read_line(&mut line)
        .await
        .map_err(|e| e.to_string())?;
    let reply = if line.len() as u64 >= MAX_REQUEST {
        json!({ "ok": false, "error": "request too large" })
    } else {
        let req: Value = serde_json::from_str(line.trim()).unwrap_or_else(|_| json!({}));
        let token = req.get("token").and_then(Value::as_str).unwrap_or("");
        if tokens_equal(token, &state.token) {
            dispatch(state, &req).await
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

async fn dispatch(state: &State, req: &Value) -> Value {
    let op = req.get("op").and_then(Value::as_str).unwrap_or("");
    match op {
        "health" => json!({
            "ok": true,
            "serve": "parzi-os",
            "version": env!("CARGO_PKG_VERSION"),
        }),
        "session.list" => session_list(&state.orch),
        "session.show" | "session.export" => session_show(&state.orch, req, op == "session.export"),
        "session.kill" => session_kill(&state.orch, req).await,
        "session.fork" => session_fork(&state.orch, req).await,
        "session.rename" => session_rename(&state.orch, req),
        "session.delete" => delete_session(&state.orch, req).await,
        "session.send" => session_send(state, req).await,
        "approval.list" => json!({ "ok": true, "approvals": state.approver.list() }),
        "approval.answer" => approval_answer(&state.approver, req),
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

fn fail(error: impl Into<String>) -> Value {
    json!({ "ok": false, "error": error.into() })
}

fn resolve(orch: &Orchestrator, id: &str) -> Result<String, Value> {
    orch.store().resolve_id(id).map_err(|e| fail(e.to_string()))
}

fn session_list(orch: &Orchestrator) -> Value {
    match orch.store().list() {
        Ok(rows) => json!({ "ok": true, "sessions": rows }),
        Err(e) => fail(e.to_string()),
    }
}

fn session_show(orch: &Orchestrator, req: &Value, export: bool) -> Value {
    let id = match resolve(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(v) => return v,
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
    let id = match resolve(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(v) => return v,
    };
    match orch.kill(&id).await {
        Ok(()) => json!({ "ok": true, "id": id }),
        Err(e) => fail(e.to_string()),
    }
}

async fn session_fork(orch: &Orchestrator, req: &Value) -> Value {
    let id = match resolve(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(v) => return v,
    };
    let at = req
        .get("at")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok());
    match orch.fork(&id, at).await {
        Ok(meta) => json!({ "ok": true, "id": meta.id }),
        Err(e) => fail(e.to_string()),
    }
}

fn session_rename(orch: &Orchestrator, req: &Value) -> Value {
    let id = match resolve(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(v) => return v,
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

async fn delete_session(orch: &Orchestrator, req: &Value) -> Value {
    let id = match resolve(orch, &str_arg(req, "id")) {
        Ok(id) => id,
        Err(v) => return v,
    };
    let all = match orch.store().list() {
        Ok(all) => all,
        Err(e) => return fail(e.to_string()),
    };
    let by_id: HashMap<&str, &SessionMeta> = all.iter().map(|m| (m.id.as_str(), m)).collect();
    let mut doomed = vec![id.clone()];
    for meta in &all {
        let mut parent = meta.parent_id.as_deref();
        let mut guard = 0;
        while let Some(pid) = parent {
            if pid == id {
                doomed.push(meta.id.clone());
                break;
            }
            if guard > 50 {
                break;
            }
            guard += 1;
            parent = by_id.get(pid).and_then(|row| row.parent_id.as_deref());
        }
    }
    for sid in &doomed {
        let _ = orch.kill(sid).await;
        orch.drop_shells(sid);
    }
    match orch.store().delete_thread(&id) {
        Ok(n) => json!({ "ok": true, "id": id, "removed": n }),
        Err(e) => fail(e.to_string()),
    }
}

async fn session_send(state: &State, req: &Value) -> Value {
    let message = str_arg(req, "message");
    if message.is_empty() {
        return fail("empty message");
    }
    let target = str_arg(req, "target");
    let model = str_arg(req, "model");
    let cwd = str_arg(req, "cwd");
    let effort = {
        let effort = str_arg(req, "effort");
        if effort.is_empty() {
            "medium".into()
        } else {
            effort
        }
    };
    let yes = req.get("yes").and_then(Value::as_bool).unwrap_or(false);
    let approver: Arc<dyn Approver> = if yes {
        Arc::new(AutoApprover)
    } else {
        state.approver.clone()
    };
    let sent = if target.is_empty() || target == "new" {
        state
            .orch
            .spawn(
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
            .map(|(meta, rx)| {
                drop(rx);
                meta.id
            })
    } else {
        match resolve(&state.orch, &target) {
            Ok(id) => state
                .orch
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
                .map(|rx| {
                    drop(rx);
                    id
                }),
            Err(v) => return v,
        }
    };
    let id = match sent {
        Ok(id) => id,
        Err(e) => return fail(e.to_string()),
    };
    let status = state
        .orch
        .store()
        .get(&id)
        .ok()
        .map(|m| serde_json::to_value(m.status).unwrap_or(json!("idle")))
        .unwrap_or(json!("idle"));
    json!({ "ok": true, "id": id, "status": status, "serve": true })
}

fn approval_answer(approver: &ParkedApprover, req: &Value) -> Value {
    let key = str_arg(req, "key");
    let Some(allow) = req.get("allow").and_then(Value::as_bool) else {
        return fail("allow true or false is required");
    };
    match approver.answer(&key, allow) {
        Ok(()) => json!({ "ok": true, "key": key }),
        Err(e) => fail(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_quotes_the_binary_and_stays_off_the_network() {
        let unit = systemd_unit(Path::new("/usr/local/bin/parzi"));
        assert!(unit.contains("ExecStart=\"/usr/local/bin/parzi\" serve"));
        assert!(unit.contains("NoNewPrivileges=true"));
        assert!(!unit.contains("0.0.0.0"));
        let spaced = systemd_unit(Path::new("/opt/Parzi OS/parzi"));
        assert!(spaced.contains("ExecStart=\"/opt/Parzi OS/parzi\" serve"));
    }

    #[test]
    fn token_compare_rejects_a_near_miss() {
        assert!(tokens_equal("abc", "abc"));
        assert!(!tokens_equal("abc", "abd"));
        assert!(!tokens_equal("abc", "abcd"));
        assert!(!tokens_equal("", "a"));
    }
}
