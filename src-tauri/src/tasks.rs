//! The desk's side of the shared task list: the Tasks view, the side panel
//! and Home read and change it here, and Assign starts an agent on a task
//! with the permissions the user picked and someone to ask (the desk).

use std::sync::Arc;

use parzi_core::store::SessionStatus;
use parzi_core::tasks::{self, NewTask, TaskPatch, TaskStatus, TaskStore, TaskView};
use parzi_runtime::tools::{ApprovalMode, Approver};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::{AppState, GuiApprover};

/// Every change (the user's, an agent's, a run's end) reaches the UI.
pub(crate) fn watch(app: &AppHandle) {
    let app = app.clone();
    tasks::watch(move |change| {
        let _ = app.emit("parzi://tasks", change);
    });
}

fn open() -> Result<TaskStore, String> {
    TaskStore::open().map_err(|e| e.to_string())
}

fn project_arg(project: Option<String>) -> Option<String> {
    project
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
}

fn view_of(ts: &TaskStore, id: &str) -> Result<TaskView, String> {
    let task = ts.get(id).map_err(|e| e.to_string())?;
    let scope = ts
        .scope(task.project.as_deref())
        .map_err(|e| e.to_string())?;
    tasks::views(scope)
        .into_iter()
        .find(|v| v.task.id == task.id)
        .ok_or_else(|| format!("no task `{id}`"))
}

/// Every task (`all`), or one project's; no project is the Inbox.
#[tauri::command]
pub async fn tasks_list(
    all: Option<bool>,
    project: Option<String>,
) -> Result<Vec<TaskView>, String> {
    let ts = open()?;
    let list = if all.unwrap_or(false) {
        ts.all()
    } else {
        ts.scope(project_arg(project).as_deref())
    };
    Ok(tasks::views(list.map_err(|e| e.to_string())?))
}

#[tauri::command]
pub async fn tasks_create(
    title: String,
    project: Option<String>,
    notes: Option<String>,
    needs: Option<Vec<String>>,
    lane: Option<String>,
) -> Result<TaskView, String> {
    let ts = open()?;
    let draft = NewTask {
        title,
        notes: notes.unwrap_or_default(),
        needs: needs.unwrap_or_default(),
        lane: lane.unwrap_or_default(),
        ..NewTask::default()
    };
    let added = ts
        .add(project_arg(project).as_deref(), &[draft], "you")
        .map_err(|e| e.to_string())?;
    view_of(&ts, &added[0].id)
}

#[tauri::command]
pub async fn tasks_update(id: String, patch: TaskPatch) -> Result<TaskView, String> {
    let ts = open()?;
    let task = ts.update(&id, &patch).map_err(|e| e.to_string())?;
    view_of(&ts, &task.id)
}

#[tauri::command]
pub async fn tasks_delete(id: String) -> Result<(), String> {
    open()?.delete(&id).map(drop).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tasks_reorder(ids: Vec<String>) -> Result<(), String> {
    open()?.reorder(&ids).map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct Assigned {
    session: String,
    /// What the permissions mean for this run, when it will not just work.
    warning: Option<String>,
}

/// The project's folder on this machine, when the brain knows it.
fn folder_of(project: Option<&str>) -> String {
    let Some(slug) = project else {
        return String::new();
    };
    parzi_core::brain::projects()
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.slug.eq_ignore_ascii_case(slug))
        .map(|p| p.folder)
        .unwrap_or_default()
}

/// Starts an agent on a task in the background: a new session, linked to
/// the task (doing) before it runs, asking the desk for anything the chosen
/// mode does not allow on its own.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn tasks_assign(
    state: State<'_, AppState>,
    id: String,
    model: String,
    effort: Option<String>,
    mode: Option<String>,
    cwd: Option<String>,
    lane: Option<String>,
) -> Result<Assigned, String> {
    let ts = open()?;
    let task = ts.get(&id).map_err(|e| e.to_string())?;
    if task.status == TaskStatus::Done {
        return Err("This task is done. Move it back to To do to assign it again.".into());
    }
    if model.trim().is_empty() {
        return Err("Pick a model first".into());
    }
    let lane = match lane
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(|| task.lane.clone())
        .as_str()
    {
        "work" => "work",
        _ => "build",
    };
    let cwd = cwd
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| folder_of(task.project.as_deref()));
    let project = match &task.project {
        Some(p) => p.clone(),
        None => crate::sessions::project_for(&cwd).await,
    };
    let title: String = task.title.chars().take(80).collect();
    let orch = &state.orch;
    let meta = orch
        .store()
        .create(&title, &project, lane, &model)
        .map_err(|e| e.to_string())?;
    if !cwd.is_empty() {
        orch.store()
            .set_cwd(&meta.id, &cwd)
            .map_err(|e| e.to_string())?;
    }
    // Linked before it can start, so its run end always finds the task.
    let linked = parzi_runtime::task_tools::link(&task.id, &meta.id).map_err(|e| e.to_string())?;
    let scope = ts.scope(linked.project.as_deref()).unwrap_or_default();
    let prompt = parzi_runtime::task_tools::brief(&linked, &scope);
    let approver: Arc<dyn Approver> = Arc::new(GuiApprover {
        app: state.app.clone(),
        pending: state.pending.clone(),
    });
    let effort =
        parzi_runtime::orchestrator::normalize_effort(effort.as_deref().unwrap_or("medium"));
    let sent = orch
        .send_to(
            &meta.id,
            &prompt,
            Some(approver),
            &cwd,
            &effort,
            vec![],
            Some(model),
            mode.clone(),
        )
        .await;
    if let Err(e) = sent {
        // Nothing ran: the task goes back as it was and the empty session goes.
        let _ = ts.update(
            &task.id,
            &TaskPatch {
                status: Some(task.status),
                session: Some(task.session.clone().unwrap_or_default()),
                ..TaskPatch::default()
            },
        );
        let _ = orch.store().set_status(&meta.id, SessionStatus::Idle);
        let _ = orch.store().delete_thread(&meta.id);
        return Err(e.to_string());
    }
    let policy = orch.config().lanes.default_mode;
    Ok(Assigned {
        session: meta.id,
        warning: permission_note(&policy, mode.as_deref(), lane),
    })
}

/// What the Settings policy and the chosen mode leave this agent able to do,
/// when that is less than "works on its own".
fn permission_note(policy: &str, mode: Option<&str>, lane: &str) -> Option<String> {
    let floor = ApprovalMode::parse(policy);
    let choice = mode.unwrap_or("").trim();
    if floor == ApprovalMode::Deny {
        return Some(
            "Lockdown is on in Settings, so this agent can read but not change anything.".into(),
        );
    }
    if choice == "deny" {
        return Some(
            "Read only: this agent can look around but cannot edit files or run commands.".into(),
        );
    }
    if choice == "full" {
        return None;
    }
    let chosen = ApprovalMode::parse(choice);
    if floor == ApprovalMode::Auto && chosen == ApprovalMode::Auto {
        return None;
    }
    let what = if lane == "work" {
        "before it changes anything"
    } else if choice == "edits" {
        "before anything but file edits"
    } else {
        "before it edits files or runs commands"
    };
    Some(format!(
        "It will ask {what}. Approvals show up in its session, so keep an eye out."
    ))
}

#[cfg(test)]
mod tests {
    use super::permission_note;

    #[test]
    fn the_note_says_what_the_agent_cannot_do_alone() {
        assert!(permission_note("deny", Some("full"), "build")
            .unwrap()
            .contains("Lockdown"));
        assert!(permission_note("ask", Some("deny"), "build")
            .unwrap()
            .contains("Read only"));
        assert_eq!(permission_note("ask", Some("full"), "build"), None);
        assert_eq!(permission_note("auto", Some("auto"), "build"), None);
        assert!(permission_note("ask", Some("auto"), "build")
            .unwrap()
            .contains("runs commands"));
        assert!(permission_note("auto", Some("edits"), "build")
            .unwrap()
            .contains("but file edits"));
        assert!(permission_note("ask", Some("supervised"), "work")
            .unwrap()
            .contains("changes anything"));
    }
}
