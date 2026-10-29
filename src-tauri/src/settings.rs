use std::sync::OnceLock;
use std::time::Duration;

use parzi_core::config::ParziConfig;
use parzi_providers::ProviderStatus;
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
    // Each new or changed program, as the line it runs (never its env:
    // that holds tokens).
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
    cfg.save().map_err(|e| e.to_string())?;
    state.orch.apply_config(cfg).await;
    crate::remote::sync_soon(&app);
    Ok(())
}

/// The line a person confirms. Empty args keep `{id} → {path}`.
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

/// The public ACP registry, as commands this computer can run. Parzi does
/// not download or install those programs.
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
        // A hosted connector runs nothing here: nothing to confirm.
        if srv.command.trim().is_empty() {
            continue;
        }
        match old.mcp.servers.get(name) {
            // An added or changed env var (PATH, NODE_OPTIONS, LD_PRELOAD...)
            // can swap the program as surely as a new command line.
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

/// Starts a connector and lists its tools: how many it offers, or why it
/// can't start. A connector that signs in opens the browser here, the
/// first time.
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

/// Signs in to a hosted connector the way its server asks (OAuth): the
/// sign-in page opens in the person's browser, Parzi keeps the token, and
/// the connector reconnects with it. How many tools it offers then.
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
    // Reconnect with the token.
    state.orch.mcp().stop(&name).await;
    state
        .orch
        .mcp()
        .exposed_tools(&name)
        .await
        .map(|tools| tools.len())
        .map_err(|e| e.to_string())
}

/// Forgets what Parzi kept for a removed connector: its sign-in and logo.
#[tauri::command]
pub async fn connector_forget(state: State<'_, AppState>, name: String) -> Result<(), String> {
    parzi_runtime::mcp_oauth::forget(&name);
    parzi_runtime::connector_icon::forget(&name);
    state.orch.mcp().stop(&name).await;
    Ok(())
}

/// One of a connector's tools, as Settings lists it.
#[derive(serde::Serialize)]
pub struct ToolRow {
    name: String,
    description: String,
    read_only: bool,
    destructive: bool,
    /// "auto" (runs without asking), "ask", "off", or "" (the approval
    /// mode decides).
    mode: String,
}

#[derive(serde::Serialize)]
pub struct ConnectorDetails {
    title: String,
    /// Its logo as an image (a data URL), when it has one.
    icon: Option<String>,
    tools: Vec<ToolRow>,
}

/// A connector as it describes itself (connecting if needed): its title,
/// its logo, and every tool with how it runs here.
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

/// Sets a connector's logo from a picture the person picked.
#[tauri::command]
pub async fn connector_set_icon(name: String, path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        parzi_runtime::connector_icon::set_picked(&name, std::path::Path::new(&path))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Connectors set up in the other apps on this computer (Claude Code,
/// Claude Desktop, Cursor, VS Code, Codex, Grok), ready to add.
#[tauri::command]
pub async fn connectors_found() -> Vec<parzi_core::connectors::Found> {
    tauri::async_runtime::spawn_blocking(parzi_core::connectors::found_on_this_computer)
        .await
        .unwrap_or_default()
}

/// What a pasted text adds: an `mcpServers` block, an address, or nothing.
#[tauri::command]
pub fn connectors_from_text(text: String) -> Vec<(String, parzi_core::config::McpServerCfg)> {
    parzi_core::connectors::from_pasted(&text)
}

/// The commands a session's agents ran (background and recent foreground
/// ones), for the session column.
#[tauri::command]
pub fn shell_jobs(
    state: State<'_, AppState>,
    session: String,
) -> Vec<parzi_runtime::shell::ShellJob> {
    state.orch.shell_jobs(&session)
}

/// Stops one of a session's background commands.
#[tauri::command]
pub fn shell_kill(state: State<'_, AppState>, session: String, id: String) -> bool {
    state.orch.shell_kill(&session, &id)
}

/// What Parzi adds to a session's context (instructions, notes, tools),
/// for the session column's breakdown.
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

#[tauri::command]
pub async fn run_doctor_quick(
    state: State<'_, AppState>,
) -> Result<Vec<parzi_runtime::doctor::Check>, String> {
    let cfg = state.orch.config();
    Ok(parzi_runtime::doctor::Doctor::new(cfg).run_quick().await)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // A hosted one runs nothing on this computer: nothing to confirm.
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
