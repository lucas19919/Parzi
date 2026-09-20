//! Managed-context IPC: the workspace set plus the deck project's set.
//!
//! Same S-1 rule as `hub_cmds`: every command here is a forward. Core owns
//! the manifest, tiers and caps; curation lives in the dock; injection
//! lives in the runtime handler.

#[derive(serde::Serialize)]
pub struct ContextItem {
    name: String,
    tier: parzi_core::context_store::Tier,
    scope: parzi_core::context_store::Scope,
    source: String,
    bytes: u64,
}

fn context_home() -> Result<std::path::PathBuf, String> {
    parzi_core::paths::parzi_dir().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_context(
    workspace: String,
    slug: Option<String>,
) -> Result<Vec<ContextItem>, String> {
    let home = context_home()?;
    parzi_core::context_store::list(&home, workspace.trim(), slug.as_deref())
        .map(|docs| {
            docs.into_iter()
                .map(|d| {
                    let bytes = d.content.len() as u64;
                    ContextItem {
                        name: d.name,
                        tier: d.tier,
                        scope: d.scope,
                        source: String::new(),
                        bytes,
                    }
                })
                .collect()
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn read_context_file(
    workspace: String,
    slug: Option<String>,
    name: String,
) -> Result<String, String> {
    let home = context_home()?;
    parzi_core::context_store::list(&home, workspace.trim(), slug.as_deref())
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|d| d.name == name)
        .map(|d| d.content)
        .ok_or_else(|| "unknown context file".to_string())
}

#[tauri::command]
pub async fn add_context(
    workspace: String,
    slug: Option<String>,
    title: String,
    content: String,
    tier: String,
    source: String,
) -> Result<String, String> {
    let home = context_home()?;
    let tier = match tier.trim() {
        "pinned" => parzi_core::context_store::Tier::Pinned,
        "auto" => parzi_core::context_store::Tier::Auto,
        _ => parzi_core::context_store::Tier::Curated,
    };
    parzi_core::context_store::add(
        &home,
        workspace.trim(),
        slug.as_deref(),
        title.trim(),
        &content,
        tier,
        source.trim(),
    )
    .map(|d| d.name)
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_context_pinned(
    workspace: String,
    slug: Option<String>,
    name: String,
    pinned: bool,
) -> Result<(), String> {
    let home = context_home()?;
    parzi_core::context_store::set_pinned(&home, workspace.trim(), slug.as_deref(), &name, pinned)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_context(
    workspace: String,
    slug: Option<String>,
    name: String,
) -> Result<(), String> {
    let home = context_home()?;
    parzi_core::context_store::remove(&home, workspace.trim(), slug.as_deref(), &name)
        .map_err(|e| e.to_string())
}
