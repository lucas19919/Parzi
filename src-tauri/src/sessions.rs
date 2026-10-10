use std::sync::Arc;

use parzi_core::store::{Event, SessionMeta};
use parzi_runtime::tools::{Approval, Approver};
use serde_json::json;
use tauri::State;

use crate::remote::{self, mirror_id, remote_id, RemoteState};
use crate::{AppState, GuiApprover};

/// This machine's sessions, then the linked server's (ids tagged `r:`).
#[tauri::command]
pub async fn list_threads(
    state: State<'_, AppState>,
    far: State<'_, RemoteState>,
) -> Result<Vec<SessionMeta>, String> {
    let mut all = state.orch.store().list().map_err(|e| e.to_string())?;
    all.extend(remote::sessions(&state.app, &far).await);
    Ok(all)
}

#[tauri::command]
pub async fn get_thread(
    state: State<'_, AppState>,
    far: State<'_, RemoteState>,
    id: String,
) -> Result<(SessionMeta, Vec<Event>), String> {
    if let Some((device, sid)) = mirror_id(&id) {
        return remote::mirror_thread(&state.app, &far, device, sid).await;
    }
    if let Some(rid) = remote_id(&id) {
        return remote::thread(&state.app, &far, rid).await;
    }
    let meta = state.orch.store().get(&id).map_err(|e| e.to_string())?;
    let events = state.orch.store().events(&id).map_err(|e| e.to_string())?;
    Ok((meta, events))
}

/// `remote` starts a new session on the linked server; an `r:` id continues
/// one there. Either way the reply is the session's (tagged) id.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn send_message(
    state: State<'_, AppState>,
    far: State<'_, RemoteState>,
    session_id: Option<String>,
    model: String,
    prompt: String,
    cwd: String,
    effort: Option<String>,
    attachments: Option<Vec<String>>,
    mode: Option<String>,
    lane: Option<String>,
    remote: Option<bool>,
) -> Result<String, String> {
    let effort =
        parzi_runtime::orchestrator::normalize_effort(effort.as_deref().unwrap_or("medium"));
    let attached = {
        let (cwd, paths) = (cwd.clone(), attachments.unwrap_or_default());
        tauri::async_runtime::spawn_blocking(move || read_attachments(&cwd, &paths))
            .await
            .map_err(|e| e.to_string())?
    };
    // Another device's session goes on on the server: copy it there first.
    let mut adopted = None;
    if let Some((device, sid)) = session_id.as_deref().and_then(mirror_id) {
        adopted = Some(remote::adopt(&state.app, &far, device, sid).await?);
    }
    let far_target = match (adopted.as_deref(), session_id.as_deref()) {
        (Some(a), _) => Some(Some(a)),
        (None, Some(id)) => remote_id(id).map(Some),
        (None, None) if remote == Some(true) => Some(None),
        (None, None) => None,
    };
    if let Some(target) = far_target {
        return remote::send(
            &state.app, &far, target, &model, &prompt, &cwd, &effort, attached, mode, lane,
        )
        .await;
    }
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
            let project = project_for(&cwd).await;
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

/// Threads belong to their folder's project, not shared "default". The
/// vault scan reads notes, so it runs on the blocking pool.
pub(crate) async fn project_for(cwd: &str) -> String {
    if cwd.trim().is_empty() || crate::files::remote_or_device(cwd) {
        return "default".into();
    }
    let cwd = cwd.to_string();
    tauri::async_runtime::spawn_blocking(move || parzi_core::brain::project_for_folder(&cwd))
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "default".into())
}

const ATTACH_MAX: u64 = 20 * 1024 * 1024;

/// Network paths are never read (Windows would authenticate to the host)
/// and nothing over 20 MiB is loaded into memory to be cut down later.
fn read_attachments(cwd: &str, paths: &[String]) -> Vec<parzi_core::context::AttachedFile> {
    let base = if cwd.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        std::path::PathBuf::from(cwd)
    };
    if crate::files::remote_or_device(&base.to_string_lossy()) {
        return vec![];
    }
    let fit: Vec<String> = paths
        .iter()
        .filter(|p| {
            let full = base.join(p.as_str());
            if crate::files::remote_or_device(p)
                || crate::files::remote_or_device(&full.to_string_lossy())
            {
                tracing::warn!("attachment skipped: network or device path");
                return false;
            }
            match std::fs::metadata(&full) {
                Ok(m) if m.len() > ATTACH_MAX => {
                    tracing::warn!("attachment skipped: {} is over 20 MiB", full.display());
                    false
                }
                _ => true,
            }
        })
        .cloned()
        .collect();
    parzi_core::context::read_attachments(&base, &fit)
}

#[tauri::command]
pub async fn kill_run(
    state: State<'_, AppState>,
    far: State<'_, RemoteState>,
    id: String,
) -> Result<(), String> {
    read_only(&id)?;
    if let Some(rid) = remote_id(&id) {
        return remote::call(&state.app, &far, "session.kill", json!({ "id": rid }))
            .await
            .map(drop);
    }
    state.orch.kill(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn compact_thread(
    state: State<'_, AppState>,
    far: State<'_, RemoteState>,
    id: String,
) -> Result<String, String> {
    read_only(&id)?;
    if let Some(rid) = remote_id(&id) {
        let v = remote::call(&state.app, &far, "session.compact", json!({ "id": rid })).await?;
        return Ok(v
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string());
    }
    state.orch.compact(&id, "").await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fork_thread(
    state: State<'_, AppState>,
    far: State<'_, RemoteState>,
    id: String,
) -> Result<SessionMeta, String> {
    // Forking another device's session copies it to the server.
    if let Some((device, sid)) = mirror_id(&id) {
        let new = remote::adopt(&state.app, &far, device, sid).await?;
        return remote::thread(&state.app, &far, &new).await.map(|(m, _)| m);
    }
    if let Some(rid) = remote_id(&id) {
        return remote::fork(&state.app, &far, rid).await;
    }
    state.orch.fork(&id, None).await.map_err(|e| e.to_string())
}

/// Another device's session is read here, not changed: its run lives
/// there. Sending to it or forking it copies it to the server instead.
fn read_only(id: &str) -> Result<(), String> {
    if mirror_id(id).is_some() {
        return Err(
            "This session ran on another device. Send a message to continue it on the server."
                .into(),
        );
    }
    Ok(())
}

#[tauri::command]
pub async fn rename_thread(
    state: State<'_, AppState>,
    far: State<'_, RemoteState>,
    id: String,
    title: String,
) -> Result<(), String> {
    read_only(&id)?;
    if let Some(rid) = remote_id(&id) {
        return remote::call(
            &state.app,
            &far,
            "session.rename",
            json!({ "id": rid, "title": title }),
        )
        .await
        .map(drop);
    }
    state
        .orch
        .store()
        .set_title(&id, &title)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_thread(
    state: State<'_, AppState>,
    far: State<'_, RemoteState>,
    id: String,
) -> Result<usize, String> {
    read_only(&id)?;
    if let Some(rid) = remote_id(&id) {
        let v = remote::call(&state.app, &far, "session.delete", json!({ "id": rid })).await?;
        let removed = v
            .get("removed")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1);
        return Ok(usize::try_from(removed).unwrap_or(1));
    }
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
    // A server session's plan lives on the server; the side panel shows none.
    if remote_id(&id).is_some() {
        return Ok(String::new());
    }
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
    if remote_id(&parent).is_some() {
        return Err("tracks start from sessions on this PC".into());
    }
    state
        .orch
        .harness()
        .spawn_session(
            &parent, &title, &prompt, true, None, None, false, None, None,
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn answer_question(
    state: State<'_, AppState>,
    far: State<'_, RemoteState>,
    key: String,
    session: String,
    answer: String,
) -> Result<(), String> {
    if let Some(rkey) = remote_id(&key) {
        let sid = remote_id(&session).unwrap_or("");
        return remote::call(
            &state.app,
            &far,
            "question.answer",
            json!({ "key": rkey, "session": sid, "text": answer }),
        )
        .await
        .map(drop);
    }
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
    far: State<'_, RemoteState>,
    key: String,
    session: String,
    allow: bool,
) -> Result<(), String> {
    if let Some(rkey) = remote_id(&key) {
        return remote::call(
            &state.app,
            &far,
            "approval.answer",
            json!({ "key": rkey, "allow": allow }),
        )
        .await
        .map(drop);
    }
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
