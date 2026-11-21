use std::path::PathBuf;
use std::sync::{OnceLock, PoisonError, RwLock};
use std::time::Duration;

use parzi_core::config::ParziConfig;
use parzi_providers::ProviderStatus;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::AppState;

#[tauri::command]
pub async fn get_config() -> Result<ParziConfig, String> {
    ParziConfig::load().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    cfg: ParziConfig,
) -> Result<(), String> {
    if cfg.version != parzi_core::config::CONFIG_VERSION {
        return Err("config version mismatch — reload settings".into());
    }
    let old = ParziConfig::load().unwrap_or_default();
    let mut changed: Vec<String> = mcp_command_changes(&old, &cfg)
        .into_iter()
        .map(|name| {
            let srv = &cfg.mcp.servers[&name];
            format!("{name}: {} {}", srv.command, srv.args.join(" "))
        })
        .collect();
    changed.extend(binary_changes(&old, &cfg));
    if !changed.is_empty() {
        let msg = format!(
            "These run as programs on this computer, with your account:\n\n{}\n\nOnly add ones you trust.",
            changed.join("\n")
        );
        let ask = app.clone();
        let allow = tokio::task::spawn_blocking(move || {
            ask.dialog()
                .message(msg)
                .title("Add to Parzi?")
                .buttons(MessageDialogButtons::OkCancel)
                .blocking_show()
        })
        .await
        .map_err(|e| e.to_string())?;
        if !allow {
            return Err("change cancelled".into());
        }
    }
    let rebill = cfg
        .providers
        .iter()
        .any(|(id, e)| e.billing != old.provider(id).billing);
    cfg.save().map_err(|e| e.to_string())?;
    if rebill {
        parzi_providers::drop_parked();
    }
    state.orch.apply_config(cfg).await;
    crate::remote::sync_soon(&app);
    Ok(())
}

fn command_line(entry: &parzi_core::config::ProviderEntry) -> String {
    let path = entry.binary.trim();
    if entry.args.is_empty() {
        return path.to_string();
    }
    if path.is_empty() {
        return entry.args.join(" ");
    }
    format!("{path} {}", entry.args.join(" "))
}

fn binary_changes(old: &ParziConfig, new: &ParziConfig) -> Vec<String> {
    let mut out: Vec<String> = new
        .providers
        .iter()
        .filter_map(|(id, entry)| {
            let line = command_line(entry);
            if line.is_empty() {
                return None;
            }
            let before = old.providers.get(id).map(command_line).unwrap_or_default();
            (before != line).then(|| format!("{id} → {line}"))
        })
        .collect();
    out.sort();
    out
}

fn registry_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .into()
    })
}

/// Which agents have a saved key. The keys themselves stay in the backend.
#[tauri::command]
pub fn agent_keys() -> Vec<String> {
    parzi_providers::saved_keys()
}

#[tauri::command]
pub fn set_agent_key(provider: String, key: Option<String>) -> Result<(), String> {
    parzi_providers::save_key(&provider, key.as_deref())?;
    parzi_providers::drop_parked();
    Ok(())
}

#[tauri::command]
pub async fn acp_registry() -> Result<Vec<parzi_providers::RegistryAgent>, String> {
    let url = parzi_providers::registry_url().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let body = registry_agent()
            .get(&url)
            .header("User-Agent", "Parzi")
            .call()
            .map_err(|e| e.to_string())?
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        parzi_providers::parse_registry(&body)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn mcp_command_changes(old: &ParziConfig, new: &ParziConfig) -> Vec<String> {
    let mut out = vec![];
    for (name, srv) in &new.mcp.servers {
        if srv.command.trim().is_empty() {
            continue;
        }
        match old.mcp.servers.get(name) {
            Some(prev)
                if prev.command == srv.command
                    && prev.args == srv.args
                    && srv.env.iter().all(|(k, v)| prev.env.get(k) == Some(v)) => {}
            _ => out.push(name.clone()),
        }
    }
    out.sort();
    out
}

#[tauri::command]
pub async fn connector_check(state: State<'_, AppState>, name: String) -> Result<usize, String> {
    state
        .orch
        .mcp()
        .exposed_tools(&name)
        .await
        .map(|tools| tools.len())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn connector_sign_in(state: State<'_, AppState>, name: String) -> Result<usize, String> {
    let cfg = ParziConfig::load().map_err(|e| e.to_string())?;
    let url = cfg
        .mcp
        .servers
        .get(&name)
        .map(|s| s.url.clone())
        .filter(|u| !u.is_empty())
        .ok_or_else(|| format!("{name} is not a hosted connector"))?;
    let challenge = parzi_runtime::mcp_http::last_challenge(&name);
    parzi_runtime::mcp_oauth::sign_in(&name, &url, challenge, |page| {
        crate::files::launch(&crate::files::open_with_default(page.into()))
    })
    .await?;
    state.orch.mcp().stop(&name).await;
    state
        .orch
        .mcp()
        .exposed_tools(&name)
        .await
        .map(|tools| tools.len())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn connector_forget(state: State<'_, AppState>, name: String) -> Result<(), String> {
    parzi_runtime::mcp_oauth::forget(&name);
    parzi_runtime::connector_icon::forget(&name);
    state.orch.mcp().stop(&name).await;
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ToolRow {
    name: String,
    description: String,
    read_only: bool,
    destructive: bool,
    mode: String,
}

#[derive(serde::Serialize)]
pub struct ConnectorDetails {
    title: String,
    icon: Option<String>,
    tools: Vec<ToolRow>,
}

#[tauri::command]
pub async fn connector_details(
    state: State<'_, AppState>,
    name: String,
) -> Result<ConnectorDetails, String> {
    let d = state
        .orch
        .mcp()
        .details(&name)
        .await
        .map_err(|e| e.to_string())?;
    let cfg = ParziConfig::load().map_err(|e| e.to_string())?;
    let srv = cfg.mcp.servers.get(&name).cloned().unwrap_or_default();
    let (icons, website, url, who) = (
        d.icons.clone(),
        d.website.clone(),
        srv.url.clone(),
        name.clone(),
    );
    let icon = tauri::async_runtime::spawn_blocking(move || {
        parzi_runtime::connector_icon::resolve(&who, &icons, &website, &url)
    })
    .await
    .unwrap_or(None);
    let tools = d
        .tools
        .into_iter()
        .map(|t| ToolRow {
            mode: if srv.deny.iter().any(|x| x == &t.name) {
                "off".into()
            } else {
                srv.tool_mode(&t.name).unwrap_or_default()
            },
            name: t.name,
            description: t.description,
            read_only: t.read_only,
            destructive: t.destructive,
        })
        .collect();
    Ok(ConnectorDetails {
        title: d.title,
        icon,
        tools,
    })
}

#[tauri::command]
pub async fn connectors_found() -> Vec<parzi_core::connectors::Found> {
    tauri::async_runtime::spawn_blocking(parzi_core::connectors::found_on_this_computer)
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub fn connectors_from_text(text: String) -> Vec<(String, parzi_core::config::McpServerCfg)> {
    parzi_core::connectors::from_pasted(&text)
}

#[tauri::command]
pub fn shell_jobs(
    state: State<'_, AppState>,
    session: String,
) -> Vec<parzi_runtime::shell::ShellJob> {
    state.orch.shell_jobs(&session)
}

#[tauri::command]
pub fn shell_hosts(
    state: State<'_, AppState>,
    session: String,
) -> Vec<parzi_runtime::shell::HostJob> {
    state.orch.shell_hosts(&session)
}

#[tauri::command]
pub fn shell_kill(state: State<'_, AppState>, session: String, id: String) -> bool {
    state.orch.shell_kill(&session, &id)
}

#[tauri::command]
pub async fn context_parts(
    state: State<'_, AppState>,
    cwd: String,
) -> Result<parzi_runtime::orchestrator::ContextParts, String> {
    let orch = state.orch.clone();
    tauri::async_runtime::spawn_blocking(move || orch.context_parts(&cwd))
        .await
        .map_err(|e| e.to_string())
}

pub fn in_roster_order(mut all: Vec<ProviderStatus>) -> Vec<ProviderStatus> {
    all.sort_by_key(|s| {
        parzi_providers::PROVIDERS
            .iter()
            .position(|p| *p == s.provider)
            .unwrap_or(usize::MAX)
    });
    all
}

#[tauri::command]
pub async fn provider_statuses(state: State<'_, AppState>) -> Result<Vec<ProviderStatus>, String> {
    Ok(in_roster_order(state.orch.provider_statuses()))
}

/// Reads each signed-in agent's plan limits from its own source (Claude
/// Code's `/usage`, Codex's `/status`), without a full status check.
#[tauri::command]
pub async fn refresh_usage(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<ProviderStatus>, String> {
    let all = in_roster_order(state.orch.refresh_usage().await);
    let _ = app.emit("parzi://providers", &all);
    Ok(all)
}

#[tauri::command]
pub async fn refresh_providers(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Option<Vec<String>>,
) -> Result<Vec<ProviderStatus>, String> {
    let all = in_roster_order(state.orch.refresh_providers(&ids.unwrap_or_default()).await);
    let _ = app.emit("parzi://providers", &all);
    Ok(all)
}

#[tauri::command]
pub async fn warm_agent(state: State<'_, AppState>, provider: String) -> Result<(), String> {
    parzi_providers::warm(&provider, &state.orch.config());
    Ok(())
}

/// Settings for web pages, kept in `browser.json` under the keys they always
/// had. Older files also hold remembered site permissions; those are ignored.
const PAGE_PREFS_FILE: &str = "browser.json";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PagePrefs {
    pub devtools: bool,
    pub agent_sign_ins: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagePrefsView {
    devtools: bool,
    agent_sign_ins: bool,
    windows: bool,
}

static PAGE_PREFS: RwLock<Option<PagePrefs>> = RwLock::new(None);

fn page_prefs_path() -> Option<PathBuf> {
    parzi_core::paths::parzi_dir()
        .ok()
        .map(|d| d.join(PAGE_PREFS_FILE))
}

fn load_page_prefs() -> PagePrefs {
    page_prefs_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn page_prefs() -> PagePrefs {
    if let Some(p) = PAGE_PREFS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        return p.clone();
    }
    let mut slot = PAGE_PREFS.write().unwrap_or_else(PoisonError::into_inner);
    slot.get_or_insert_with(load_page_prefs).clone()
}

pub fn devtools() -> bool {
    page_prefs().devtools
}

pub fn agent_sign_ins() -> bool {
    page_prefs().agent_sign_ins
}

fn save_page_prefs() -> Result<(), String> {
    static SAVE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = SAVE.lock().unwrap_or_else(PoisonError::into_inner);
    let path = page_prefs_path().ok_or("no Parzi folder")?;
    let text = serde_json::to_vec_pretty(&page_prefs()).map_err(|e| e.to_string())?;
    parzi_core::error::atomic_write(&path, &text).map_err(|e| e.to_string())
}

fn page_prefs_view(p: &PagePrefs) -> PagePrefsView {
    PagePrefsView {
        devtools: p.devtools,
        agent_sign_ins: p.agent_sign_ins,
        windows: cfg!(windows),
    }
}

#[tauri::command]
pub fn browser_prefs() -> PagePrefsView {
    page_prefs_view(&page_prefs())
}

#[tauri::command]
pub async fn browser_set_pref(
    app: AppHandle,
    name: String,
    on: bool,
) -> Result<PagePrefsView, String> {
    let prefs = {
        let mut slot = PAGE_PREFS.write().unwrap_or_else(PoisonError::into_inner);
        let prefs = slot.get_or_insert_with(load_page_prefs);
        match name.as_str() {
            "devtools" => prefs.devtools = on,
            "agentSignIns" => prefs.agent_sign_ins = on,
            _ => return Err(format!("unknown browser setting {name}")),
        }
        prefs.clone()
    };
    tauri::async_runtime::spawn_blocking(save_page_prefs)
        .await
        .map_err(|e| e.to_string())??;
    if name == "devtools" {
        crate::browser::apply_devtools(&app, on);
    }
    Ok(page_prefs_view(&prefs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_prefs_keep_their_keys_and_default_off() {
        let p: PagePrefs = serde_json::from_str("{}").unwrap();
        assert!(!p.devtools && !p.agent_sign_ins);
        let old =
            r#"{"devtools":true,"agent_sign_ins":true,"sites":{"https://a.com":{"camera":true}}}"#;
        let p: PagePrefs = serde_json::from_str(old).unwrap();
        assert!(
            p.devtools && p.agent_sign_ins,
            "a file from before keeps its values"
        );
        let back: PagePrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
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
    fn an_acp_argument_change_is_confirmed_too() {
        let mut plain = ParziConfig::default();
        plain.providers.insert(
            "acp".into(),
            parzi_core::config::ProviderEntry {
                binary: "gemini".into(),
                ..Default::default()
            },
        );
        let mut with_args = plain.clone();
        with_args.providers.get_mut("acp").unwrap().args = vec!["--acp".into()];
        assert_eq!(
            binary_changes(&plain, &with_args),
            vec!["acp → gemini --acp".to_string()]
        );
        assert!(
            binary_changes(&with_args, &with_args).is_empty(),
            "an identical command is silent"
        );
        let mut flagged = ParziConfig::default();
        flagged.providers.insert(
            "claude".into(),
            parzi_core::config::ProviderEntry {
                binary: r"C:\tools\claude.exe".into(),
                args: vec!["--flag".into()],
                ..Default::default()
            },
        );
        let mut cleared = flagged.clone();
        cleared.providers.get_mut("claude").unwrap().args.clear();
        assert_eq!(
            binary_changes(&flagged, &cleared),
            vec![r"claude → C:\tools\claude.exe".to_string()],
            "empty args keep the path-only line"
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
        assert!(mcp_command_changes(&new, &new).is_empty());
        assert!(mcp_command_changes(&new, &old).is_empty());
        let mut hosted = ParziConfig::default();
        hosted.mcp.servers.insert(
            "notion".into(),
            parzi_core::config::McpServerCfg {
                url: "https://mcp.notion.com/mcp".into(),
                ..Default::default()
            },
        );
        assert!(mcp_command_changes(&old, &hosted).is_empty());
    }

    #[test]
    fn env_changes_count_as_command_changes() {
        let with_env = |pairs: &[(&str, &str)]| {
            let mut cfg = ParziConfig::default();
            cfg.mcp.servers.insert(
                "s".into(),
                parzi_core::config::McpServerCfg {
                    command: "npx".into(),
                    args: vec!["-y".into()],
                    env: pairs
                        .iter()
                        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                        .collect(),
                    ..Default::default()
                },
            );
            cfg
        };
        let base = with_env(&[("TOKEN", "a")]);
        assert!(mcp_command_changes(&base, &base).is_empty(), "same env");
        assert_eq!(
            mcp_command_changes(&base, &with_env(&[("TOKEN", "b")])),
            vec!["s".to_string()],
            "changed value"
        );
        assert_eq!(
            mcp_command_changes(
                &base,
                &with_env(&[("TOKEN", "a"), ("NODE_OPTIONS", "-r x")])
            ),
            vec!["s".to_string()],
            "added key"
        );
        assert!(
            mcp_command_changes(&base, &with_env(&[])).is_empty(),
            "a removed key does not prompt"
        );
    }
}
