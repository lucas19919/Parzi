//! Hub IPC: workspaces, GitHub, projects (PLAN.md §1, §2, §8).
//!
//! S-1 rule of the shell: every command here is a forward. Core owns the
//! files and the grammar, the runtime owns the sessions and the plan flow,
//! and this module only moves arguments across the IPC boundary.

use std::path::Path;
use std::sync::Arc;

use parzi_core::workspace::{self, RepoRef, Workspace};
use parzi_providers::github;
use parzi_runtime::tools::Approver;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::{AppState, GuiApprover};

mod projects;
// Glob, not a named list: the hidden items `#[tauri::command]` generates have
// to travel with the functions — `generate_handler!` looks up both.
pub use projects::*;

/// The same approval card path `send_message` uses, so a `critical` lease
/// transfer shows up exactly like a tool approval (PLAN.md §4).
fn approver(state: &State<'_, AppState>) -> Arc<dyn Approver> {
    Arc::new(GuiApprover {
        app: state.app.clone(),
        pending: state.pending.clone(),
    })
}

/* ---------- workspaces ---------- */

#[tauri::command]
pub async fn workspace_list() -> Result<Vec<String>, String> {
    Ok(workspace::list())
}

#[tauri::command]
pub async fn workspace_get(name: String) -> Result<Workspace, String> {
    workspace::load(&name).map_err(|e| e.to_string())
}

/// Step 1 of the wizard: an empty workspace, `Solo` or `Team`, owned by
/// whoever is at the machine (identity is a name on this hub, §7).
#[tauri::command]
pub async fn workspace_create(name: String, kind: workspace::Kind) -> Result<Workspace, String> {
    workspace::create_for(&name, kind, &current_user()).map_err(|e| e.to_string())
}

/// Delete a hub workspace: its deck projects' role sessions and its own
/// chats are killed first and removed with it, then the workspace tree.
/// `default` is the inbox, not a hub workspace, and is refused.
#[tauri::command]
pub async fn workspace_delete(
    state: State<'_, AppState>,
    workspace: String,
) -> Result<usize, String> {
    let target = workspace.trim().to_string();
    if target.is_empty() || target == "default" {
        return Err("the Inbox can't be deleted".into());
    }
    // Role sessions file under their deck slug, chats under the workspace
    // name: collect both keys before touching disk.
    let mut keys = vec![target.clone()];
    keys.extend(parzi_core::project::list(&target));
    let ids: Vec<String> = state
        .orch
        .store()
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|m| keys.iter().any(|k| k == &m.project))
        .map(|m| m.id)
        .collect();
    for id in &ids {
        let _ = state.orch.kill(id).await;
    }
    let mut n = 0;
    for key in &keys {
        n += state
            .orch
            .store()
            .delete_project_threads(key)
            .map_err(|e| e.to_string())?;
    }
    parzi_core::workspace::remove(&target).map_err(|e| e.to_string())?;
    Ok(n)
}

/// Unify: move a legacy `~/.parzi/projects/<name>` into a hub workspace of
/// the same name. The chat key stays `name`, so existing sessions keep
/// working — only the directory moves.
#[tauri::command]
pub async fn workspace_migrate(name: String) -> Result<Workspace, String> {
    workspace::import_legacy(&name).map_err(|e| e.to_string())
}

/// The local account name — the only identity round 1 has.
fn current_user() -> String {
    std::env::var("PARZI_USER")
        .or_else(|_| std::env::var("USERNAME"))
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "me".to_string())
}

/// Last step of the wizard: the picked repos, deduped by name.
#[tauri::command]
pub async fn workspace_add_repos(
    workspace: String,
    repos: Vec<RepoRef>,
) -> Result<Workspace, String> {
    workspace::add_repos(&workspace, repos).map_err(|e| e.to_string())
}

/// The round-1 syncer: the workspace's own git repo (§1.1). No hub yet, so
/// "sync" is commit + pull + push; a workspace without a remote just commits,
/// and a PLAN.md/PROJECT.md conflict comes back in `conflicts` instead of
/// being merged behind the team's back.
#[tauri::command]
pub async fn workspace_sync(
    workspace: String,
) -> Result<parzi_core::workspace::sync::SyncReport, String> {
    parzi_runtime::sync_timer::sync_workspace(&workspace)
        .await
        .map_err(|e| e.to_string())
}

/* ---------- github ---------- */

#[tauri::command]
pub async fn github_connect(token: Option<String>) -> Result<github::Status, String> {
    github::connect(token).await
}

#[tauri::command]
pub async fn github_status() -> Result<github::Status, String> {
    Ok(github::status().await)
}

#[tauri::command]
pub async fn github_list_orgs() -> Result<Vec<github::Org>, String> {
    github::orgs().await
}

/// `org` empty = the signed-in user's own repos.
#[tauri::command]
pub async fn github_list_repos(org: String) -> Result<Vec<github::Repo>, String> {
    github::repos(&org).await
}

/// Progress of one clone, pushed on `parzi://repo-clone` so the wizard never
/// blocks on the network.
#[derive(Debug, Clone, Serialize)]
pub struct CloneEvent {
    workspace: String,
    repo: String,
    /// `cloning` | `done` | `error`
    phase: String,
    line: String,
    path: String,
}

/// Map an existing checkout, or clone the repo in the background. Returns the
/// destination path at once; `parzi://repo-clone` carries the rest. The
/// decision, the paths and the recording are `github`'s; this end owns only
/// the event and the task.
#[tauri::command]
pub async fn repo_clone_or_map(
    app: AppHandle,
    workspace: String,
    repo: RepoRef,
    local_path: Option<String>,
    dest_root: Option<String>,
) -> Result<String, String> {
    let name = repo.name.clone();
    match github::prepare(
        &workspace,
        &repo,
        local_path.as_deref(),
        dest_root.as_deref(),
    )
    .await?
    {
        github::Prepared::Mapped(path) => {
            emit_clone(&app, &workspace, &name, "done", "", &path);
            Ok(path.display().to_string())
        }
        github::Prepared::Clone(dest) => {
            let shown = dest.display().to_string();
            tauri::async_runtime::spawn(async move {
                let (app2, ws2, n2, d2) = (app.clone(), workspace.clone(), name.clone(), dest.clone());
                let res = github::clone_and_map(&ws2, &repo, &d2, |line| {
                    emit_clone(&app2, &ws2, &n2, "cloning", &line, &d2);
                })
                .await;
                match res {
                    Ok(p) => emit_clone(&app, &workspace, &name, "done", "", &p),
                    Err(e) => emit_clone(&app, &workspace, &name, "error", &e, &dest),
                }
            });
            Ok(shown)
        }
    }
}

fn emit_clone(app: &AppHandle, workspace: &str, repo: &str, phase: &str, line: &str, path: &Path) {
    let ev = CloneEvent {
        workspace: workspace.to_string(),
        repo: repo.to_string(),
        phase: phase.to_string(),
        line: line.to_string(),
        path: path.display().to_string(),
    };
    if let Err(e) = app.emit("parzi://repo-clone", ev) {
        eprintln!("repo-clone emit: {e}");
    }
}

/* ---------- screenshots ---------- */

/// Verifier aid: `PARZI_UI_STATE=new-workspace:repos` drops the app straight
/// into one wizard step so each screen can be captured without input. Empty
/// unless the env var is set, so it costs nothing in normal use.
#[tauri::command]
pub fn ui_state() -> String {
    std::env::var("PARZI_UI_STATE").unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::current_user;

    #[test]
    fn the_user_is_the_account_name_and_never_empty() {
        std::env::set_var("PARZI_USER", "ada");
        assert_eq!(current_user(), "ada");
        std::env::remove_var("PARZI_USER");
        assert!(!current_user().trim().is_empty());
    }
}
