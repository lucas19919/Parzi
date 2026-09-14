//! parzi-app: thin Tauri glue. No business logic — every command delegates
//! to parzi-core / parzi-runtime. Events stream to the UI, never block it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::store::{Event, SessionMeta, SessionStore};
use parzi_core::theme::Theme;
use parzi_runtime::handler::RunEvent;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};
use parzi_runtime::Orchestrator;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, oneshot};

struct AppState {
    orch: Arc<Orchestrator>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Approval>>>>,
    app: AppHandle,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum UiEvent {
    Text { session: String, text: String },
    Reasoning { session: String, text: String },
    ToolCall { session: String, id: String, name: String },
    ToolResult { session: String, name: String, ok: bool, ms: u128 },
    Notice { session: String, text: String },
    RouteTransition { session: String, from_provider: String, to_provider: String, reason: String, cooldown_secs: Option<u64> },
    Usage { session: String, tokens_in: u64, tokens_out: u64, cost_usd: f64 },
    Approval { key: String, call: ToolCallView },
    SubsessionCreated { parent_id: String, subsession: SessionMeta },
    Done { session: String, turns: u32 },
    Error { session: String, error: String },
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
        let _ = self.app.emit("parzi://run-event", UiEvent::Approval { key: key.clone(), call: view });
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
    Ok(format!("{}-v3", env!("CARGO_PKG_VERSION")))
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
async fn effort_options(provider: String) -> Result<Vec<parzi_providers::router::EffortOption>, String> {
    Ok(parzi_providers::router::effort_options(&provider))
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
    let (sid, mut rx) = match session_id {
        Some(id) => {
            // Per-message model switch: the thread follows the picker.
            let rx = state.orch.send_to(&id, &prompt, Some(approver), &cwd, &effort, attached, Some(model.clone())).await.map_err(|e| e.to_string())?;
            (id, rx)
        }
        None => {
            // UI-spawned subsession: nest under the parent, inheriting its
            // project/lane/cwd (and model when the picker is untouched).
            if let Some(pid) = parent_id.filter(|p| !p.trim().is_empty()) {
                let pmeta = state.orch.store().get(&pid).map_err(|e| e.to_string())?;
                let title: String =
                    prompt.lines().next().unwrap_or("subsession").chars().take(80).collect();
                let lane = if lane.is_empty() { pmeta.lane.clone() } else { lane };
                let model = if model.is_empty() { pmeta.model.clone() } else { model };
                let cwd = if cwd.is_empty() { pmeta.cwd.clone() } else { cwd };
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
                    .send_to(&meta.id, &prompt, Some(approver), &cwd, &effort, attached, Some(model.clone()))
                    .await
                    .map_err(|e| e.to_string())?;
                let _ = app.emit(
                    "parzi://run-event",
                    UiEvent::SubsessionCreated { parent_id: pid, subsession: meta.clone() },
                );
                (meta.id, rx)
            } else {
                let (meta, rx) = state
                    .orch
                    .spawn(&project, &lane, &model, &prompt, Some(approver), &cwd, &effort, attached)
                    .await
                    .map_err(|e| e.to_string())?;
                (meta.id, rx)
            }
        }
    };
    let sid_task = sid.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let ui = match ev {
                RunEvent::Text(text) => UiEvent::Text { session: sid_task.clone(), text },
                RunEvent::Reasoning { text } => {
                    UiEvent::Reasoning { session: sid_task.clone(), text }
                }
                RunEvent::ToolCall { id, name } => {
                    UiEvent::ToolCall { session: sid_task.clone(), id, name }
                }
                RunEvent::ToolResult { name, ok, ms } => {
                    UiEvent::ToolResult { session: sid_task.clone(), name, ok, ms }
                }
                RunEvent::Notice { text } => {
                    UiEvent::Notice { session: sid_task.clone(), text }
                }
                RunEvent::RouteTransition { from_provider, to_provider, reason, cooldown_secs } => {
                    UiEvent::RouteTransition { session: sid_task.clone(), from_provider, to_provider, reason, cooldown_secs }
                }
                RunEvent::Usage { tokens_in, tokens_out, cost_usd } => UiEvent::Usage {
                    session: sid_task.clone(),
                    tokens_in,
                    tokens_out,
                    cost_usd,
                },
                RunEvent::ApprovalRequest { .. } => continue, // GuiApprover emits its own
                RunEvent::Done { turns } => UiEvent::Done { session: sid_task.clone(), turns },
                RunEvent::Error(error) => UiEvent::Error { session: sid_task.clone(), error },
            };
            let _ = app.emit("parzi://run-event", ui);
        }
    });
    Ok(sid)
}

#[tauri::command]
async fn kill_run(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.orch.kill(&id).await.map_err(|e| e.to_string())
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
        UiEvent::SubsessionCreated { parent_id, subsession: meta.clone() },
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

/// Read @-attached files (cap 8, 12k chars each), resolved against cwd.
fn read_attachments(cwd: &str, paths: &[String]) -> Vec<parzi_core::context::AttachedFile> {
    let base = if cwd.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        std::path::PathBuf::from(cwd)
    };
    paths
        .iter()
        .take(8)
        .filter_map(|p| {
            // Contain to cwd (lexical) — same rule as the tool sandbox.
            let full = base.join(p);
            std::fs::read_to_string(&full).ok().map(|t| {
                parzi_core::context::AttachedFile {
                    path: p.clone(),
                    snippet: t.chars().take(12_000).collect(),
                }
            })
        })
        .collect()
}

#[tauri::command]
async fn toggle_pin(state: State<'_, AppState>, id: String, pinned: bool) -> Result<(), String> {
    state.orch.store().set_pinned(&id, pinned).map_err(|e| e.to_string())
}

#[tauri::command]
async fn approve_tool(
    state: State<'_, AppState>,
    key: String,
    allow: bool,
) -> Result<(), String> {
    let tx = state.pending.lock().await.remove(&key);
    match tx {
        Some(tx) => {
            let _ = tx.send(if allow { Approval::Allow } else { Approval::Deny });
            Ok(())
        }
        None => Err("approval expired or unknown".into()),
    }
}

#[derive(Debug, Clone, Serialize)]
struct ModelRow {
    provider: String,
    /// ok | missing | expired
    auth: String,
    /// subscription | api_key | none — class of the credential that won.
    billing: String,
    /// What to do when not signed in, or the plan label when signed in.
    hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<String>,
    /// Whether Settings can store an API key for this provider.
    takes_key: bool,
    models: Vec<parzi_providers::Model>,
}

/// One provider's picker row: auth status + catalog (live pull when the
/// config allows it, otherwise disk cache, otherwise static). Never fails —
/// an unknown id reports `missing` so the UI can show a hint, not a crash.
async fn model_row_for(id: &str, cfg: ParziConfig) -> ModelRow {
    let takes_key = parzi_providers::key_entry(id).is_some();
    match parzi_providers::provider(id, &cfg) {
        Err(e) => ModelRow {
            provider: id.into(),
            auth: "missing".into(),
            billing: "none".into(),
            hint: e.to_string(),
            account: None,
            takes_key,
            models: vec![],
        },
        Ok(p) => {
            let (auth, hint) = match p.auth_status() {
                parzi_providers::AuthStatus::Ok => ("ok", String::new()),
                parzi_providers::AuthStatus::Missing(h) => ("missing", h),
                parzi_providers::AuthStatus::Expired(h) => ("expired", h),
            };
            // Live pulls can hang on network: bound each provider.
            let mut models = tokio::time::timeout(std::time::Duration::from_secs(10), p.models())
                .await
                .ok()
                .and_then(|r| r.ok())
                .unwrap_or_default();
            parzi_providers::catalog::sort_models(&mut models);
            ModelRow {
                provider: id.into(),
                auth: auth.into(),
                billing: p.billing().as_str().into(),
                hint,
                account: p.account_label(),
                takes_key,
                models,
            }
        }
    }
}

#[tauri::command]
async fn get_models(
    state: State<'_, AppState>,
    refresh: Option<bool>,
) -> Result<Vec<ModelRow>, String> {
    // Fast path (default): auth status + cached/static catalog, no network.
    // Full refresh (`refresh: true`): live /models pulls, providers queried
    // concurrently with a per-provider timeout so one hung vendor can't
    // stall settings.
    let mut cfg = state.orch.config().clone();
    if refresh != Some(true) {
        cfg.catalog_refresh = false;
    }
    let mut set = tokio::task::JoinSet::new();
    for id in parzi_providers::PROVIDERS.iter().copied() {
        let cfg = cfg.clone();
        set.spawn(async move { model_row_for(id, cfg).await });
    }
    let mut out = vec![];
    while let Some(row) = set.join_next().await {
        if let Ok(row) = row {
            out.push(row);
        }
    }
    // Fixed roster order (picker, settings, sidebar all agree). Auth state
    // is shown per row, never used to reshuffle — a provider stays where
    // the user expects it whether or not it is signed in today.
    out.sort_by_key(|r| {
        parzi_providers::PROVIDERS
            .iter()
            .position(|p| *p == r.provider)
            .unwrap_or(usize::MAX)
    });
    Ok(out)
}

/// Live catalog for exactly one provider (the model picker's second step).
/// Forces a `/models` pull (loopback servers like `opencode serve` answer
/// without auth), so opening e.g. OpenCode always shows what is served
/// right now — Big Pickle, Muse, whatever landed upstream. Times out
/// like the bulk path; unknown ids are rejected up front.
#[tauri::command]
async fn refresh_provider(
    state: State<'_, AppState>,
    provider: String,
) -> Result<ModelRow, String> {
    let id = parzi_providers::canonical_id(provider.trim())
        .ok_or_else(|| "unknown provider (Parzi routes claude, codex, antigravity, opencode, xai)".to_string())?;
    let mut cfg = state.orch.config().clone();
    cfg.catalog_refresh = true;
    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        model_row_for(id, cfg),
    )
    .await
    .map_err(|_| format!("{id} took too long — check the connection"))
}

#[tauri::command]
async fn get_provider_health(state: State<'_, AppState>) -> Result<Vec<parzi_providers::ProviderHealth>, String> {
    Ok(state.orch.provider_health())
}

#[tauri::command]
async fn reset_circuit_breaker(state: State<'_, AppState>, provider: Option<String>) -> Result<(), String> {
    state.orch.reset_circuit_breaker(provider.as_deref());
    Ok(())
}

/// Store an API key for a roster provider. The keyring entry name is the
/// provider's key slot (`claude` → `anthropic`, `codex` → `openai`), so
/// subscription tokens under other names are never overwritten.
#[tauri::command]
async fn save_key(provider: String, value: String) -> Result<(), String> {
    let slot = parzi_providers::key_entry(&provider)
        .ok_or_else(|| format!("{provider} takes no API key"))?;
    let value = value.trim();
    if value.is_empty() {
        return Err("empty key".into());
    }
    let entry = keyring::Entry::new("parzi", slot).map_err(|e| e.to_string())?;
    entry.set_password(value).map_err(|e| e.to_string())
}

/// Remove a stored API key (subscription sign-ins are untouched).
#[tauri::command]
async fn delete_key(provider: String) -> Result<(), String> {
    let slot = parzi_providers::key_entry(&provider)
        .ok_or_else(|| format!("{provider} takes no API key"))?;
    if let Ok(e) = keyring::Entry::new("parzi", slot) {
        let _ = e.delete_credential();
    }
    Ok(())
}

#[tauri::command]
async fn reset_theme() -> Result<String, String> {
    let theme = Theme::default();
    theme.save().map_err(|e| e.to_string())?;
    get_theme_css().await
}

#[tauri::command]
async fn purge_sessions(state: State<'_, AppState>) -> Result<usize, String> {
    state.orch.store().purge_finished().map_err(|e| e.to_string())
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
async fn save_config(cfg: ParziConfig) -> Result<(), String> {
    if cfg.version != parzi_core::config::CONFIG_VERSION {
        return Err("config version mismatch — reload settings".into());
    }
    cfg.save().map_err(|e| e.to_string())
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
async fn apply_pack(name: String) -> Result<String, String> {
    let theme = parzi_core::theme::apply_pack(&name).map_err(|e| e.to_string())?;
    Ok(theme_css(&theme))
}

#[tauri::command]
async fn background_url(app: AppHandle) -> Result<String, String> {
    // Return a plain absolute path; the UI turns it into an asset URL with
    // convertFileSrc (asset protocol scope is configured in tauri.conf.json).
    let theme = Theme::load().map_err(|e| e.to_string())?;
    if theme.background.image.is_empty() {
        return Ok(String::new());
    }
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    let abs = home.join(".parzi").join(&theme.background.image);
    if abs.exists() {
        return Ok(abs.to_string_lossy().to_string());
    }
    let res = app
        .path()
        .resolve("asuka.png", tauri::path::BaseDirectory::Resource)
        .map_err(|e| e.to_string())?;
    Ok(if res.exists() { res.to_string_lossy().to_string() } else { String::new() })
}

#[tauri::command]
async fn run_doctor(state: State<'_, AppState>) -> Result<Vec<parzi_runtime::doctor::Check>, String> {
    let cfg = state.orch.config().clone();
    Ok(parzi_runtime::doctor::Doctor::new(cfg).run().await)
}

/// Fast health subset (no MCP probes). System tab renders this immediately.
#[tauri::command]
async fn run_doctor_quick(state: State<'_, AppState>) -> Result<Vec<parzi_runtime::doctor::Check>, String> {
    let cfg = state.orch.config().clone();
    Ok(parzi_runtime::doctor::Doctor::new(cfg).run_quick().await)
}

/// MCP server probes only (each can take up to 15s). Loaded lazily.
#[tauri::command]
async fn run_doctor_mcp(state: State<'_, AppState>) -> Result<Vec<parzi_runtime::doctor::Check>, String> {
    let cfg = state.orch.config().clone();
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
    let clean_name = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '_' || *c == '-')
        .collect::<String>();
    if clean_name.is_empty() {
        return Err("invalid background filename".into());
    }
    let bytes = decode_b64(&base64_data).ok_or("invalid base64 image data")?;
    let dest = parzi_core::paths::backgrounds_dir()
        .map_err(|e| e.to_string())?
        .join(&clean_name);
    std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    let _ = parzi_core::theme::set_background(&clean_name);
    Ok(dest.to_string_lossy().to_string())
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
        Some(n) if !n.is_empty() => {
            if n.contains('/') || n.contains('\\') || n.contains("..") {
                return Err("bad background name".into());
            }
            parzi_core::paths::backgrounds_dir()
                .map_err(|e| e.to_string())?
                .join(n)
        }
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
    parzi_core::theme::extract_palette(&path).map_err(|e| e.to_string())
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
        .map(|p| PluginView {
            name: p.manifest.name,
            version: p.manifest.version,
            kind: p.manifest.kind,
            enabled: p.enabled,
        })
        .collect())
}

#[derive(Debug, Clone, Serialize)]
struct PluginView {
    name: String,
    version: String,
    kind: String,
    enabled: bool,
}

#[tauri::command]
async fn rename_thread(state: State<'_, AppState>, id: String, title: String) -> Result<(), String> {
    state.orch.store().set_title(&id, &title).map_err(|e| e.to_string())
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
    state.orch.store().delete_thread(&id).map_err(|e| e.to_string())
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
    const SKIP: &[&str] = &[".git", "node_modules", "target", "dist", ".venv", "__pycache__"];
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
        || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
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
const READ_TEXT_MAX: u64 = 2 * 1024 * 1024;

#[tauri::command]
async fn read_text_file(path: String) -> Result<String, String> {
    let p = std::path::PathBuf::from(path.trim());
    if p.as_os_str().is_empty() {
        return Err("empty path".into());
    }
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
    const SKIP: &[&str] = &[".git", "node_modules", "target", "dist", ".venv", "__pycache__"];
    const FIRST: &[&str] = &["PLAN.md", "README.md", "AGENTS.md", "CLAUDE.md", "PROGRESS.md", "TODO.md"];
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
        ra.cmp(&rb).then_with(|| a.0.matches('/').count().cmp(&b.0.matches('/').count())).then_with(|| a.0.cmp(&b.0))
    });
    for (label, path) in found {
        out.push(DocEntry { label, path, source: "root".into() });
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
    let p = std::path::PathBuf::from(path.trim());
    if p.as_os_str().is_empty() {
        return Err("empty path".into());
    }
    if !p.is_absolute() {
        return Err("path must be absolute".into());
    }
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
    let name = project.trim();
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
        return Err("invalid project name".into());
    }
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
async fn skill_commands(
    name: String,
) -> Result<Vec<parzi_runtime::plugins::SlashCommand>, String> {
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

#[tauri::command]
async fn login_antigravity() -> Result<String, String> {
    let url = parzi_providers::antigravity_oauth::auth_url();
    parzi_providers::antigravity_oauth::open_browser(&url);

    let code = parzi_providers::antigravity_oauth::wait_for_code(300)
        .await
        .map_err(|e| e.to_string())?;
    let toks = parzi_providers::antigravity_oauth::exchange_code(&code)
        .await
        .map_err(|e| e.to_string())?;

    keyring::Entry::new("parzi", "antigravity")
        .map_err(|e| e.to_string())?
        .set_password(&toks.access)
        .map_err(|e| e.to_string())?;
    if let Some(r) = toks.refresh {
        keyring::Entry::new("parzi", "antigravity-refresh")
            .map_err(|e| e.to_string())?
            .set_password(&r)
            .map_err(|e| e.to_string())?;
    }
    Ok("Signed in with Google".to_string())
}

#[tauri::command]
async fn logout_antigravity() -> Result<(), String> {
    for entry in ["antigravity", "antigravity-refresh", "antigravity-session"] {
        if let Ok(e) = keyring::Entry::new("parzi", entry) {
            let _ = e.delete_credential();
        }
    }
    Ok(())
}

fn main() {
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
            app.manage(AppState { orch, pending, app: app.handle().clone() });
            // Long-lived queue pump + boot kick for sessions left Queued.
            tauri::async_runtime::spawn(async move {
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
            login_antigravity,
            logout_antigravity,
            migrate_tasks,
            list_threads,
            get_thread,
            send_message,
            kill_run,
            fork_thread,
            create_subsession,
            reparent_thread,
            toggle_pin,
            rename_thread,
            delete_thread,
            list_projects,
            list_files,
            create_project,
            delete_project,
            git_branch,
            approve_tool,
            get_models,
            refresh_provider,
            get_provider_health,
            reset_circuit_breaker,
            save_key,
            delete_key,
            toggle_favorite,
            effort_options,
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
            get_user_css,
            save_user_css,
            list_backgrounds,
            set_background,
            upload_background,
            save_background_data,
            background_file,
            background_url,
            list_background_urls,
            palette_from_background,
            run_doctor,
            run_doctor_quick,
            run_doctor_mcp,
            list_plugins,
            toggle_plugin,
            read_text_file,
            write_text_file,
            save_project_system,
            create_skill,
            skill_commands,
            save_skill_commands,
            list_project_docs,
        ])
        .run(tauri::generate_context!())
        .expect("parzi failed to start");
}
