//! Project IPC: create, open, draft, audit, approve, read (PLAN.md §2, §8).
//!
//! Same rule as the parent module — every command is a forward. The shapes
//! below exist only because the runtime types they carry are not themselves
//! serialisable; they add no decision of their own.

use parzi_core::journal::JournalLine;
use parzi_core::plan::Plan;
use parzi_core::project::{self, Project};
use parzi_runtime::project_flow;
use serde::Serialize;
use tauri::State;

use super::{approver, current_user};
use crate::AppState;

/// How much of the journal the Activity tab asks for. One screen of history
/// is plenty; the file stays the record.
const JOURNAL_TAIL: usize = 400;

/// Step 2 of the flow: a project in `Drafting`, ready for the header agent.
/// The wizard supplies what a person chose; everything else — why, what,
/// constraints — the header drafts with them, so it starts empty.
#[tauri::command]
pub async fn project_create(
    workspace: String,
    title: String,
    repos: Vec<String>,
    roster: project::Roster,
    budget_usd: Option<f64>,
) -> Result<Project, String> {
    project::create(&workspace, &title, repos, roster, budget_usd).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn project_get(workspace: String, slug: String) -> Result<Project, String> {
    project::load(&workspace, &slug).map_err(|e| e.to_string())
}

/// Persist a project the Settings tab edited. Core owns the file; this
/// only carries the struct across the IPC boundary.
#[tauri::command]
pub async fn project_save(project: Project) -> Result<Project, String> {
    project::save(&project).map_err(|e| e.to_string())?;
    project::load(&project.workspace, &project.slug).map_err(|e| e.to_string())
}

/// Every `projects/<slug>/PROJECT.md` of the workspace, in slug order. A
/// listing, not a decision: a project whose grammar does not parse is skipped
/// rather than failing the sidebar.
#[tauri::command]
pub async fn project_list(workspace: String) -> Result<Vec<Project>, String> {
    Ok(project::list(&workspace)
        .iter()
        .filter_map(|slug| project::load(&workspace, slug).ok())
        .collect())
}

/// The project and the sessions the deck talks to. `project_flow::Opened`
/// crosses the IPC boundary as this; the orchestrator id comes from
/// `sessions.toml`, which is empty until the first audit.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectOpen {
    project: Project,
    header_session: String,
    orchestrator_session: Option<String>,
    /// The deterministic STATUS.md as of this open (§15.7).
    status: String,
}

#[tauri::command]
pub async fn project_open(
    state: State<'_, AppState>,
    workspace: String,
    slug: String,
) -> Result<ProjectOpen, String> {
    let opened = project_flow::open(&state.orch, &workspace, &slug)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ProjectOpen {
        orchestrator_session: project_flow::sessions(&workspace, &slug).orchestrator,
        project: opened.project,
        header_session: opened.header_session,
        status: opened.status,
    })
}

/// One rough plan from `drafts/`, with the path the Docs tab opens it by.
#[derive(Debug, Clone, Serialize)]
pub struct DraftView {
    name: String,
    title: String,
    content: String,
    path: String,
}

#[tauri::command]
pub async fn project_drafts(workspace: String, slug: String) -> Result<Vec<DraftView>, String> {
    let dir = project::dir(&workspace, &slug).join("drafts");
    let drafts = project_flow::drafts(&workspace, &slug).map_err(|e| e.to_string())?;
    Ok(drafts
        .into_iter()
        .map(|d| DraftView {
            path: dir.join(&d.name).display().to_string(),
            name: d.name,
            title: d.title,
            content: d.body,
        })
        .collect())
}

/// What an audit turn hands back to the deck: the orchestrator's summary and
/// the plan it wrote. PLAN.md itself is written inside the run, by core.
#[derive(Debug, Clone, Serialize)]
pub struct AuditResult {
    summary: String,
    plan: Plan,
}

/// `draft` empty = the newest one.
#[tauri::command]
pub async fn project_audit(
    state: State<'_, AppState>,
    workspace: String,
    slug: String,
    draft: Option<String>,
) -> Result<AuditResult, String> {
    let name = draft
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());
    let audited = project_flow::audit(
        &state.orch,
        &workspace,
        &slug,
        name.as_deref(),
        Some(approver(&state)),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(AuditResult {
        summary: audited.summary,
        plan: audited.plan,
    })
}

/// The human's move (§4): the plan is good, dispatch sprint 1. The lanes it
/// started are on the board a moment later; the deck re-reads the project.
#[tauri::command]
pub async fn project_approve(
    state: State<'_, AppState>,
    workspace: String,
    slug: String,
) -> Result<Project, String> {
    project_flow::approve(
        &state.orch,
        &workspace,
        &slug,
        &current_user(),
        Some(approver(&state)),
    )
    .await
    .map_err(|e| e.to_string())?;
    project::load(&workspace, &slug).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn project_status(workspace: String, slug: String) -> Result<String, String> {
    project_flow::status(&workspace, &slug).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn project_plan(workspace: String, slug: String) -> Result<Plan, String> {
    project_flow::plan(&workspace, &slug).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn project_journal(workspace: String, slug: String) -> Result<Vec<JournalLine>, String> {
    Ok(project_flow::journal(&workspace, &slug, JOURNAL_TAIL))
}
