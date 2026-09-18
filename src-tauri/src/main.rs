//! parzi-app: thin Tauri glue. No business logic — every command delegates
//! to parzi-core / parzi-runtime. Events stream to the UI, never block it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::store::{Event, SessionMeta, SessionStore};
use parzi_core::theme::Theme;
use parzi_providers::ProviderStatus;
use parzi_runtime::handler::RunEvent;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};
use parzi_runtime::Orchestrator;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tokio::sync::{broadcast, oneshot, Mutex};

/// The crate's only unsafe code lives here, and nowhere else.
#[cfg(target_os = "windows")]
mod dwm;

/// Hub IPC: workspaces, projects, the two wizards (PLAN.md §1, §2).
mod hub_cmds;

struct AppState {
    orch: Arc<Orchestrator>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Approval>>>>,
    app: AppHandle,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum UiEvent {
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
    Usage {
        session: String,
        tokens_in: u64,
        tokens_out: u64,
        cost_usd: f64,
    },
    Context {
        session: String,
        used: u64,
        limit: u64,
    },
    Approval {
        key: String,
        call: ToolCallView,
    },
    SubsessionCreated {
        parent_id: String,
        subsession: SessionMeta,
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
struct ToolCallView {
    id: String,
    name: String,
    args: serde_json::Value,
    lane: String,
}

/// GUI approver: emits an approval event, waits for `approve_tool` (120s cap).
struct GuiApprover {
    app: AppHandle,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Approval>>>>,
}

#[async_trait::async_trait]
impl Approver for GuiApprover {
    async fn approve(&self, call: &ToolCallInfo) -> Approval {
        let key = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(key.clone(), tx);
        let view = ToolCallView {
            id: call.id.clone(),
            name: call.name.clone(),
            args: call.args.clone(),
            lane: call.lane.clone(),
        };
        let _ = self.app.emit(
            "parzi://run-event",
            UiEvent::Approval {
                key: key.clone(),
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

fn boot() -> anyhow::Result<(ParziConfig, SessionStore)> {
    parzi_core::paths::ensure_dirs()?;
    Ok((ParziConfig::load()?, SessionStore::open()?))
}

#[tauri::command]
async fn app_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[tauri::command]
async fn toggle_favorite(spec: String) -> Result<Vec<String>, String> {
    let mut cfg = ParziConfig::load().map_err(|e| e.to_string())?;
    if cfg.favorite_models.iter().any(|f| f == &spec) {
        cfg.favorite_models.retain(|f| f != &spec);
    } else {
        cfg.favorite_models.push(spec);
        cfg.favorite_models.truncate(9);
    }
    cfg.save().map_err(|e| e.to_string())?;
    Ok(cfg.favorite_models)
}

#[tauri::command]
async fn list_threads(state: State<'_, AppState>) -> Result<Vec<SessionMeta>, String> {
    state.orch.store().list().map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_thread(
    state: State<'_, AppState>,
    id: String,
) -> Result<(SessionMeta, Vec<Event>, String), String> {
    let meta = state.orch.store().get(&id).map_err(|e| e.to_string())?;
    let events = state.orch.store().events(&id).map_err(|e| e.to_string())?;
    let md = std::fs::read_to_string(
        parzi_core::paths::sessions_dir()
            .map_err(|e| e.to_string())?
            .join(&id)
            .join("session.md"),
    )
    .unwrap_or_default();
    Ok((meta, events, md))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn send_message(
    state: State<'_, AppState>,
    session_id: Option<String>,
    project: String,
    lane: String,
    model: String,
    prompt: String,
    cwd: String,
    effort: Option<String>,
    attachments: Option<Vec<String>>,
    parent_id: Option<String>,
    mode: Option<String>,
) -> Result<String, String> {
    let effort = parzi_runtime::orchestrator::normalize_effort(effort.as_deref().unwrap_or("med"));
    // Cwd: explicit > lane root > empty. @-files resolve against it.
    let cwd = if cwd.is_empty() {
        parzi_core::lanes::lane_root(&project, &lane).unwrap_or_default()
    } else {
        cwd
    };
    let attached = read_attachments(&cwd, &attachments.unwrap_or_default());
    let approver: Arc<dyn Approver> = Arc::new(GuiApprover {
        app: state.app.clone(),
        pending: state.pending.clone(),
    });
    let app = state.app.clone();
    let (sid, rx) = match session_id {
        Some(id) => {
            // Per-message model switch: the thread follows the picker.
            let rx = state
                .orch
                .send_to(
                    &id,
                    &prompt,
                    Some(approver),
                    &cwd,
                    &effort,
                    attached,
                    Some(model.clone()),
                    mode.clone(),
                )
                .await
                .map_err(|e| e.to_string())?;
            (id, rx)
        }
        None => {
            // UI-spawned subsession: nest under the parent, inheriting its
            // project/lane/cwd (and model when the picker is untouched).
            if let Some(pid) = parent_id.filter(|p| !p.trim().is_empty()) {
                let pmeta = state.orch.store().get(&pid).map_err(|e| e.to_string())?;
                let title: String = prompt
                    .lines()
                    .next()
                    .unwrap_or("subsession")
                    .chars()
                    .take(80)
                    .collect();
                let lane = if lane.is_empty() {
                    pmeta.lane.clone()
                } else {
                    lane
                };
                let model = if model.is_empty() {
                    pmeta.model.clone()
                } else {
                    model
                };
                let cwd = if cwd.is_empty() {
                    pmeta.cwd.clone()
                } else {
                    cwd
                };
                let meta = state
                    .orch
                    .store()
                    .create_with_parent(&title, &pmeta.project, &lane, &model, Some(&pid))
                    .map_err(|e| e.to_string())?;
                if !cwd.is_empty() {
                    let _ = state.orch.store().set_cwd(&meta.id, &cwd);
                }
                let rx = state
                    .orch
                    .send_to(
                        &meta.id,
                        &prompt,
                        Some(approver),
                        &cwd,
                        &effort,
                        attached,
                        Some(model.clone()),
                        mode.clone(),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                let _ = app.emit(
                    "parzi://run-event",
                    UiEvent::SubsessionCreated {
                        parent_id: pid,
                        subsession: meta.clone(),
                    },
                );
                (meta.id, rx)
            } else {
                let (meta, rx) = state
                    .orch
                    .spawn(
                        &project,
                        &lane,
                        &model,
                        &prompt,
                        Some(approver),
                        &cwd,
                        &effort,
                        attached,
                        mode.clone(),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                (meta.id, rx)
            }
        }
    };
    // Events go out on the host bus. setup() subscribed once (R-5), so a
    // dispatched lane whose receiver is dropped still reaches the deck, and
    // forwarding here as well would double every token of an interactive run.
    drop(rx);
    Ok(sid)
}

fn ui_from_run(session: String, ev: RunEvent) -> Option<UiEvent> {
    Some(match ev {
        RunEvent::Text(_) | RunEvent::Reasoning { .. } => return None,
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
        RunEvent::Usage {
            tokens_in,
            tokens_out,
            cost_usd,
        } => UiEvent::Usage {
            session,
            tokens_in,
            tokens_out,
            cost_usd,
        },
        RunEvent::Context { used, limit } => UiEvent::Context {
            session,
            used,
            limit,
        },
        // GuiApprover emits its own UiEvent::Approval; a lease transfer
        // that also notifies ApprovalRequest must not draw a second card.
        RunEvent::ApprovalRequest { .. } => return None,
        RunEvent::Done { turns } => UiEvent::Done { session, turns },
        RunEvent::Error(error) => UiEvent::Error { session, error },
    })
}

/// R-5: one subscriber for every run — queued, harness-spawned, and
/// dispatched lanes whose per-run receiver was dropped.
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

    loop {
        let has_buf = bufs
            .values()
            .any(|b| !b.text.is_empty() || !b.reasoning.is_empty());
        let next = if has_buf {
            tokio::select! {
                rec = rx.recv() => rec,
                _ = tokio::time::sleep(flush_interval) => {
                    for (sid, buf) in bufs.iter_mut() {
                        flush_sid(&app, sid, buf);
                    }
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
                if let Some(ui) = ui_from_run(sid, other) {
                    let _ = app.emit("parzi://run-event", ui);
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("host bus lagged by {n} event(s)");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

#[tauri::command]
async fn kill_run(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.orch.kill(&id).await.map_err(|e| e.to_string())
}

/// Summarize a thread into a checkpoint; later messages read from there.
#[tauri::command]
async fn compact_thread(
    state: State<'_, AppState>,
    id: String,
    focus: Option<String>,
) -> Result<String, String> {
    state
        .orch
        .compact(&id, focus.as_deref().unwrap_or(""))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn fork_thread(
    state: State<'_, AppState>,
    id: String,
    at: Option<usize>,
) -> Result<SessionMeta, String> {
    state.orch.fork(&id, at).await.map_err(|e| e.to_string())
}

/// Create a child subsession under `parent_id` (UI-initiated teamwork).
/// Inherits project/lane/cwd and — unless overridden — the parent's model.
/// With `prompt`, a headless run starts on the child; without, it waits empty
/// for its first message. Emits `SubsessionCreated` so the sidebar expands.
#[tauri::command]
async fn create_subsession(
    state: State<'_, AppState>,
    parent_id: String,
    title: String,
    prompt: Option<String>,
    model: Option<String>,
) -> Result<SessionMeta, String> {
    let meta = state
        .orch
        .create_subsession(&parent_id, &title, prompt.as_deref(), model.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    let _ = state.app.emit(
        "parzi://run-event",
        UiEvent::SubsessionCreated {
            parent_id,
            subsession: meta.clone(),
        },
    );
    Ok(meta)
}

/// Move a session under a new parent (or detach to top-level with `None`).
#[tauri::command]
async fn reparent_thread(
    state: State<'_, AppState>,
    id: String,
    parent_id: Option<String>,
) -> Result<(), String> {
    state
        .orch
        .store()
        .set_parent(&id, parent_id.as_deref())
        .map_err(|e| e.to_string())
}

/// Read @-attached files (cap 8, 12k chars each + images as base64),
/// resolved against cwd. One implementation lives in core; the shell,
/// the CLI and the handler all share it.
fn read_attachments(cwd: &str, paths: &[String]) -> Vec<parzi_core::context::AttachedFile> {
    let base = if cwd.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        std::path::PathBuf::from(cwd)
    };
    parzi_core::context::read_attachments(&base, paths)
}

/// Image bytes for UI previews (composer thumbs, thread bubbles) as a data
/// URL. H-2: confined exactly like `read_text_file`; 8 MiB cap; images only.
const READ_IMAGE_MAX: u64 = 8 * 1024 * 1024;

#[tauri::command]
async fn read_image_data_url(path: String, cwd: String) -> Result<String, String> {
    let abs = if std::path::Path::new(&path).is_absolute() {
        path
    } else if cwd.trim().is_empty() {
        return Err("no workspace folder for a relative image path".into());
    } else {
        format!(
            "{}/{}",
            cwd.trim().trim_end_matches(['/', '\\']),
            path.trim().trim_start_matches(['/', '\\'])
        )
    };
    let p = confined_path(&abs)?;
    let meta = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", p.display()));
    }
    if meta.len() > READ_IMAGE_MAX {
        return Err(format!(
            "{} is {} KiB — over the {} KiB image limit",
            p.display(),
            meta.len() / 1024,
            READ_IMAGE_MAX / 1024
        ));
    }
    let img = parzi_core::context::encode_image_file(&p)
        .ok_or_else(|| format!("{} is not a supported image", p.display()))?;
    Ok(img.data_url())
}

/// Staged paste/drop images for the composer: pasted pixels have no disk
/// path, so the shell stores them under `<parzi>/attachments/<uuid>.<ext>`
/// (8 MiB cap, image extensions only) and returns the absolute path, which
/// attaches exactly like a picker-chosen file. Oldest staged files past 32
/// are pruned on each stage so the folder cannot grow without bound.
#[tauri::command]
async fn stage_image(name: String, base64_data: String) -> Result<String, String> {
    use base64::Engine as _;
    const STAGE_MAX: usize = 8 * 1024 * 1024;
    const KEEP: usize = 32;
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    if !matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
    ) {
        return Err(format!("{name} is not a supported image"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data.trim())
        .map_err(|_| "bad image data".to_string())?;
    if bytes.len() > STAGE_MAX {
        return Err("image is over the 8 MiB stage limit".into());
    }
    // Refuse non-image payloads wearing an image extension.
    if parzi_core::context::encode_image(&format!("x.{ext}"), &bytes).is_none() {
        return Err(format!("{name} is not a supported image"));
    }
    let dir = parzi_core::paths::parzi_dir()
        .map_err(|e| e.to_string())?
        .join("attachments");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{}.{ext}", uuid::Uuid::new_v4()));
    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    // Prune oldest past KEEP (best-effort, never fails the stage).
    if let Ok(mut files) = std::fs::read_dir(&dir).map(|rd| {
        rd.flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| {
                e.metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|t| (t, e.path()))
            })
            .collect::<Vec<_>>()
    }) {
        files.sort_by_key(|(t, _)| *t);
        for (_, p) in files.iter().take(files.len().saturating_sub(KEEP)) {
            let _ = std::fs::remove_file(p);
        }
    }
    Ok(dest.to_string_lossy().to_string())
}

#[tauri::command]
async fn toggle_pin(state: State<'_, AppState>, id: String, pinned: bool) -> Result<(), String> {
    state
        .orch
        .store()
        .set_pinned(&id, pinned)
        .map_err(|e| e.to_string())
}

/// Built-in (non-connector) tool catalog for the Agent-tools settings page:
/// groups and blurbs come from the runtime so docs and UI never drift.
#[derive(Debug, Clone, Serialize)]
struct BuiltinToolView {
    name: String,
    group: String,
    blurb: String,
}

#[tauri::command]
async fn list_builtin_tools() -> Result<Vec<BuiltinToolView>, String> {
    Ok(parzi_runtime::tools::builtin_tools()
        .into_iter()
        .map(|t| BuiltinToolView {
            name: t.name.to_string(),
            group: t.group.to_string(),
            blurb: t.blurb.to_string(),
        })
        .collect())
}

#[tauri::command]
async fn approve_tool(state: State<'_, AppState>, key: String, allow: bool) -> Result<(), String> {
    let tx = state.pending.lock().await.remove(&key);
    match tx {
        Some(tx) => {
            let _ = tx.send(if allow {
                Approval::Allow
            } else {
                Approval::Deny
            });
            Ok(())
        }
        None => Err("approval expired or unknown".into()),
    }
}

/// The board in roster order, so picker, settings and sidebar agree. A
/// provider stays where the person expects it whether or not it is ready.
fn in_roster_order(mut all: Vec<ProviderStatus>) -> Vec<ProviderStatus> {
    all.sort_by_key(|s| {
        parzi_providers::PROVIDERS
            .iter()
            .position(|p| *p == s.provider)
            .unwrap_or(usize::MAX)
    });
    all
}

/// Where each agent stood when its own program was last asked. Instant:
/// the board is kept on disk between launches.
#[tauri::command]
async fn provider_statuses(state: State<'_, AppState>) -> Result<Vec<ProviderStatus>, String> {
    Ok(in_roster_order(state.orch.provider_statuses()))
}

/// Ask the agents' own programs again (`ids` empty = all). Spends no quota;
/// the slowest vendor sets the pace, so the UI calls this in the background.
#[tauri::command]
async fn refresh_providers(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Option<Vec<String>>,
) -> Result<Vec<ProviderStatus>, String> {
    let all = in_roster_order(state.orch.refresh_providers(&ids.unwrap_or_default()).await);
    let _ = app.emit("parzi://providers", &all);
    Ok(all)
}

#[tauri::command]
async fn reset_theme() -> Result<String, String> {
    let theme = Theme::default();
    theme.save().map_err(|e| e.to_string())?;
    get_theme_css().await
}

#[tauri::command]
async fn purge_sessions(state: State<'_, AppState>) -> Result<usize, String> {
    state
        .orch
        .store()
        .purge_finished()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_theme_css() -> Result<String, String> {
    let theme = Theme::load().map_err(|e| e.to_string())?;
    Ok(theme_css(&theme))
}

#[tauri::command]
async fn get_user_css() -> Result<String, String> {
    parzi_core::theme::read_user_css().map_err(|e| e.to_string())
}

/// Save (or clear, when empty) user.css and return the full stylesheet.
#[tauri::command]
async fn save_user_css(css: String) -> Result<String, String> {
    parzi_core::theme::write_user_css(&css).map_err(|e| e.to_string())?;
    get_theme_css().await
}

#[tauri::command]
async fn save_theme(theme: Theme) -> Result<(), String> {
    theme.save().map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_config() -> Result<ParziConfig, String> {
    ParziConfig::load().map_err(|e| e.to_string())
}
#[tauri::command]
async fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    cfg: ParziConfig,
) -> Result<(), String> {
    if cfg.version != parzi_core::config::CONFIG_VERSION {
        return Err("config version mismatch — reload settings".into());
    }
    // H-2: a changed MCP command/args, or a changed agent program, runs
    // with user privileges on every agent turn from the next launch on.
    // New or edited ones get a native confirm; removals (fewer
    // capabilities) save silently.
    let old = ParziConfig::load().unwrap_or_default();
    let mut changed: Vec<String> = mcp_command_changes(&old, &cfg)
        .into_iter()
        .map(|s| format!("connector {s}"))
        .collect();
    changed.extend(binary_changes(&old, &cfg));
    if !changed.is_empty() {
        let msg = format!(
            "Changed: {}. A malicious program runs with your user account on every agent turn. Save anyway?",
            changed.join(", ")
        );
        let allow = tokio::task::spawn_blocking(move || {
            app.dialog()
                .message(msg)
                .title("Parzi — confirm connector")
                .buttons(MessageDialogButtons::OkCancel)
                .blocking_show()
        })
        .await
        .map_err(|e| e.to_string())?;
        if !allow {
            return Err("change cancelled".into());
        }
    }
    cfg.save().map_err(|e| e.to_string())?;
    // Hot-apply: next agent launch uses the new policy/MCP servers, no restart.
    state.orch.apply_config(cfg).await;
    Ok(())
}

/// Agents whose program path was set or edited between two configs, as
/// "claude → C:\…\claude.exe". Clearing a path (back to PATH) is silent.
/// Pure and unit-tested.
fn binary_changes(old: &ParziConfig, new: &ParziConfig) -> Vec<String> {
    let mut out: Vec<String> = new
        .providers
        .iter()
        .filter_map(|(id, entry)| {
            let path = Some(entry.binary.trim()).filter(|p| !p.is_empty())?;
            let before = old.providers.get(id).map(|e| e.binary.trim());
            (before != Some(path)).then(|| format!("{id} → {path}"))
        })
        .collect();
    out.sort();
    out
}

/// Names of MCP servers whose `command`/`args` were added or edited between
/// two configs. Pure and unit-tested.
fn mcp_command_changes(old: &ParziConfig, new: &ParziConfig) -> Vec<String> {
    let mut out = vec![];
    for (name, srv) in &new.mcp.servers {
        match old.mcp.servers.get(name) {
            Some(prev) if prev.command == srv.command && prev.args == srv.args => {}
            _ => out.push(name.clone()),
        }
    }
    out.sort();
    out
}

/// One connector's tool inventory for the Settings tool browser.
/// Best-effort: unreachable servers return `ok: false` with the error.
#[derive(Debug, Clone, serde::Serialize)]
struct McpToolView {
    name: String,
    qualified: String,
    description: String,
    exposed: bool,
    mode: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct McpServerTools {
    server: String,
    ok: bool,
    error: Option<String>,
    tools: Vec<McpToolView>,
}

#[tauri::command]
async fn list_mcp_tools(
    state: State<'_, AppState>,
    server: String,
) -> Result<McpServerTools, String> {
    let name = server.trim().to_string();
    if name.is_empty() {
        return Err("empty server name".into());
    }
    let mgr = state.orch.mcp().clone();
    // Raw list (even unexposed tools) so the UI can render exposure toggles.
    match tokio::time::timeout(std::time::Duration::from_secs(20), mgr.list_tools(&name)).await {
        Ok(Ok(tools)) => {
            let views = tools
                .into_iter()
                .map(|t| McpToolView {
                    qualified: format!("{}.{}", name, t.name),
                    exposed: mgr.is_tool_exposed(&name, &t.name),
                    mode: mgr.tool_mode(&name, &t.name),
                    name: t.name,
                    description: t.description,
                })
                .collect();
            Ok(McpServerTools {
                server: name,
                ok: true,
                error: None,
                tools: views,
            })
        }
        Ok(Err(e)) => Ok(McpServerTools {
            server: name,
            ok: false,
            error: Some(e.to_string()),
            tools: vec![],
        }),
        Err(_) => Ok(McpServerTools {
            server: name,
            ok: false,
            error: Some("probe timed out".into()),
            tools: vec![],
        }),
    }
}

/// Tool inventories for every configured server (parallel, 20s cap each).
/// Powers the Connectors page: one roundtrip renders all tool lists.
#[tauri::command]
async fn list_all_mcp_tools(state: State<'_, AppState>) -> Result<Vec<McpServerTools>, String> {
    let servers = state.orch.mcp().server_names();
    let mut set = tokio::task::JoinSet::new();
    for name in servers {
        let mgr = state.orch.mcp().clone();
        set.spawn(async move {
            match tokio::time::timeout(std::time::Duration::from_secs(20), mgr.list_tools(&name))
                .await
            {
                Ok(Ok(tools)) => {
                    let views = tools
                        .into_iter()
                        .map(|t| McpToolView {
                            qualified: format!("{}.{}", name, t.name),
                            exposed: mgr.is_tool_exposed(&name, &t.name),
                            mode: mgr.tool_mode(&name, &t.name),
                            name: t.name,
                            description: t.description,
                        })
                        .collect();
                    McpServerTools {
                        server: name,
                        ok: true,
                        error: None,
                        tools: views,
                    }
                }
                Ok(Err(e)) => McpServerTools {
                    server: name,
                    ok: false,
                    error: Some(e.to_string()),
                    tools: vec![],
                },
                Err(_) => McpServerTools {
                    server: name,
                    ok: false,
                    error: Some("probe timed out".into()),
                    tools: vec![],
                },
            }
        });
    }
    let mut out = vec![];
    while let Some(row) = set.join_next().await {
        if let Ok(row) = row {
            out.push(row);
        }
    }
    out.sort_by(|a, b| a.server.cmp(&b.server));
    Ok(out)
}

#[tauri::command]
async fn get_theme() -> Result<Theme, String> {
    Theme::load().map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_packs() -> Result<Vec<String>, String> {
    parzi_core::theme::list_packs().map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_pack(name: String) -> Result<(), String> {
    parzi_core::theme::save_pack(&name).map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_pack_infos() -> Result<Vec<parzi_core::theme::PackInfo>, String> {
    parzi_core::theme::list_pack_infos().map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_pack(name: String) -> Result<(), String> {
    parzi_core::theme::delete_pack(&name).map_err(|e| e.to_string())
}

#[tauri::command]
async fn rename_pack(old: String, new: String) -> Result<(), String> {
    parzi_core::theme::rename_pack(&old, &new).map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_background(name: String) -> Result<String, String> {
    let theme = parzi_core::theme::delete_background(&name).map_err(|e| e.to_string())?;
    Ok(theme_css(&theme))
}

#[tauri::command]
async fn apply_pack(name: String) -> Result<String, String> {
    let theme = parzi_core::theme::apply_pack(&name).map_err(|e| e.to_string())?;
    Ok(theme_css(&theme))
}

/// AUDIT C-7, shared by every command that turns a wallpaper name into a
/// path: `abs` is accepted only when it stays inside `~/.parzi/backgrounds`
/// and names a regular file. `starts_with` compares components and does not
/// resolve `..`, so a traversal is refused outright instead of normalized
/// away; `symlink_metadata` refuses a link pointing out of the folder.
fn checked_background(abs: std::path::PathBuf) -> Option<std::path::PathBuf> {
    let bgs = parzi_core::paths::backgrounds_dir().ok()?;
    if abs
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
        || !abs.starts_with(&bgs)
    {
        return None;
    }
    std::fs::symlink_metadata(&abs)
        .is_ok_and(|m| m.is_file())
        .then_some(abs)
}

#[tauri::command]
async fn background_url(app: AppHandle) -> Result<String, String> {
    // Return a plain absolute path; the UI turns it into an asset URL with
    // convertFileSrc (asset protocol scope is configured in tauri.conf.json).
    let theme = Theme::load().map_err(|e| e.to_string())?;
    if theme.background.image.is_empty() {
        return Ok(String::new());
    }
    let abs = parzi_core::paths::parzi_dir()
        .map_err(|e| e.to_string())?
        .join(&theme.background.image);
    // A hand-edited theme.toml must not point the webview outside the
    // wallpaper folder. A refused, missing or non-regular file means a solid
    // stage, not an error.
    let Some(abs) = checked_background(abs) else {
        return Ok(String::new());
    };
    // E2: hand the UI a texture the size of the screen with the blur already
    // baked in — the CSS filter that used to do it went with the compositor
    // diet. A render failure falls back to the source: never a black stage.
    let (sw, sh, scale) = app
        .available_monitors()
        .ok()
        .and_then(|ms| {
            ms.into_iter()
                .max_by_key(|m| u64::from(m.size().width) * u64::from(m.size().height))
        })
        .map(|m| (m.size().width, m.size().height, m.scale_factor()))
        .unwrap_or((1920, 1080, 1.0));
    let cache = parzi_core::paths::cache_dir().map_err(|e| e.to_string())?;
    let (src, blur) = (abs.clone(), theme.background.blur);
    let rendered = tauri::async_runtime::spawn_blocking(move || {
        parzi_core::wallpaper::prepare(&src, &cache, blur, sw, sh, scale)
    })
    .await
    .map_err(|e| e.to_string())?;
    let out = match rendered {
        Ok(p) => p,
        Err(e) => {
            eprintln!("wallpaper render failed, serving the source: {e}");
            abs
        }
    };
    // The cache lives next to the wallpapers, not inside them, so grant the
    // asset protocol this one file rather than widening the configured glob.
    let _ = app.asset_protocol_scope().allow_file(&out);
    Ok(out.to_string_lossy().to_string())
}

#[tauri::command]
async fn run_doctor(
    state: State<'_, AppState>,
) -> Result<Vec<parzi_runtime::doctor::Check>, String> {
    let cfg = state.orch.config();
    Ok(parzi_runtime::doctor::Doctor::new(cfg).run().await)
}

/// Fast health subset (no MCP probes). System tab renders this immediately.
#[tauri::command]
async fn run_doctor_quick(
    state: State<'_, AppState>,
) -> Result<Vec<parzi_runtime::doctor::Check>, String> {
    let cfg = state.orch.config();
    Ok(parzi_runtime::doctor::Doctor::new(cfg).run_quick().await)
}

/// MCP server probes only (each can take up to 15s). Loaded lazily.
#[tauri::command]
async fn run_doctor_mcp(
    state: State<'_, AppState>,
) -> Result<Vec<parzi_runtime::doctor::Check>, String> {
    let cfg = state.orch.config();
    Ok(parzi_runtime::doctor::Doctor::new(cfg).run_mcp_only().await)
}

#[derive(Debug, Clone, serde::Serialize)]
struct BackgroundFile {
    name: String,
    url: String,
}

/// All wallpapers + absolute paths in one roundtrip (replaces N sequential
/// `background_file` calls from settings).
#[tauri::command]
async fn list_background_urls() -> Result<Vec<BackgroundFile>, String> {
    let names = parzi_core::theme::list_backgrounds().map_err(|e| e.to_string())?;
    let dir = parzi_core::paths::backgrounds_dir().map_err(|e| e.to_string())?;
    Ok(names
        .into_iter()
        .map(|name| {
            let url = dir.join(&name).to_string_lossy().to_string();
            BackgroundFile { name, url }
        })
        .collect())
}

#[tauri::command]
async fn list_backgrounds() -> Result<Vec<String>, String> {
    parzi_core::theme::list_backgrounds().map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_background(name: String) -> Result<String, String> {
    let theme = parzi_core::theme::set_background(&name).map_err(|e| e.to_string())?;
    Ok(theme_css(&theme))
}

#[tauri::command]
async fn upload_background(src: String) -> Result<String, String> {
    parzi_core::theme::upload_background(&src).map_err(|e| e.to_string())
}

fn decode_b64(input: &str) -> Option<Vec<u8>> {
    let clean = input.split(',').next_back()?.trim();
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for &b in clean.as_bytes() {
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        } as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

#[tauri::command]
async fn save_background_data(name: String, base64_data: String) -> Result<String, String> {
    let bytes = decode_b64(&base64_data).ok_or("invalid base64 image data")?;
    // Decoding an oversized picture to shrink it is seconds of CPU; keep it
    // off the async runtime.
    let saved = tauri::async_runtime::spawn_blocking(move || {
        parzi_core::theme::import_background(&name, &bytes)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    // `set_background` still checks the stored file. Its refusal is this
    // command's answer — swallowing it left the picture on disk and reported
    // success while the wallpaper never changed.
    if let Err(e) = parzi_core::theme::set_background(&saved) {
        if let Ok(dir) = parzi_core::paths::backgrounds_dir() {
            let _ = std::fs::remove_file(dir.join(&saved));
        }
        return Err(e.to_string());
    }
    // The stored name, which is what the gallery lists and selects.
    Ok(saved)
}

#[tauri::command]
async fn background_file(name: String) -> Result<String, String> {
    let p = parzi_core::paths::backgrounds_dir()
        .map_err(|e| e.to_string())?
        .join(&name);
    if !p.exists() {
        return Err("background not found".into());
    }
    Ok(p.to_string_lossy().to_string())
}

/// Derive UI colors from wallpaper art. `name`: a saved background, or omit
/// for the currently active wallpaper. Errors when no wallpaper is set.
#[tauri::command]
async fn palette_from_background(
    name: Option<String>,
) -> Result<parzi_core::theme::Palette, String> {
    let path = match name {
        Some(n) if !n.is_empty() => parzi_core::paths::backgrounds_dir()
            .map_err(|e| e.to_string())?
            .join(n),
        _ => {
            let theme = Theme::load().map_err(|e| e.to_string())?;
            if theme.background.image.is_empty() {
                return Err("no wallpaper set".into());
            }
            parzi_core::paths::parzi_dir()
                .map_err(|e| e.to_string())?
                .join(&theme.background.image)
        }
    };
    // Same guard as `background_url` (C-7): the decoder must not be pointed
    // outside the wallpaper folder. Here a refusal is an error, because the
    // caller asked for a palette of a named picture.
    let path = checked_background(path).ok_or("bad background name")?;
    // E9: a full JPEG decode must not sit on a tokio worker.
    tauri::async_runtime::spawn_blocking(move || {
        parzi_core::theme::extract_palette(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Generated variables followed by user.css (which loads last and wins).
fn theme_css(theme: &Theme) -> String {
    let mut css = theme.to_css_vars();
    if let Ok(user) = parzi_core::theme::read_user_css() {
        if !user.trim().is_empty() {
            css.push_str("\n/* user.css */\n");
            css.push_str(&user);
        }
    }
    css
}

#[tauri::command]
async fn list_plugins() -> Result<Vec<PluginView>, String> {
    Ok(parzi_runtime::plugins::scan()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|p| {
            let commands = if p.manifest.kind == "commands" {
                parzi_runtime::plugins::commands_for(&p.manifest.name)
                    .map(|c| c.len())
                    .unwrap_or(0)
            } else {
                0
            };
            PluginView {
                name: p.manifest.name,
                version: p.manifest.version,
                kind: p.manifest.kind,
                enabled: p.enabled,
                commands,
            }
        })
        .collect())
}

#[derive(Debug, Clone, Serialize)]
struct PluginView {
    name: String,
    version: String,
    kind: String,
    enabled: bool,
    /// Slash-command count for `commands` packs (0 otherwise).
    commands: usize,
}

/// Settings → Skills: paste a commands.toml block or a SKILL.md file.
#[tauri::command]
async fn install_pasted_skill(
    pack_name: String,
    text: String,
) -> Result<parzi_runtime::plugins::InstalledSkill, String> {
    parzi_runtime::plugins::install_pasted_skill(&pack_name, &text).map_err(|e| e.to_string())
}

/// Settings → Skills: clone a skill library and install every skill inside.
/// Blocking subprocess — off the async worker thread.
#[tauri::command]
async fn install_skill_from_git(
    url: String,
) -> Result<parzi_runtime::plugins::SkillInstallReport, String> {
    tokio::task::spawn_blocking(move || {
        parzi_runtime::plugins::install_skill_from_git(&url).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Settings → Skills: remove a skill pack entirely.
#[tauri::command]
async fn delete_skill(name: String) -> Result<(), String> {
    parzi_runtime::plugins::delete_skill(name.trim()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn rename_thread(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<(), String> {
    state
        .orch
        .store()
        .set_title(&id, &title)
        .map_err(|e| e.to_string())
}

/// Delete a thread plus its subsession subtree. Live runs in the subtree
/// are killed first so nothing keeps writing to a removed dir.
#[tauri::command]
async fn delete_thread(state: State<'_, AppState>, id: String) -> Result<usize, String> {
    let all = state.orch.store().list().map_err(|e| e.to_string())?;
    let by_id: std::collections::HashMap<&str, &SessionMeta> =
        all.iter().map(|m| (m.id.as_str(), m)).collect();
    // The thread itself plus any session whose ancestor chain contains it.
    let mut doomed = vec![id.clone()];
    for m in &all {
        let mut p = m.parent_id.as_deref();
        let mut guard = 0;
        while let Some(pid) = p {
            if pid == id {
                doomed.push(m.id.clone());
                break;
            }
            if guard > 50 {
                break;
            }
            guard += 1;
            p = by_id.get(pid).and_then(|g| g.parent_id.as_deref());
        }
    }
    for sid in &doomed {
        let _ = state.orch.kill(sid).await;
    }
    state
        .orch
        .store()
        .delete_thread(&id)
        .map_err(|e| e.to_string())
}

/// Delete a workspace: its sessions (and subtrees) plus its project dir.
/// `default` is refused — it is the inbox, not a folder on disk.
#[tauri::command]
async fn delete_project(state: State<'_, AppState>, name: String) -> Result<usize, String> {
    let target = name.trim().to_string();
    if target.is_empty() || target == "default" {
        return Err("the default workspace can't be deleted".into());
    }
    // Stop live runs in this workspace before removing their dirs.
    let ids: Vec<String> = state
        .orch
        .store()
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|m| m.project == target)
        .map(|m| m.id)
        .collect();
    for id in &ids {
        let _ = state.orch.kill(id).await;
    }
    let n = state
        .orch
        .store()
        .delete_project_threads(&target)
        .map_err(|e| e.to_string())?;
    parzi_core::lanes::delete_project(&target).map_err(|e| e.to_string())?;
    Ok(n)
}

#[tauri::command]
async fn list_projects() -> Result<Vec<parzi_core::lanes::ProjectView>, String> {
    parzi_core::lanes::project_views().map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_project_roster(project: String) -> Result<parzi_core::lanes::ProjectRoster, String> {
    parzi_core::lanes::get_project_roster(&project).map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_project_roster(
    project: String,
    roster: parzi_core::lanes::ProjectRoster,
) -> Result<(), String> {
    parzi_core::lanes::save_project_roster(&project, &roster).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_project_plan(project: String) -> Result<String, String> {
    parzi_core::plan::read_plan(&project).map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_project_plan(project: String, content: String) -> Result<(), String> {
    parzi_core::plan::write_plan(&project, &content).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_project_knowledge(project: String) -> Result<String, String> {
    Ok(parzi_core::lanes::read_knowledge(&project).unwrap_or_default())
}

#[tauri::command]
async fn save_project_knowledge(project: String, content: String) -> Result<(), String> {
    let clean = parzi_core::lanes::safe_name(&project).map_err(|e| e.to_string())?;
    let path = parzi_core::paths::project_knowledge_path(&clean).map_err(|e| e.to_string())?;
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    parzi_core::atomic_write(&path, content.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_worktree_diff(
    project: String,
    lane: String,
    session_id: String,
) -> Result<String, String> {
    let clean_lane = if lane.trim().is_empty() {
        "default"
    } else {
        lane.trim()
    };
    let wt_path = parzi_core::paths::worktrees_dir()
        .map_err(|e| e.to_string())?
        .join(project)
        .join(clean_lane)
        .join(session_id);
    if !wt_path.exists() {
        return Ok(String::new());
    }
    parzi_runtime::git_worktree::worktree_diff(&wt_path).map_err(|e| e.to_string())
}

#[tauri::command]
async fn apply_worktree(
    project: String,
    lane: String,
    session_id: String,
) -> Result<String, String> {
    let clean_lane = if lane.trim().is_empty() {
        "default"
    } else {
        lane.trim()
    };
    let root = parzi_core::lanes::lane_root(&project, clean_lane)
        .ok_or_else(|| "project root not configured".to_string())?;
    parzi_runtime::git_worktree::apply_worktree(&root, clean_lane, &session_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_checkpoints(
    repo: String,
    session_id: String,
) -> Result<Vec<parzi_runtime::git_checkpoints::CheckpointView>, String> {
    parzi_runtime::git_checkpoints::list_checkpoints(&repo, &session_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn restore_checkpoint(repo: String, session_id: String, turn: u32) -> Result<(), String> {
    parzi_runtime::git_checkpoints::restore_checkpoint(&repo, &session_id, turn)
        .map_err(|e| e.to_string())
}

/// File picker backend for @-autocomplete: shallow walk, hidden/dirs skipped.
#[tauri::command]
async fn git_branch(cwd: String) -> Result<String, String> {
    if cwd.is_empty() {
        return Ok(String::new());
    }
    let out = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&cwd)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
#[tauri::command]
async fn list_files(root: String, query: String) -> Result<Vec<String>, String> {
    if root.is_empty() {
        return Ok(vec![]);
    }
    const SKIP: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        "dist",
        ".venv",
        "__pycache__",
    ];
    let q = query.to_lowercase();
    let mut out = vec![];
    let mut stack = vec![(std::path::PathBuf::from(&root), 0u8)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 4 || out.len() >= 200 {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                stack.push((p, depth + 1));
            } else if let Ok(rel) = p.strip_prefix(&root) {
                let s = rel.to_string_lossy().replace('\\', "/");
                if q.is_empty() || s.to_lowercase().contains(&q) {
                    out.push(s);
                }
                if out.len() >= 200 {
                    break;
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

#[tauri::command]
async fn create_project(name: String, root: String) -> Result<(), String> {
    if name.trim().is_empty()
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err("project name: letters, numbers, _ and - only".into());
    }
    let dir = parzi_core::paths::projects_dir()
        .map_err(|e| e.to_string())?
        .join(&name);
    std::fs::create_dir_all(dir.join("lanes")).map_err(|e| e.to_string())?;
    let toml = format!("root = \"{}\"\n", root.replace('\\', "/"));
    if !dir.join("parzi.toml").exists() {
        std::fs::write(dir.join("parzi.toml"), toml).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn toggle_plugin(name: String, enabled: bool) -> Result<(), String> {
    parzi_runtime::plugins::set_enabled(&name, enabled).map_err(|e| e.to_string())
}

/// Inspector deck: read one text file for side-by-side viewing. Bounded to
/// 2 MiB so a stray binary or log cannot balloon the webview; lossy UTF-8.
/// H-2: confined to the Parzi home tree and registered project roots.
const READ_TEXT_MAX: u64 = 2 * 1024 * 1024;

#[tauri::command]
async fn read_text_file(path: String) -> Result<String, String> {
    let p = confined_path(&path)?;
    let meta = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", p.display()));
    }
    if meta.len() > READ_TEXT_MAX {
        return Err(format!(
            "{} is {} KiB — over the {} KiB inspector limit",
            p.display(),
            meta.len() / 1024,
            READ_TEXT_MAX / 1024
        ));
    }
    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Lexical `.`/`..` normalization without touching disk (floors at the
/// prefix/root, matching OS semantics). Pure and unit-tested.
fn normalize_lexical(p: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    // Above the root: flooring keeps `C:\..\x` == `C:\x`.
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(std::path::MAIN_SEPARATOR.to_string());
    }
    out
}

fn contained_in(target: &std::path::Path, roots: &[std::path::PathBuf]) -> bool {
    roots.iter().any(|r| target.starts_with(r))
}

/// Files chosen in a native dialog, granted access for the current session.
static PICKED_FILES: std::sync::Mutex<Option<std::collections::BTreeSet<std::path::PathBuf>>> =
    std::sync::Mutex::new(None);

fn grant_picked(path: &std::path::Path) {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Ok(mut g) = PICKED_FILES.lock() {
        g.get_or_insert_with(Default::default).insert(canon);
    }
}

fn was_picked(canon: &std::path::Path) -> bool {
    PICKED_FILES
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.contains(canon)))
        .unwrap_or(false)
}

/// Pick a text file via native dialog and grant session access to it.
#[tauri::command]
async fn pick_text_file(app: AppHandle, start: Option<String>) -> Result<Option<String>, String> {
    let mut builder = app
        .dialog()
        .file()
        .set_title("Open a text file")
        .add_filter("Markdown / text", &["md", "markdown", "txt", "mdx"]);
    if let Some(dir) = start.filter(|s| !s.trim().is_empty()) {
        builder = builder.set_directory(dir);
    }
    let (tx, rx) = oneshot::channel();
    builder.pick_file(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.map_err(|_| "the file dialog closed".to_string())? else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    grant_picked(&path);
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Resolve every configured file root the shell may touch: the Parzi home
/// tree plus each registered project/lane root. Canonicalized; missing roots
/// are skipped.
fn file_roots() -> Vec<std::path::PathBuf> {
    let mut roots = vec![];
    if let Ok(home) = parzi_core::paths::parzi_dir() {
        roots.push(home.canonicalize().unwrap_or_else(|_| {
            parzi_core::paths::parzi_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        }));
    }
    if let Ok(scan) = parzi_core::lanes::scan_projects() {
        for (proj, lanes) in scan {
            for root in std::iter::once(proj.root)
                .flatten()
                .chain(lanes.into_iter().filter_map(|l| l.root))
            {
                if root.trim().is_empty() {
                    continue;
                }
                let rb = std::path::PathBuf::from(&root);
                roots.push(rb.canonicalize().unwrap_or(rb));
            }
        }
    }
    // Hub workspaces map their own checkouts.
    for name in parzi_core::workspace::list() {
        let Ok(ws) = parzi_core::workspace::load(&name) else {
            continue;
        };
        for repo in ws.repos {
            let Some(path) = repo.local_path else {
                continue;
            };
            if path.as_os_str().is_empty() {
                continue;
            }
            roots.push(path.canonicalize().unwrap_or(path));
        }
    }
    roots
}

/// Gate for shell-side file access: absolute path, lexically normalized,
/// canonicalized, and required to sit under `file_roots()` or `PICKED_FILES`.
fn confined_path(raw: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::PathBuf::from(raw.trim());
    if p.as_os_str().is_empty() {
        return Err("empty path".into());
    }
    if !p.is_absolute() {
        return Err("path must be absolute".into());
    }
    let normal = normalize_lexical(&p);
    let mut probe: Option<&std::path::Path> = Some(&normal);
    let mut canon: Option<std::path::PathBuf> = None;
    while let Some(q) = probe {
        if let Ok(c) = q.canonicalize() {
            canon = Some(c);
            break;
        }
        probe = q.parent();
    }
    let canon = canon.ok_or_else(|| "cannot resolve path".to_string())?;
    if contained_in(&canon, &file_roots()) || was_picked(&canon) {
        Ok(normal)
    } else {
        Err("that location is outside the workspace — use Open file to pick it".into())
    }
}

#[derive(Debug, Clone, Serialize)]
struct DocEntry {
    /// Short label shown on the quick tab (`SYSTEM.md`, `PLAN.md`, `docs/x.md`).
    label: String,
    path: String,
    /// `system` = project prompt under ~/.parzi, `root` = file in the workspace.
    source: String,
}

/// Inspector deck: markdown documents worth a quick tab for a project —
/// the project SYSTEM.md plus every `.md` in the workspace root (depth ≤ 2,
/// well-known names first). Missing root = SYSTEM.md only.
#[tauri::command]
async fn list_project_docs(project: String, root: String) -> Result<Vec<DocEntry>, String> {
    let mut out = vec![];
    if !project.trim().is_empty() {
        if let Ok(dir) = parzi_core::paths::projects_dir() {
            let sys = dir.join(project.trim()).join("SYSTEM.md");
            if sys.is_file() {
                out.push(DocEntry {
                    label: "SYSTEM.md".into(),
                    path: sys.to_string_lossy().to_string(),
                    source: "system".into(),
                });
            }
        }
    }
    let root = root.trim();
    if root.is_empty() {
        return Ok(out);
    }
    const SKIP: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        "dist",
        ".venv",
        "__pycache__",
    ];
    const FIRST: &[&str] = &[
        "PLAN.md",
        "README.md",
        "AGENTS.md",
        "CLAUDE.md",
        "PROGRESS.md",
        "TODO.md",
    ];
    let root_path = std::path::PathBuf::from(root);
    let mut found: Vec<(String, String)> = vec![];
    let mut stack = vec![(root_path.clone(), 0u8)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 2 || found.len() >= 80 {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                stack.push((p, depth + 1));
            } else if name.to_lowercase().ends_with(".md") {
                if let Ok(rel) = p.strip_prefix(&root_path) {
                    let label = rel.to_string_lossy().replace('\\', "/");
                    found.push((label, p.to_string_lossy().to_string()));
                }
            }
        }
    }
    found.sort_by(|a, b| {
        let ra = FIRST.iter().position(|f| *f == a.0).unwrap_or(FIRST.len());
        let rb = FIRST.iter().position(|f| *f == b.0).unwrap_or(FIRST.len());
        ra.cmp(&rb)
            .then_with(|| a.0.matches('/').count().cmp(&b.0.matches('/').count()))
            .then_with(|| a.0.cmp(&b.0))
    });
    for (label, path) in found {
        out.push(DocEntry {
            label,
            path,
            source: "root".into(),
        });
    }
    Ok(out)
}

/// Settings → Context: bounded text write for project memory files.
/// Mirrors the `read_text_file` cap; creates missing parent dirs.
#[tauri::command]
async fn write_text_file(path: String, content: String) -> Result<(), String> {
    const WRITE_TEXT_MAX: usize = 2 * 1024 * 1024;
    if content.len() > WRITE_TEXT_MAX {
        return Err(format!(
            "refusing to write {} KiB — over the {} KiB limit",
            content.len() / 1024,
            WRITE_TEXT_MAX / 1024
        ));
    }
    let p = confined_path(&path)?;
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(&p, content).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(())
}

/// Settings → Context: create or replace a project's SYSTEM.md prompt.
#[tauri::command]
async fn save_project_system(project: String, content: String) -> Result<String, String> {
    let name = parzi_core::lanes::safe_name(&project).map_err(|e| e.to_string())?;
    let path = parzi_core::paths::projects_dir()
        .map_err(|e| e.to_string())?
        .join(name)
        .join("SYSTEM.md");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, &content).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// Settings → Skills: scaffold a new `commands` skill pack.
#[tauri::command]
async fn create_skill(name: String) -> Result<(), String> {
    parzi_runtime::plugins::create_commands_pack(name.trim()).map_err(|e| e.to_string())
}

/// Settings → Skills: slash commands inside one pack (empty when none yet).
#[tauri::command]
async fn skill_commands(name: String) -> Result<Vec<parzi_runtime::plugins::SlashCommand>, String> {
    parzi_runtime::plugins::commands_for(name.trim()).map_err(|e| e.to_string())
}

/// Settings → Skills: replace a skill's command list wholesale.
#[tauri::command]
async fn save_skill_commands(
    name: String,
    commands: Vec<parzi_runtime::plugins::SlashCommand>,
) -> Result<(), String> {
    parzi_runtime::plugins::save_commands(name.trim(), &commands).map_err(|e| e.to_string())
}

/// Report-an-issue flow: open a URL in the system browser. Allowlisted to
/// the project's own GitHub pages so this can never become an open redirect.
#[tauri::command]
async fn open_external_url(url: String) -> Result<(), String> {
    const ALLOW: &[&str] = &[
        "https://github.com/lucas19919/Parzi/",
        "https://github.com/parzi/parzi/",
    ];
    if url.len() > 8192
        || url.chars().any(char::is_whitespace)
        || !ALLOW.iter().any(|base| url.starts_with(base))
    {
        return Err("that link isn't allowed".into());
    }
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("cmd")
        .args(["/C", "start", "", &url])
        .status()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open")
        .arg(&url)
        .status()
        .map_err(|e| e.to_string())?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let status = std::process::Command::new("xdg-open")
        .arg(&url)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("couldn't open the browser".into())
    }
}

#[tauri::command]
fn window_minimize(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
fn window_maximize(window: tauri::Window) -> Result<bool, String> {
    let is_max = window.is_maximized().map_err(|e| e.to_string())?;
    if is_max {
        window.unmaximize().map_err(|e| e.to_string())?;
        Ok(false)
    } else {
        window.maximize().map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[tauri::command]
fn window_close(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
fn window_start_dragging(window: tauri::Window) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

#[tauri::command]
async fn migrate_tasks(project: String) -> Result<usize, String> {
    parzi_core::lanes::migrate_tasks_to_lanes(&project).map_err(|e| e.to_string())
}

/// How long a day's log is kept.
const LOG_DAYS: u64 = 7;

/// A desktop app has no console: runs, failed turns and warnings go to
/// `~/.parzi/logs/parzi-<date>.log`, one file a day, a week kept. Without a
/// home to write to the app runs unlogged rather than not at all.
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

fn main() {
    init_log();
    // E9: Tauri's default runtime is one worker per hardware thread (16 here,
    // 27 threads in the process at idle). The host only moves IPC and I/O —
    // every CPU-bound step already runs under spawn_blocking — so three
    // workers and a small blocking pool are plenty.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(3)
        .max_blocking_threads(8)
        .enable_all()
        .build()
        .expect("tokio runtime");
    tauri::async_runtime::set(rt.handle().clone());
    let (cfg, store) = boot().expect("parzi home");
    let orch = Arc::new(Orchestrator::new(cfg, store));
    orch.recover().ok();
    let pending: Arc<Mutex<HashMap<String, oneshot::Sender<Approval>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let o = orch.clone();
            app.manage(AppState {
                orch,
                pending,
                app: app.handle().clone(),
            });
            #[cfg(target_os = "windows")]
            if let Some(w) = app.get_webview_window("main") {
                dwm::round_window_corners(&w);
            }
            // R-5: subscribe before any run starts, including recovered
            // queued ones. Dispatched lanes drop their own receiver; this
            // is the app's one ear on every run.
            let bus_rx = o.subscribe();
            let bus_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                forward_host_bus(bus_app, bus_rx).await;
            });
            // Each agent's own program is asked once at start, in the
            // background: the picker shows the last known board at once and
            // the fresh one when it lands.
            let board = o.clone();
            let board_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let all = in_roster_order(board.refresh_providers(&[]).await);
                let _ = board_app.emit("parzi://providers", &all);
            });
            // Long-lived queue pump + boot kick for sessions left Queued.
            // R-5: runs that were still queued when the app closed are put
            // back on the queue first — their prompts live next to the
            // transcript, so nothing is lost with the process.
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
            window_minimize,
            window_maximize,
            window_close,
            window_start_dragging,
            migrate_tasks,
            list_threads,
            get_thread,
            send_message,
            kill_run,
            compact_thread,
            fork_thread,
            create_subsession,
            reparent_thread,
            toggle_pin,
            rename_thread,
            delete_thread,
            list_projects,
            get_project_roster,
            save_project_roster,
            get_project_plan,
            save_project_plan,
            get_project_knowledge,
            save_project_knowledge,
            get_worktree_diff,
            apply_worktree,
            list_checkpoints,
            restore_checkpoint,
            list_files,
            create_project,
            delete_project,
            git_branch,
            approve_tool,
            provider_statuses,
            refresh_providers,
            toggle_favorite,
            get_theme_css,
            save_theme,
            get_theme,
            reset_theme,
            purge_sessions,
            get_config,
            save_config,
            list_packs,
            list_pack_infos,
            save_pack,
            apply_pack,
            delete_pack,
            rename_pack,
            get_user_css,
            save_user_css,
            list_backgrounds,
            set_background,
            delete_background,
            upload_background,
            save_background_data,
            background_file,
            background_url,
            list_background_urls,
            palette_from_background,
            run_doctor,
            run_doctor_quick,
            run_doctor_mcp,
            list_mcp_tools,
            list_all_mcp_tools,
            list_builtin_tools,
            list_plugins,
            toggle_plugin,
            install_pasted_skill,
            install_skill_from_git,
            delete_skill,
            open_external_url,
            read_text_file,
            pick_text_file,
            write_text_file,
            read_image_data_url,
            stage_image,
            save_project_system,
            create_skill,
            skill_commands,
            save_skill_commands,
            list_project_docs,
            // hub (PLAN.md): workspaces, GitHub, projects, the deck's reads.
            hub_cmds::workspace_list,
            hub_cmds::workspace_create,
            hub_cmds::workspace_get,
            hub_cmds::workspace_add_repos,
            hub_cmds::workspace_delete,
            hub_cmds::workspace_migrate,
            hub_cmds::workspace_sync,
            hub_cmds::github_connect,
            hub_cmds::github_status,
            hub_cmds::github_list_orgs,
            hub_cmds::github_list_repos,
            hub_cmds::repo_clone_or_map,
            hub_cmds::project_create,
            hub_cmds::project_get,
            hub_cmds::project_rename,
            hub_cmds::project_delete,
            hub_cmds::project_save,
            hub_cmds::project_list,
            hub_cmds::project_open,
            hub_cmds::project_drafts,
            hub_cmds::project_audit,
            hub_cmds::project_approve,
            hub_cmds::project_status,
            hub_cmds::project_plan,
            hub_cmds::project_journal,
            hub_cmds::ui_state,
        ])
        .build(tauri::generate_context!())
        .expect("parzi failed to start")
        .run(|app, event| {
            // E8/R-2: MCP servers are our children. Kill them (and their
            // trees) before the process goes away, or every `npx` connector
            // touched this session outlives the app.
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(state) = app.try_state::<AppState>() {
                    let mcp = state.orch.mcp().clone();
                    tauri::async_runtime::block_on(async move { mcp.shutdown().await });
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_normalize_floors_at_root() {
        let norm = |s: &str| normalize_lexical(std::path::Path::new(s));
        assert_eq!(
            norm("C:\\proj\\a\\..\\b"),
            std::path::PathBuf::from("C:\\proj\\b")
        );
        // `..` above the root floors instead of escaping.
        assert_eq!(norm("C:\\..\\x"), std::path::PathBuf::from("C:\\x"));
        assert_eq!(norm("/a/./b"), std::path::PathBuf::from("/a/b"));
    }

    #[test]
    fn containment_is_prefix_based() {
        let roots = vec![std::path::PathBuf::from("C:\\parzi")];
        assert!(contained_in(
            std::path::Path::new("C:\\parzi\\a.md"),
            &roots
        ));
        assert!(!contained_in(
            std::path::Path::new("C:\\other\\a.md"),
            &roots
        ));
        // `C:\parzi-evil` shares a string prefix but is not under the root.
        assert!(!contained_in(
            std::path::Path::new("C:\\parzi-evil\\a.md"),
            &roots
        ));
    }

    #[test]
    fn a_set_or_edited_agent_program_is_confirmed() {
        let with = |path: &str| {
            let mut cfg = ParziConfig::default();
            cfg.providers.insert(
                "claude".into(),
                parzi_core::config::ProviderEntry {
                    binary: path.to_string(),
                    ..Default::default()
                },
            );
            cfg
        };
        let none = with("");
        let set = with(r"C:\tools\claude.exe");
        assert_eq!(
            binary_changes(&none, &set),
            vec![r"claude → C:\tools\claude.exe".to_string()]
        );
        assert!(
            binary_changes(&set, &set).is_empty(),
            "an identical resave is silent"
        );
        assert_eq!(binary_changes(&set, &with(r"C:\evil\claude.exe")).len(), 1);
        assert!(
            binary_changes(&set, &none).is_empty(),
            "back to PATH does not prompt"
        );
        assert!(
            binary_changes(&none, &with("  ")).is_empty(),
            "blank is no path"
        );
        assert_eq!(
            binary_changes(&ParziConfig::default(), &set).len(),
            1,
            "a provider the old file never named"
        );
    }

    #[test]
    fn command_changes_detected_add_and_edit_only() {
        let old = ParziConfig::default();
        let mut new = ParziConfig::default();
        new.mcp.servers.insert(
            "s".into(),
            parzi_core::config::McpServerCfg {
                command: "npx".into(),
                args: vec!["-y".into()],
                ..Default::default()
            },
        );
        assert_eq!(mcp_command_changes(&old, &new), vec!["s".to_string()]);
        // Identical resave is silent.
        assert!(mcp_command_changes(&new, &new).is_empty());
        // Removal alone does not prompt.
        assert!(mcp_command_changes(&new, &old).is_empty());
    }
}
