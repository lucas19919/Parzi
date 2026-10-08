use std::sync::Arc;

use parzi_core::store::{Event, SessionMeta};
use parzi_runtime::tools::{Approval, Approver};
use tauri::State;

use crate::{AppState, GuiApprover};

#[tauri::command]
pub async fn list_threads(state: State<'_, AppState>) -> Result<Vec<SessionMeta>, String> {
    state.orch.store().list().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_thread(
    state: State<'_, AppState>,
    id: String,
) -> Result<(SessionMeta, Vec<Event>), String> {
    let meta = state.orch.store().get(&id).map_err(|e| e.to_string())?;
    let events = state.orch.store().events(&id).map_err(|e| e.to_string())?;
    Ok((meta, events))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn send_message(
    state: State<'_, AppState>,
    session_id: Option<String>,
    model: String,
    prompt: String,
    cwd: String,
    effort: Option<String>,
    attachments: Option<Vec<String>>,
    mode: Option<String>,
    lane: Option<String>,
) -> Result<String, String> {
    let effort = parzi_runtime::orchestrator::normalize_effort(effort.as_deref().unwrap_or("med"));
    let attached = read_attachments(&cwd, &attachments.unwrap_or_default());
    let approver: Arc<dyn Approver> = Arc::new(GuiApprover {
        app: state.app.clone(),
        pending: state.pending.clone(),
    });
    let sid = match session_id {
        Some(id) => {
            state
                .orch
                .send_to(
                    &id,
                    &prompt,
                    Some(approver),
                    &cwd,
                    &effort,
                    attached,
                    Some(model),
                    mode,
                )
                .await
                .map_err(|e| e.to_string())?;
            id
        }
        None => {
            // Threads belong to their folder's project, not shared "default".
            let project =
                parzi_core::brain::project_for_folder(&cwd).unwrap_or_else(|| "default".into());
            state
                .orch
                .spawn(
                    &project,
                    lane.as_deref().unwrap_or(""),
                    &model,
                    &prompt,
                    Some(approver),
                    &cwd,
                    &effort,
                    attached,
                    mode,
                )
                .await
                .map_err(|e| e.to_string())?
                .0
                .id
        }
    };
    Ok(sid)
}

fn read_attachments(cwd: &str, paths: &[String]) -> Vec<parzi_core::context::AttachedFile> {
    let base = if cwd.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        std::path::PathBuf::from(cwd)
    };
    parzi_core::context::read_attachments(&base, paths)
}

#[tauri::command]
pub async fn kill_run(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.orch.kill(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn compact_thread(state: State<'_, AppState>, id: String) -> Result<String, String> {
    state.orch.compact(&id, "").await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fork_thread(state: State<'_, AppState>, id: String) -> Result<SessionMeta, String> {
    state.orch.fork(&id, None).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rename_thread(
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

#[tauri::command]
pub async fn delete_thread(state: State<'_, AppState>, id: String) -> Result<usize, String> {
    delete_with_runs(&state.orch, &id).await
}

pub(crate) async fn delete_with_runs(
    orch: &parzi_runtime::Orchestrator,
    id: &str,
) -> Result<usize, String> {
    let id = id.to_string();
    let all = orch.store().list().map_err(|e| e.to_string())?;
    let by_id: std::collections::HashMap<&str, &SessionMeta> =
        all.iter().map(|m| (m.id.as_str(), m)).collect();
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
        let _ = orch.kill(sid).await;
        orch.drop_shells(sid);
    }
    orch.store().delete_thread(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn purge_sessions(state: State<'_, AppState>) -> Result<usize, String> {
    let gone: Vec<String> = state
        .orch
        .store()
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|m| {
            matches!(
                m.status,
                parzi_core::store::SessionStatus::Done | parzi_core::store::SessionStatus::Killed
            )
        })
        .map(|m| m.id)
        .collect();
    let n = state
        .orch
        .store()
        .purge_finished()
        .map_err(|e| e.to_string())?;
    for sid in &gone {
        state.orch.drop_shells(sid);
    }
    Ok(n)
}

#[tauri::command]
pub async fn plan_get(id: String) -> Result<String, String> {
    let dir = parzi_core::paths::sessions_dir()
        .map(|d| d.join(&id))
        .map_err(|e| e.to_string())?;
    Ok(std::fs::read_to_string(dir.join("plan.json")).unwrap_or_default())
}

#[tauri::command]
pub async fn spawn_track(
    state: State<'_, AppState>,
    parent: String,
    title: String,
    prompt: String,
) -> Result<String, String> {
    state
        .orch
        .harness()
        .spawn_session(&parent, &title, &prompt, true, None, None, false, None, None)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn answer_question(
    state: State<'_, AppState>,
    key: String,
    session: String,
    answer: String,
) -> Result<(), String> {
    let entry = state.questions.lock().await.remove(&key);
    match entry {
        Some((bound, tx)) => {
            if bound != session {
                return Err("answer is for another session".into());
            }
            let _ = tx.send(answer);
            Ok(())
        }
        None => Err("question expired or unknown".into()),
    }
}

#[tauri::command]
pub async fn approve_tool(
    state: State<'_, AppState>,
    key: String,
    session: String,
    allow: bool,
) -> Result<(), String> {
    let entry = state.pending.lock().await.remove(&key);
    match entry {
        Some((bound, tx)) => {
            if bound != session {
                return Err("approval is for another session".into());
            }
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
