#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::store::SessionStore;
use parzi_runtime::handler::RunEvent;
use parzi_runtime::tools::{Approval, Approver, AskRequest, Asker, ToolCallInfo};
use parzi_runtime::Orchestrator;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{broadcast, oneshot, Mutex};

#[cfg(target_os = "windows")]
mod dwm;

mod adblock;
mod appearance;
mod brain;
mod browser;
mod control;
mod files;
mod job;
mod onboard;
mod pagectl;
mod remote;
mod search;
mod sessions;
mod settings;
mod vault;

pub(crate) type Pending = Arc<Mutex<HashMap<String, (String, oneshot::Sender<Approval>)>>>;
pub(crate) type PendingQuestions = Arc<Mutex<HashMap<String, (String, oneshot::Sender<String>)>>>;

pub(crate) struct AppState {
    pub(crate) orch: Arc<Orchestrator>,
    pub(crate) pending: Pending,
    pub(crate) questions: PendingQuestions,
    pub(crate) app: AppHandle,
    pub(crate) desk: Arc<control::Desk>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum UiEvent {
    Text {
        session: String,
        text: String,
    },
    Reasoning {
        session: String,
        text: String,
    },
    ToolCall {
        session: String,
        id: String,
        name: String,
        label: String,
    },
    ToolResult {
        session: String,
        id: String,
        name: String,
        ok: bool,
        ms: u64,
    },
    Notice {
        session: String,
        text: String,
    },
    Context {
        session: String,
        used: u64,
        limit: u64,
    },
    Approval {
        key: String,
        session: String,
        call: ToolCallView,
    },
    Question {
        key: String,
        session: String,
        question: String,
        options: Vec<String>,
    },
    Request {
        key: String,
        session: String,
        request: String,
        req_kind: String,
    },
    Done {
        session: String,
        turns: u32,
    },
    Error {
        session: String,
        error: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ToolCallView {
    id: String,
    name: String,
    args: serde_json::Value,
    lane: String,
    session: String,
}

pub(crate) struct GuiApprover {
    pub(crate) app: AppHandle,
    pub(crate) pending: Pending,
}

#[async_trait::async_trait]
impl Approver for GuiApprover {
    async fn approve(&self, call: &ToolCallInfo) -> Approval {
        let key = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .await
            .insert(key.clone(), (call.session.clone(), tx));
        let view = ToolCallView {
            id: call.id.clone(),
            name: call.name.clone(),
            args: call.args.clone(),
            lane: call.lane.clone(),
            session: call.session.clone(),
        };
        let _ = self.app.emit(
            "parzi://run-event",
            UiEvent::Approval {
                key: key.clone(),
                session: call.session.clone(),
                call: view,
            },
        );
        let out = tokio::time::timeout(std::time::Duration::from_secs(120), rx).await;
        self.pending.lock().await.remove(&key);
        match out {
            Ok(Ok(a)) => a,
            _ => Approval::Deny,
        }
    }
}

pub(crate) struct GuiAsker {
    pub(crate) app: AppHandle,
    pub(crate) pending: PendingQuestions,
}

#[async_trait::async_trait]
impl Asker for GuiAsker {
    async fn ask(&self, req: &AskRequest) -> String {
        let key = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .await
            .insert(key.clone(), (req.session.clone(), tx));
        let _ = self.app.emit(
            "parzi://run-event",
            if req.kind.is_empty() {
                UiEvent::Question {
                    key: key.clone(),
                    session: req.session.clone(),
                    question: req.question.clone(),
                    options: req.options.clone(),
                }
            } else {
                UiEvent::Request {
                    key: key.clone(),
                    session: req.session.clone(),
                    request: req.question.clone(),
                    req_kind: req.kind.clone(),
                }
            },
        );
        let out = tokio::time::timeout(std::time::Duration::from_secs(300), rx).await;
        self.pending.lock().await.remove(&key);
        match out {
            Ok(Ok(a)) if !a.trim().is_empty() => a,
            _ => "the user didn't answer — decide yourself and say what you assumed".into(),
        }
    }
}

fn boot() -> anyhow::Result<(ParziConfig, SessionStore, Option<String>)> {
    parzi_core::paths::ensure_dirs()?;
    let (cfg, note) = load_or_recover()?;
    Ok((cfg, SessionStore::open()?, note))
}

fn load_or_recover() -> anyhow::Result<(ParziConfig, Option<String>)> {
    match ParziConfig::load() {
        Ok(cfg) => Ok((cfg, None)),
        Err(e) => {
            let path = parzi_core::paths::config_path()?;
            let why = config_fault(&e, &path);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let broken = path.with_extension(format!("broken-{stamp}.toml"));
            let _ = std::fs::rename(&path, &broken);
            if path.exists() {
                let note = format!(
                    "{} was unreadable ({why}) and could not be moved aside; defaults loaded, your file left untouched.",
                    path.display()
                );
                tracing::warn!("{note}");
                return Ok((ParziConfig::default(), Some(note)));
            }
            let cfg = ParziConfig::default();
            let _ = cfg.save();
            let note = format!(
                "{} was unreadable ({why}) and was set aside as {}. Defaults loaded.",
                path.display(),
                broken
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            );
            tracing::warn!("{note}");
            Ok((cfg, Some(note)))
        }
    }
}

/// Where the config broke, never what it says: a parse error quotes the
/// offending line, which may hold an API key, into the log and the dialog.
fn config_fault(e: &parzi_core::ParziError, path: &std::path::Path) -> String {
    match e {
        parzi_core::ParziError::TomlDe(de) => de
            .span()
            .and_then(|span| {
                let text = std::fs::read_to_string(path).ok()?;
                Some(line_col(&text, span.start))
            })
            .unwrap_or_else(|| "a syntax error".into()),
        parzi_core::ParziError::Io(io) => format!("read failed: {}", io.kind()),
        // Built by parzi-core from versions and paths, never file content.
        parzi_core::ParziError::Config(msg) => msg.clone(),
        _ => "an invalid value".into(),
    }
}

fn line_col(text: &str, at: usize) -> String {
    let head = text.get(..at).unwrap_or(text);
    let line = head.matches('\n').count() + 1;
    let col = head.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    format!("line {line}, column {col}")
}

#[tauri::command]
async fn app_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[tauri::command]
fn window_fullscreen(window: tauri::Window, on: bool) -> Result<(), String> {
    window.set_fullscreen(on).map_err(|e| e.to_string())
}

fn ui_from_run(session: String, ev: RunEvent) -> Option<UiEvent> {
    Some(match ev {
        RunEvent::Text(_)
        | RunEvent::Reasoning { .. }
        | RunEvent::Usage { .. }
        | RunEvent::ApprovalRequest { .. } => return None,
        RunEvent::ToolCall { id, name, label } => UiEvent::ToolCall {
            session,
            id,
            name,
            label,
        },
        RunEvent::ToolResult { id, name, ok, ms } => UiEvent::ToolResult {
            session,
            id,
            name,
            ok,
            ms,
        },
        RunEvent::Notice { text } => UiEvent::Notice { session, text },
        RunEvent::Context { used, limit } => UiEvent::Context {
            session,
            used,
            limit,
        },
        RunEvent::Done { turns } => UiEvent::Done { session, turns },
        RunEvent::Error(error) => UiEvent::Error { session, error },
    })
}

async fn forward_host_bus(app: AppHandle, mut rx: broadcast::Receiver<(String, RunEvent)>) {
    #[derive(Default)]
    struct Buf {
        text: String,
        reasoning: String,
    }
    let mut bufs: HashMap<String, Buf> = HashMap::new();
    let flush_interval = std::time::Duration::from_millis(30);

    fn flush_sid(app: &AppHandle, sid: &str, buf: &mut Buf) {
        if !buf.text.is_empty() {
            let text = std::mem::take(&mut buf.text);
            let _ = app.emit(
                "parzi://run-event",
                UiEvent::Text {
                    session: sid.to_string(),
                    text,
                },
            );
        }
        if !buf.reasoning.is_empty() {
            let text = std::mem::take(&mut buf.reasoning);
            let _ = app.emit(
                "parzi://run-event",
                UiEvent::Reasoning {
                    session: sid.to_string(),
                    text,
                },
            );
        }
    }

    // Fixed from when the buffer first fills: a timer re-armed per event
    // would never fire while tokens stream faster than the interval.
    let mut deadline: Option<tokio::time::Instant> = None;
    loop {
        let has_buf = bufs
            .values()
            .any(|b| !b.text.is_empty() || !b.reasoning.is_empty());
        if !has_buf {
            deadline = None;
        } else if deadline.is_none() {
            deadline = Some(tokio::time::Instant::now() + flush_interval);
        }
        let next = if let Some(at) = deadline {
            tokio::select! {
                rec = rx.recv() => rec,
                () = tokio::time::sleep_until(at) => {
                    for (sid, buf) in &mut bufs {
                        flush_sid(&app, sid, buf);
                    }
                    deadline = None;
                    continue;
                }
            }
        } else {
            rx.recv().await
        };
        match next {
            Ok((sid, RunEvent::Text(text))) => {
                let buf = bufs.entry(sid.clone()).or_default();
                buf.text.push_str(&text);
                if buf.text.len() >= 1024 {
                    flush_sid(&app, &sid, buf);
                }
            }
            Ok((sid, RunEvent::Reasoning { text })) => {
                let buf = bufs.entry(sid.clone()).or_default();
                buf.reasoning.push_str(&text);
                if buf.reasoning.len() >= 1024 {
                    flush_sid(&app, &sid, buf);
                }
            }
            Ok((sid, other)) => {
                if let Some(buf) = bufs.get_mut(&sid) {
                    flush_sid(&app, &sid, buf);
                }
                // A finished local run goes to the server's mirror.
                let ended = matches!(other, RunEvent::Done { .. } | RunEvent::Error(_));
                if let Some(ui) = ui_from_run(sid, other) {
                    let _ = app.emit("parzi://run-event", ui);
                }
                if ended {
                    remote::sync_soon(&app);
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("host bus lagged by {n} event(s)");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

const LOG_DAYS: u64 = 7;

fn init_log() {
    let Ok(dir) = parzi_core::paths::parzi_dir().map(|d| d.join("logs")) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let week = std::time::Duration::from_secs(LOG_DAYS * 24 * 60 * 60);
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > week);
        let ours = entry.file_name().to_string_lossy().starts_with("parzi-");
        if old && ours {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let name = format!("parzi-{}.log", chrono::Local::now().format("%Y-%m-%d"));
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(name))
    else {
        return;
    };
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(std::sync::Mutex::new(file))
        .init();
}

/// Before the app is built there is no dialog plugin, and a release build
/// has no console: a native box is the only way the user learns why.
fn alert(title: &str, text: &str) {
    #[cfg(target_os = "windows")]
    dwm::alert(title, text);
    #[cfg(not(target_os = "windows"))]
    eprintln!("{title}: {text}");
}

fn fatal(text: &str) -> ! {
    tracing::error!("{text}");
    alert("Parzi could not start", text);
    std::process::exit(1);
}

fn main() {
    init_log();
    // Mutual exclusion with `parzi serve`: both own ~/.parzi, so the desk
    // refuses to start while the headless lock is held. Checked before the
    // store opens: recovery would flip serve's live sessions to idle.
    if parzi_runtime::osserve::serve_locked() {
        tracing::warn!("parzi serve holds the lock; the desk is not starting");
        alert(
            "Parzi is already running headless",
            "parzi serve is running. The desk and serve must never own sessions at the same time. Stop serve first, then open Parzi.",
        );
        std::process::exit(2);
    }
    parzi_providers::process::on_spawn(job::adopt);
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("panic: {info}");
        eprintln!("{msg}");
        if let Ok(dir) = parzi_core::paths::parzi_dir().map(|d| d.join("logs")) {
            let name = chrono::Local::now()
                .format("parzi-%Y-%m-%d.log")
                .to_string();
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join(name))
            {
                use std::io::Write as _;
                let _ = writeln!(f, "{msg}");
            }
        }
    }));
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(3)
        .max_blocking_threads(8)
        .enable_all()
        .build()
        .unwrap_or_else(|e| fatal(&format!("The task runtime did not start: {e}")));
    tauri::async_runtime::set(rt.handle().clone());
    let (cfg, store, recovered) = boot().unwrap_or_else(|e| {
        let home = parzi_core::paths::parzi_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        fatal(&format!("Parzi could not open its data folder {home}: {e}"))
    });
    let orch = Arc::new(Orchestrator::new(cfg, store));
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let questions: PendingQuestions = Arc::new(Mutex::new(HashMap::new()));
    let desk = Arc::new(control::Desk::new());
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
                return;
            }
            let Some(cfg) = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
            else {
                app.exit(0);
                return;
            };
            match tauri::WebviewWindowBuilder::from_config(app, &cfg).and_then(|b| b.build()) {
                Ok(w) => {
                    #[cfg(target_os = "windows")]
                    dwm::round_window_corners(&w);
                    let _ = w.show();
                    let _ = w.set_focus();
                }
                Err(e) => {
                    tracing::error!("could not reopen the window: {e}");
                    app.exit(0);
                }
            }
        }))
        .setup(move |app| {
            // Here, not in main: plugins initialize first, so a second
            // launch has already handed off to the running desk and exited
            // instead of marking that desk's live runs idle.
            orch.recover().ok();
            let o = orch.clone();
            parzi_runtime::remote::sweep_askpass();
            if let Err(e) = parzi_core::device::ensure() {
                tracing::warn!("device id not saved: {e}");
            }
            app.manage(remote::RemoteState::default());
            remote::start_ticker(app.handle());
            app.manage(AppState {
                orch,
                pending,
                questions: questions.clone(),
                app: app.handle().clone(),
                desk: desk.clone(),
            });
            o.set_asker(Arc::new(GuiAsker {
                app: app.handle().clone(),
                pending: questions.clone(),
            }));
            let ctl_app = app.handle().clone();
            let ctl_orch = o.clone();
            let ctl_pending = app.state::<AppState>().pending.clone();
            let ctl_desk = desk.clone();
            tauri::async_runtime::spawn(async move {
                control::serve(ctl_app, ctl_orch, ctl_pending, ctl_desk).await;
            });
            if let Some(note) = recovered {
                let h = app.handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
                    let _ = h
                        .dialog()
                        .message(note)
                        .title("Parzi settings recovered")
                        .buttons(MessageDialogButtons::Ok)
                        .blocking_show();
                });
            }
            #[cfg(target_os = "windows")]
            if let Some(w) = app.get_webview_window("main") {
                dwm::round_window_corners(&w);
            }
            adblock::start();
            let bus_rx = o.subscribe();
            let bus_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                forward_host_bus(bus_app, bus_rx).await;
            });
            let board = o.clone();
            let board_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let all = settings::in_roster_order(board.refresh_providers(&[]).await);
                let _ = board_app.emit("parzi://providers", &all);
            });
            tauri::async_runtime::spawn(async move {
                match o.recover_queue().await {
                    Ok(n) if n > 0 => tracing::info!("re-enqueued {n} queued run(s) from last run"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!("queued runs not recovered: {e}"),
                }
                o.kick().await;
                o.pump_loop().await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            window_fullscreen,
            browser::browser_show,
            browser::browser_hide,
            browser::browser_close,
            browser::browser_navigate,
            browser::browser_nav,
            browser::browser_prepare,
            browser::browser_snapshot,
            search::search_suggest,
            adblock::adblock_state,
            adblock::adblock_enable,
            adblock::adblock_site,
            vault::vault_state,
            vault::vault_unlock,
            vault::vault_lock,
            vault::vault_logins,
            vault::vault_fill,
            brain::brain_dir,
            brain::brain_list,
            brain::brain_read,
            brain::brain_write,
            brain::brain_save_answer,
            brain::brain_delete,
            brain::brain_projects,
            brain::brain_project_upsert,
            brain::brain_map,
            brain::brain_context,
            brain::brain_pin,
            brain::brain_open,
            brain::brain_obsidian,
            sessions::list_threads,
            sessions::get_thread,
            sessions::send_message,
            sessions::kill_run,
            sessions::compact_thread,
            sessions::fork_thread,
            sessions::rename_thread,
            sessions::delete_thread,
            sessions::purge_sessions,
            sessions::approve_tool,
            sessions::answer_question,
            sessions::spawn_track,
            sessions::plan_get,
            files::pick_folder,
            files::list_files,
            files::git_branch,
            files::read_image_data_url,
            files::stage_image,
            files::open_confirmed_url,
            files::open_file_path,
            settings::get_config,
            settings::save_config,
            settings::toggle_favorite,
            settings::provider_statuses,
            settings::refresh_providers,
            settings::warm_agent,
            settings::run_doctor_quick,
            appearance::get_theme_css,
            appearance::get_theme,
            appearance::save_theme,
            appearance::reset_theme,
            appearance::background_url,
            appearance::list_pack_infos,
            appearance::save_pack,
            appearance::apply_pack,
            appearance::delete_pack,
            appearance::rename_pack,
            appearance::list_background_urls,
            appearance::set_background,
            appearance::delete_background,
            appearance::save_background_data,
            appearance::palette_from_background,
            control::desk_sync,
            onboard::onboard_scan,
            onboard::onboard_browser,
            onboard::onboard_import,
            onboard::agent_install,
            onboard::agent_login,
            remote::remote_info,
            remote::remote_setup,
            remote::remote_connect,
            remote::remote_providers,
            remote::remote_projects,
            remote::remote_project_folder,
            remote::remote_sync,
            remote::remote_agent,
            remote::remote_forget,
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| fatal(&format!("Parzi failed to start: {e}")))
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                control::clear_gui_file();
                if let Some(state) = app.try_state::<AppState>() {
                    let mcp = state.orch.mcp().clone();
                    let (tx, rx) = std::sync::mpsc::sync_channel(1);
                    tauri::async_runtime::spawn(async move {
                        mcp.shutdown().await;
                        let _ = tx.send(());
                    });
                    let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_faults_name_the_place_not_the_line() {
        let text = "version = 2\n[mcp.servers.s]\nenv = { KEY = \"sk-secret\" \n";
        let at = text.find("sk-secret").unwrap();
        assert_eq!(line_col(text, at), "line 3, column 16");
        assert_eq!(line_col(text, 0), "line 1, column 1");
        assert_eq!(line_col("ab", 99), "line 1, column 3");
    }
}
