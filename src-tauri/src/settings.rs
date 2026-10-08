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
    state.orch.apply_config(cfg).await;
    Ok(())
}

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

#[tauri::command]
pub async fn toggle_favorite(spec: String) -> Result<Vec<String>, String> {
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

#[derive(serde::Serialize)]
pub struct ServeStatus {
    pub running: bool,
    pub port: Option<u16>,
}

/// Local `parzi serve` state for onboarding: reads serve.json and proves
/// the socket answers. Never starts anything.
#[tauri::command]
pub async fn serve_status() -> Result<ServeStatus, String> {
    let root = parzi_core::paths::parzi_dir().map_err(|e| e.to_string())?;
    let raw = std::fs::read_to_string(root.join("serve.json")).map_err(|_| "no serve.json".to_string());
    let raw = match raw {
        Ok(r) => r,
        Err(_) => {
            return Ok(ServeStatus {
                running: false,
                port: None,
            })
        }
    };
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|_| "serve.json unreadable".to_string())?;
    let port = v.get("port").and_then(|p| p.as_u64()).unwrap_or(0) as u16;
    let token = v.get("token").and_then(|t| t.as_str()).unwrap_or("").to_string();
    if port == 0 || token.is_empty() {
        return Ok(ServeStatus {
            running: false,
            port: None,
        });
    }
    let ok = parzi_runtime::desk::call_serve(
        "health",
        serde_json::json!({}),
        std::time::Duration::from_secs(3),
    )
    .await
    .is_ok();
    Ok(ServeStatus {
        running: ok,
        port: ok.then_some(port),
    })
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
    }
}
