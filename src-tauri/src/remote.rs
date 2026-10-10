//! The desk's door to ParziOS. A remote session is an ordinary session
//! whose id carries the `r:` prefix: the session commands route it here,
//! and its live events arrive on `parzi://run-event` like local ones. The
//! work itself lives in `parzi_runtime::remote`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parzi_core::store::{Event, SessionMeta};
use parzi_providers::ProviderStatus;
use parzi_runtime::remote::{self, Link, Progress, SetupOptions, Target, Transport, REMOTE_BIN};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

pub const PREFIX: &str = "r:";
/// A session another device ran and mirrored to the server:
/// `m:<device>:<session>`. Read here; continuing it moves it to the server.
pub const MIRROR: &str = "m:";
/// After a failed connect, background refreshes wait this long to retry.
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// While linked, everything syncs at least this often.
const SYNC_EVERY: Duration = Duration::from_secs(120);
/// A change here waits this long for more before it syncs.
const SYNC_SETTLE: Duration = Duration::from_secs(3);

/// The server's own id for a remote session id, or None for a local one.
#[must_use]
pub fn remote_id(id: &str) -> Option<&str> {
    id.strip_prefix(PREFIX)
}

/// (device, session) of a mirrored session id.
#[must_use]
pub fn mirror_id(id: &str) -> Option<(&str, &str)> {
    id.strip_prefix(MIRROR)?.split_once(':')
}

fn tag(id: &str) -> String {
    format!("{PREFIX}{id}")
}

#[derive(Default)]
pub struct RemoteState {
    link: Mutex<Option<Arc<Link>>>,
    busy: AtomicBool,
    /// The server's sessions and other devices' mirrors as last listed,
    /// ids already tagged.
    sessions: std::sync::Mutex<Vec<SessionMeta>>,
    failed_at: std::sync::Mutex<Option<Instant>>,
    opening: AtomicBool,
    syncing: Mutex<()>,
    sync_pending: AtomicBool,
    synced_at: std::sync::Mutex<i64>,
}

#[derive(serde::Serialize)]
pub struct RemoteInfo {
    pub label: String,
    pub user: String,
    pub host: String,
    pub version: String,
    pub linked_at: i64,
    pub alive: bool,
    /// Last finished sync (ms), 0 for not yet.
    pub synced_at: i64,
    /// This machine's device id.
    pub device: String,
}

fn info(saved: &remote::Saved, alive: bool, synced_at: i64) -> RemoteInfo {
    RemoteInfo {
        label: saved.target.label(),
        user: saved.target.user.clone(),
        host: match saved.target.port {
            Some(p) => format!("{}:{p}", saved.target.host),
            None => saved.target.host.clone(),
        },
        version: saved.version.clone(),
        linked_at: saved.linked_at,
        alive,
        synced_at,
        device: parzi_core::device::id(),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Settings, notes and this device's sessions, once; never two at a time.
pub(crate) async fn sync_now(app: &AppHandle) -> Result<String, String> {
    let st = app.state::<RemoteState>();
    let _one = st.syncing.lock().await;
    let link = link(app, &st).await?;
    let server = remote::load_saved()
        .map(|s| s.target.label())
        .ok_or("no remote is set up")?;
    let local = app.state::<crate::AppState>();
    let done = parzi_runtime::sync::everything(&link, local.orch.store(), &server).await;
    if let parzi_runtime::sync::SettingsOutcome::Pulled(cfg) = &done.settings {
        local.orch.apply_config((**cfg).clone()).await;
        status(app, "settings", "");
    }
    if done.brain.changed() {
        status(app, "brain", "");
    }
    *st.synced_at.lock().unwrap_or_else(|p| p.into_inner()) = now_ms();
    let summary = done.summary();
    if done.errors.is_empty() {
        status(app, "synced", summary.clone());
        Ok(summary)
    } else {
        let why = done.errors.join("; ");
        status(app, "error", format!("Sync with {server}: {why}"));
        Err(why)
    }
}

/// Sync a few seconds from now; changes in between ride along.
pub(crate) fn sync_soon(app: &AppHandle) {
    if remote::load_saved().is_none() {
        return;
    }
    let st = app.state::<RemoteState>();
    if st.sync_pending.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SYNC_SETTLE).await;
        app.state::<RemoteState>()
            .sync_pending
            .store(false, Ordering::Release);
        if let Err(e) = sync_now(&app).await {
            tracing::info!("sync skipped: {e}");
        }
    });
}

/// While a server is linked and reachable, sync on a timer.
pub(crate) fn start_ticker(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(SYNC_EVERY).await;
            let live = {
                let st = app.state::<RemoteState>();
                let slot = st.link.lock().await;
                slot.as_ref().is_some_and(|l| l.alive())
            };
            if live {
                let _ = sync_now(&app).await;
            }
        }
    });
}

/// Connection news for the UI: `up`, `down`, `updating`, `ready`, `error`,
/// `resync` (the stream restarted, reload what is shown).
fn status(app: &AppHandle, state: &str, detail: impl Into<String>) {
    let _ = app.emit(
        "parzi://remote",
        json!({ "state": state, "detail": detail.into() }),
    );
}

/// `0.1.9 < 0.1.23`: dotted numbers compared as numbers.
fn older(a: &str, b: &str) -> bool {
    let key = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    key(a) < key(b)
}

/// The live link, opened on first use. A server older than this desk is
/// brought up to date first; if that fails the old one is still used.
pub(crate) async fn link(app: &AppHandle, state: &RemoteState) -> Result<Arc<Link>, String> {
    let mut slot = state.link.lock().await;
    if let Some(l) = slot.as_ref().filter(|l| l.alive()) {
        return Ok(l.clone());
    }
    let mut saved = remote::load_saved().ok_or("no remote is set up")?;
    let label = saved.target.label();
    let opened = Link::open(&Transport::ssh(&saved.target)).await;
    let mut link = match opened {
        Ok(l) => l,
        Err(e) => {
            *state.failed_at.lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
            return Err(format!("{label}: {e}"));
        }
    };
    let ours = env!("CARGO_PKG_VERSION");
    if older(&link.version, ours) {
        status(
            app,
            "updating",
            format!("Updating Parzi on {label} to {ours}…"),
        );
        drop(link);
        let quiet = |_: Progress| {};
        match remote::upgrade(&saved, &quiet).await {
            Ok(next) => {
                saved = next;
                status(app, "ready", format!("Parzi on {label} is now {ours}"));
            }
            Err(e) => status(
                app,
                "error",
                format!("Could not update Parzi on {label}: {e}"),
            ),
        }
        link = Link::open(&Transport::ssh(&saved.target))
            .await
            .map_err(|e| format!("{label}: {e}"))?;
    }
    let events = link.subscribe().await?;
    let link = Arc::new(link);
    *slot = Some(link.clone());
    *state.failed_at.lock().unwrap_or_else(|p| p.into_inner()) = None;
    tokio::spawn(forward(app.clone(), events));
    status(app, "up", label);
    sync_soon(app);
    Ok(link)
}

/// Remote events to the desk's event channel, with ids tagged `r:`.
async fn forward(app: AppHandle, mut rx: tokio::sync::mpsc::UnboundedReceiver<Value>) {
    while let Some(mut ev) = rx.recv().await {
        match ev.get("kind").and_then(Value::as_str) {
            Some("resync" | "lagged") => {
                status(&app, "resync", "");
                continue;
            }
            None => continue,
            Some(_) => {}
        }
        // A finished run on the server may have written notes there.
        let ended = matches!(
            ev.get("kind").and_then(Value::as_str),
            Some("done" | "error")
        );
        retag(&mut ev);
        let _ = app.emit("parzi://run-event", &ev);
        if ended {
            sync_soon(&app);
        }
    }
    status(&app, "down", "");
}

fn retag(ev: &mut Value) {
    for field in ["session", "key"] {
        if let Some(s) = ev.get(field).and_then(Value::as_str).map(tag) {
            ev[field] = json!(s);
        }
    }
    if let Some(call) = ev.get_mut("call") {
        if let Some(s) = call.get("session").and_then(Value::as_str).map(tag) {
            call["session"] = json!(s);
        }
    }
}

fn wait_for(op: &str) -> Duration {
    match op {
        "providers" | "doctor" | "session.compact" => Duration::from_secs(100),
        "session.send" | "session.kill" | "session.delete" => Duration::from_secs(70),
        _ => Duration::from_secs(30),
    }
}

pub(crate) async fn call(
    app: &AppHandle,
    state: &RemoteState,
    op: &str,
    body: Value,
) -> Result<Value, String> {
    link(app, state).await?.call(op, body, wait_for(op)).await
}

fn tagged_meta(v: Value) -> Option<SessionMeta> {
    let mut meta: SessionMeta = serde_json::from_value(v).ok()?;
    meta.id = tag(&meta.id);
    meta.parent_id = meta.parent_id.as_deref().map(tag);
    Some(meta)
}

/// The server's sessions for the desk's list. Never waits on a connect:
/// without a live link it starts one in the background and answers from
/// the last list.
pub(crate) async fn sessions(app: &AppHandle, state: &RemoteState) -> Vec<SessionMeta> {
    if remote::load_saved().is_none() {
        return vec![];
    }
    let live = match state.link.try_lock() {
        Ok(slot) => slot.as_ref().filter(|l| l.alive()).cloned(),
        Err(_) => None,
    };
    let Some(link) = live else {
        connect_in_background(app, state);
        return state
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
    };
    let fresh = tokio::time::timeout(
        Duration::from_secs(4),
        link.call("session.list", json!({}), Duration::from_secs(4)),
    )
    .await;
    if let Ok(Ok(v)) = fresh {
        let mut rows: Vec<SessionMeta> = v
            .get("sessions")
            .and_then(Value::as_array)
            .map(|a| a.iter().cloned().filter_map(tagged_meta).collect())
            .unwrap_or_default();
        // Other devices' sessions, as their mirrors. This device's own
        // mirrors are its local sessions, already listed.
        let me = parzi_core::device::id();
        if let Ok(Ok(m)) = tokio::time::timeout(
            Duration::from_secs(4),
            link.call("mirror.list", json!({}), Duration::from_secs(4)),
        )
        .await
        {
            for card in m
                .get("sessions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let device = card.get("device").and_then(Value::as_str).unwrap_or("");
                if device.is_empty() || device == me {
                    continue;
                }
                if let Some(meta) = mirrored_meta(card) {
                    rows.push(meta);
                }
            }
        }
        *state.sessions.lock().unwrap_or_else(|p| p.into_inner()) = rows;
    }
    state
        .sessions
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

fn connect_in_background(app: &AppHandle, state: &RemoteState) {
    let recent = state
        .failed_at
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_some_and(|t| t.elapsed() < RETRY_AFTER);
    if recent || state.opening.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<RemoteState>();
        if let Err(e) = link(&app, &st).await {
            tracing::info!("remote not linked: {e}");
        }
        st.opening.store(false, Ordering::Release);
    });
}

/// A mirror card's session, its id tagged `m:<device>:` and its title
/// saying where it ran. It reads as done: its run lives on that device.
fn mirrored_meta(card: &Value) -> Option<SessionMeta> {
    let device = card.get("device").and_then(Value::as_str)?;
    let name = card.get("name").and_then(Value::as_str).unwrap_or(device);
    let mut meta: SessionMeta = serde_json::from_value(card.get("session")?.clone()).ok()?;
    meta.id = format!("{MIRROR}{device}:{}", meta.id);
    meta.parent_id = meta.parent_id.map(|p| format!("{MIRROR}{device}:{p}"));
    meta.title = format!("{} · {name}", meta.title);
    if matches!(
        meta.status,
        parzi_core::store::SessionStatus::Active | parzi_core::store::SessionStatus::Queued
    ) {
        meta.status = parzi_core::store::SessionStatus::Idle;
    }
    Some(meta)
}

fn events_of(v: &Value) -> Vec<Event> {
    v.get("events")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| serde_json::from_value(e.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) async fn mirror_thread(
    app: &AppHandle,
    state: &RemoteState,
    device: &str,
    id: &str,
) -> Result<(SessionMeta, Vec<Event>), String> {
    let v = call(
        app,
        state,
        "mirror.events",
        json!({ "device": device, "id": id }),
    )
    .await?;
    let card = json!({ "device": device, "name": v.get("name"), "session": v.get("session") });
    let meta = mirrored_meta(&card).ok_or("the server sent no session")?;
    Ok((meta, events_of(&v)))
}

/// Copy another device's session into the server's own store so it can go
/// on there. Returns the server's id for it (untagged).
pub(crate) async fn adopt(
    app: &AppHandle,
    state: &RemoteState,
    device: &str,
    id: &str,
) -> Result<String, String> {
    let v = call(
        app,
        state,
        "mirror.adopt",
        json!({ "device": device, "id": id }),
    )
    .await?;
    v.get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "the server could not take the session over".into())
}

pub(crate) async fn thread(
    app: &AppHandle,
    state: &RemoteState,
    id: &str,
) -> Result<(SessionMeta, Vec<Event>), String> {
    let v = call(app, state, "session.events", json!({ "id": id, "from": 0 })).await?;
    let meta = v
        .get("session")
        .cloned()
        .and_then(tagged_meta)
        .ok_or("the server sent no session")?;
    let events = v
        .get("events")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| serde_json::from_value(e.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    Ok((meta, events))
}

/// Start or continue a session on the server. Attachments were read here;
/// they travel inline.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn send(
    app: &AppHandle,
    state: &RemoteState,
    target: Option<&str>,
    model: &str,
    prompt: &str,
    cwd: &str,
    effort: &str,
    attachments: Vec<parzi_core::context::AttachedFile>,
    mode: Option<String>,
    lane: Option<String>,
) -> Result<String, String> {
    let body = json!({
        "target": target.unwrap_or("new"),
        "message": prompt,
        "model": model,
        "effort": effort,
        "mode": mode.unwrap_or_default(),
        "lane": lane.unwrap_or_default(),
        "attachments": attachments,
        // A folder on the server (a project there), or its scratch folder.
        "cwd": if cwd.starts_with('/') || cwd.starts_with('~') { cwd } else { "" },
    });
    let v = call(app, state, "session.send", body).await?;
    let id = v
        .get("id")
        .and_then(Value::as_str)
        .ok_or("the server sent no id")?;
    Ok(tag(id))
}

pub(crate) async fn fork(
    app: &AppHandle,
    state: &RemoteState,
    id: &str,
) -> Result<SessionMeta, String> {
    let v = call(app, state, "session.fork", json!({ "id": id })).await?;
    v.get("session")
        .cloned()
        .and_then(tagged_meta)
        .ok_or_else(|| "update Parzi on the server to fork there".into())
}

/// The saved remote, if any. Never opens a connection.
#[tauri::command]
pub async fn remote_info(state: State<'_, RemoteState>) -> Result<Option<RemoteInfo>, String> {
    let alive = state
        .link
        .try_lock()
        .ok()
        .and_then(|l| l.as_ref().map(|l| l.alive()))
        .unwrap_or(false);
    let synced = *state.synced_at.lock().unwrap_or_else(|p| p.into_inner());
    Ok(remote::load_saved().map(|s| info(&s, alive, synced)))
}

/// Open the link now (the composer's switch): connects, updates the
/// server when it is older, and starts the event stream.
#[tauri::command]
pub async fn remote_connect(
    app: AppHandle,
    state: State<'_, RemoteState>,
) -> Result<RemoteInfo, String> {
    link(&app, &state).await?;
    let saved = remote::load_saved().ok_or("no remote is set up")?;
    let synced = *state.synced_at.lock().unwrap_or_else(|p| p.into_inner());
    Ok(info(&saved, true, synced))
}

/// Projects as the server sees them; `folder` is its own folder for each
/// ("" when the server has none yet).
#[tauri::command]
pub async fn remote_projects(
    app: AppHandle,
    state: State<'_, RemoteState>,
) -> Result<Vec<parzi_core::brain::Project>, String> {
    let v = call(&app, &state, "project.list", json!({})).await?;
    serde_json::from_value(v.get("projects").cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("the server's project list is unreadable: {e}"))
}

/// Give a project a folder on the server (a path there), or start one.
#[tauri::command]
pub async fn remote_project_folder(
    app: AppHandle,
    state: State<'_, RemoteState>,
    slug: Option<String>,
    title: String,
    folder: String,
) -> Result<parzi_core::brain::Project, String> {
    let v = call(
        &app,
        &state,
        "project.folder",
        json!({ "slug": slug, "title": title, "folder": folder }),
    )
    .await?;
    sync_soon(&app);
    serde_json::from_value(v.get("project").cloned().unwrap_or(Value::Null))
        .map_err(|e| e.to_string())
}

/// Sync now (Settings › Connections). Answers what moved.
#[tauri::command]
pub async fn remote_sync(app: AppHandle) -> Result<String, String> {
    sync_now(&app).await
}

/// The agents on the server, for the composer's model list.
#[tauri::command]
pub async fn remote_providers(
    app: AppHandle,
    state: State<'_, RemoteState>,
    refresh: Option<bool>,
) -> Result<Vec<ProviderStatus>, String> {
    let v = call(
        &app,
        &state,
        "providers",
        json!({ "refresh": refresh.unwrap_or(false) }),
    )
    .await?;
    serde_json::from_value(v.get("providers").cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("the server's agent list is unreadable: {e}"))
}

struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Set a Linux machine up and link to it. Progress arrives as
/// `parzi://remote-setup` events. The password is used for this call only
/// and is never stored.
#[tauri::command]
pub async fn remote_setup(
    app: AppHandle,
    state: State<'_, RemoteState>,
    user: String,
    host: String,
    password: Option<String>,
    notes: bool,
) -> Result<RemoteInfo, String> {
    if state.busy.swap(true, Ordering::AcqRel) {
        return Err("a setup is already running".into());
    }
    let _busy = Busy(&state.busy);
    let target = Target::parse(&user, &host)?;
    let opts = SetupOptions {
        password: password.filter(|p| !p.is_empty()),
        notes,
        ..Default::default()
    };
    let emit = |p: Progress| {
        let _ = app.emit("parzi://remote-setup", &p);
    };
    let (saved, setup_link) = remote::setup(target, opts, &emit).await?;
    drop(setup_link);
    *state.link.lock().await = None;
    state
        .sessions
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clear();
    link(&app, &state).await?;
    let synced = *state.synced_at.lock().unwrap_or_else(|p| p.into_inner());
    Ok(info(&saved, true, synced))
}

/// Install an agent on the remote, or sign in to it, in a terminal the
/// user watches: vendor installers and logins are interactive.
#[tauri::command]
pub async fn remote_agent(provider: String, action: String) -> Result<(), String> {
    let id = parzi_providers::canonical_id(&provider)
        .ok_or_else(|| format!("unknown agent {provider}"))?;
    let verb = match action.as_str() {
        "install" => "install",
        "login" => "login",
        other => return Err(format!("unknown action {other}")),
    };
    let saved = remote::load_saved().ok_or("no remote is set up")?;
    let args = Transport::ssh(&saved.target)
        .terminal_args(&format!("{REMOTE_BIN} agent {verb} {id}"))
        .ok_or("this remote has no terminal")?;
    let line = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    let script = if cfg!(windows) {
        format!("& {line}")
    } else {
        line
    };
    let name = parzi_providers::display_name(id);
    let title = if verb == "install" {
        format!("Install {name} on {}", saved.target.label())
    } else {
        format!("Sign in to {name} on {}", saved.target.label())
    };
    crate::onboard::open_terminal(&title, &script)
}

fn quote(arg: &str) -> String {
    if cfg!(windows) {
        let mut out = String::from("'");
        for c in arg.chars() {
            if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
                out.push(c);
            }
            out.push(c);
        }
        out.push('\'');
        out
    } else {
        remote::sh_quote(arg)
    }
}

/// Unlink: close the link and drop the saved remote. The engine on the
/// other machine keeps running.
#[tauri::command]
pub async fn remote_forget(state: State<'_, RemoteState>) -> Result<(), String> {
    *state.link.lock().await = None;
    state
        .sessions
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clear();
    remote::forget()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_arguments_are_quoted_for_the_local_shell() {
        if cfg!(windows) {
            assert_eq!(quote("it's"), "'it''s'");
            assert_eq!(quote("D\u{2019}Angelo"), "'D\u{2019}\u{2019}Angelo'");
            assert_eq!(
                quote("~/.local/bin/parzi agent login claude"),
                "'~/.local/bin/parzi agent login claude'"
            );
        } else {
            assert_eq!(quote("it's"), r"'it'\''s'");
        }
    }

    #[test]
    fn remote_ids_carry_their_prefix_both_ways() {
        assert_eq!(remote_id("r:abc"), Some("abc"));
        assert_eq!(remote_id("abc"), None);
        let mut ev = json!({
            "kind": "approval", "key": "k1", "session": "s1",
            "call": { "session": "s1", "name": "shell.exec" },
        });
        retag(&mut ev);
        assert_eq!(ev["key"], "r:k1");
        assert_eq!(ev["session"], "r:s1");
        assert_eq!(ev["call"]["session"], "r:s1");
        let mut text = json!({ "kind": "text", "session": "s2", "text": "hi" });
        retag(&mut text);
        assert_eq!(text["session"], "r:s2");
        assert_eq!(text["text"], "hi");
    }

    #[test]
    fn only_an_older_server_is_upgraded() {
        assert!(older("0.1.9", "0.1.23"));
        assert!(older("0.1.23", "0.1.24"));
        assert!(!older("0.1.24", "0.1.24"));
        assert!(!older("0.2.0", "0.1.24"));
    }
}
