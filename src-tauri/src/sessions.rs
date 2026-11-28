use std::sync::Arc;

use parzi_core::store::{Event, SessionMeta};
use parzi_runtime::tools::{Approval, Approver};
use serde_json::json;
use tauri::State;

use crate::remote::{self, mirror_id, remote_id, RemoteState};
use crate::{AppState, GuiApprover};

#[tauri::command]
pub async fn list_threads(
    state: State<'_, AppState>,
    far: State<'_, RemoteState>,
) -> Result<Vec<SessionMeta>, String> {
    let store = state.orch.store().clone();
    let mut all = tauri::async_runtime::spawn_blocking(move || store.list())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    all.extend(remote::sessions(&state.app, &far).await);
    Ok(all)
}

#[tauri::command]
pub async fn get_thread(
    state: State<'_, AppState>,
    far: State<'_, RemoteState>,
    id: String,
) -> Result<(SessionMeta, Vec<serde_json::Value>), String> {
    if let Some((device, sid)) = mirror_id(&id) {
        return remote::mirror_thread(&state.app, &far, device, sid)
            .await
            .map(|(m, evs)| (m, evs.iter().map(stamped_plain).collect()));
    }
    if let Some(rid) = remote_id(&id) {
        return remote::thread(&state.app, &far, rid)
            .await
            .map(|(m, evs)| (m, evs.iter().map(stamped_plain).collect()));
    }
    let store = state.orch.store().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let meta = store.get(&id).map_err(|e| e.to_string())?;
        let stamped = store.events_with_times(&id).map_err(|e| e.to_string())?;
        let mut events: Vec<serde_json::Value> = stamped
            .into_iter()
            .map(|(e, at)| stamped_of(&e, at))
            .collect();
        // Read after the store: a step in between saves these words and
        // clears them, so they are never lost; what the store already has of
        // them is cut off the front.
        let unsaved = crate::unsaved_reply(&id).and_then(|t| unsaved_tail(&events, &t));
        if let Some((text, upto)) = unsaved {
            events.push(serde_json::json!({
                "kind": "assistant", "text": text, "done": false, "unsaved": true, "upto": upto,
            }));
        }
        Ok((meta, events))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The part of a running reply the store does not have yet, and the whole
/// reply's length in UTF-16 units (what the window counts text events by).
/// The store saves each finished message of a reply, so the messages saved
/// since the last step are what it already has.
fn unsaved_tail(events: &[serde_json::Value], unsaved: &str) -> Option<(String, usize)> {
    let upto = unsaved.encode_utf16().count();
    let since = events
        .iter()
        .rposition(|e| {
            matches!(
                e["kind"].as_str(),
                Some("tool_call" | "tool_result" | "user")
            )
        })
        .map_or(0, |i| i + 1);
    let saved: String = events[since..]
        .iter()
        .filter(|e| e["kind"] == "assistant")
        .filter_map(|e| e["text"].as_str())
        .collect();
    let rest = if let Some(rest) = unsaved.strip_prefix(saved.as_str()) {
        rest
    } else {
        let head = saved.trim();
        match unsaved.find(head) {
            Some(at) if !head.is_empty() => &unsaved[at + head.len()..],
            _ => unsaved,
        }
    };
    (!rest.trim().is_empty()).then(|| (rest.to_string(), upto))
}

fn stamped_of(event: &Event, at: Option<i64>) -> serde_json::Value {
    let mut v = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
    if let (Some(at), Some(map)) = (at, v.as_object_mut()) {
        map.insert("at".into(), at.into());
    }
    v
}

fn stamped_plain(event: &Event) -> serde_json::Value {
    stamped_of(event, None)
}

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
    remote: Option<bool>,
    parent: Option<String>,
    worktree: Option<String>,
) -> Result<String, String> {
    let effort =
        parzi_runtime::orchestrator::normalize_effort(effort.as_deref().unwrap_or("medium"));
    let attached = {
        let (cwd, paths) = (cwd.clone(), attachments.unwrap_or_default());
        tauri::async_runtime::spawn_blocking(move || read_attachments(&cwd, &paths))
            .await
            .map_err(|e| e.to_string())?
    };
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
            &state.app, &far, target, &model, &prompt, &cwd, &effort, attached, mode,
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
            let cwd = match worktree.filter(|b| !b.trim().is_empty() && !cwd.is_empty()) {
                Some(base) => {
                    let (repo, about) = (std::path::PathBuf::from(&cwd), prompt.clone());
                    tauri::async_runtime::spawn_blocking(move || {
                        crate::worktree::create(&repo, base.trim(), &about)
                    })
                    .await
                    .map_err(|e| e.to_string())??
                    .display()
                    .to_string()
                }
                None => cwd.clone(),
            };
            state
                .orch
                .spawn(
                    &project,
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
    if let Some(parent) = parent.filter(|p| !p.is_empty() && *p != sid) {
        if let Err(e) = state.orch.store().set_parent(&sid, &parent) {
            tracing::warn!("side task {sid} not linked to {parent}: {e}");
        }
    }
    Ok(sid)
}

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
    if let Some((device, sid)) = mirror_id(&id) {
        let new = remote::adopt(&state.app, &far, device, sid).await?;
        return remote::thread(&state.app, &far, &new).await.map(|(m, _)| m);
    }
    if let Some(rid) = remote_id(&id) {
        return remote::fork(&state.app, &far, rid).await;
    }
    state.orch.fork(&id, None).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rewind_thread(
    state: State<'_, AppState>,
    id: String,
    nth: usize,
) -> Result<SessionMeta, String> {
    read_only(&id)?;
    if remote_id(&id).is_some() {
        return Err("Rewind works on this computer's sessions.".into());
    }
    state.orch.rewind(&id, nth).map_err(|e| e.to_string())
}

/// Sends the session's last message again to the model picked now (a retry
/// after a usage limit), without a second copy in the thread.
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "a Tauri command's arguments")]
pub async fn resend_last(
    state: State<'_, AppState>,
    session_id: String,
    model: String,
    cwd: String,
    effort: Option<String>,
    attachments: Option<Vec<String>>,
    mode: Option<String>,
) -> Result<(), String> {
    read_only(&session_id)?;
    if remote_id(&session_id).is_some() {
        return Err("Retrying works on this computer's sessions; send the message again.".into());
    }
    let effort =
        parzi_runtime::orchestrator::normalize_effort(effort.as_deref().unwrap_or("medium"));
    let attached = {
        let (cwd, paths) = (cwd.clone(), attachments.unwrap_or_default());
        tauri::async_runtime::spawn_blocking(move || read_attachments(&cwd, &paths))
            .await
            .map_err(|e| e.to_string())?
    };
    let approver: Arc<dyn Approver> = Arc::new(GuiApprover {
        app: state.app.clone(),
        pending: state.pending.clone(),
    });
    state
        .orch
        .resend_last(
            &session_id,
            Some(approver),
            &cwd,
            &effort,
            attached,
            Some(model),
            mode,
        )
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Answers a plan the agent proposed: yes saves it as the session's plan;
/// either way the proposal card goes. The window sends the answer on as the
/// person's next message.
#[tauri::command]
pub async fn answer_plan_proposal(
    state: State<'_, AppState>,
    id: String,
    accept: bool,
) -> Result<SessionMeta, String> {
    read_only(&id)?;
    if remote_id(&id).is_some() {
        return Err(
            "Answer this plan by sending a message: plan cards on the server's sessions can't be answered here yet."
                .into(),
        );
    }
    state
        .orch
        .answer_proposal(&id, accept)
        .map_err(|e| e.to_string())
}

/// Moves a session to another folder (a project, or a branch's worktree) and
/// project, from the Environment card.
#[tauri::command]
pub async fn set_session_environment(
    state: State<'_, AppState>,
    id: String,
    cwd: String,
    project: Option<String>,
) -> Result<SessionMeta, String> {
    read_only(&id)?;
    if remote_id(&id).is_some() {
        return Err("Moving a session works on this computer's sessions for now.".into());
    }
    state
        .orch
        .set_environment(&id, &cwd, project.as_deref())
        .await
        .map_err(|e| e.to_string())
}

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
    let cwd = state
        .orch
        .store()
        .get(&id)
        .map(|m| m.cwd)
        .unwrap_or_default();
    let removed = state
        .orch
        .delete_thread(&id)
        .await
        .map_err(|e| e.to_string())?;
    if !cwd.is_empty() {
        let store = state.orch.store().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let used = store
                .list()
                .unwrap_or_default()
                .iter()
                .any(|m| m.cwd == cwd);
            if !used {
                let dir = std::path::Path::new(&cwd);
                if parzi_providers::drop_live_in(dir) > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(800));
                }
                if let Some(note) = crate::worktree::remove(dir) {
                    tracing::warn!("worktree {note}");
                }
            }
        });
    }
    Ok(removed)
}

#[tauri::command]
pub async fn purge_sessions(state: State<'_, AppState>) -> Result<usize, String> {
    state.orch.purge_finished().map_err(|e| e.to_string())
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
    let mut lock = state.questions.lock().await;
    match lock.remove(&key) {
        Some((bound, tx)) if bound == session => {
            let _ = tx.send(answer);
            Ok(())
        }
        Some(other) => {
            lock.insert(key, other);
            Err("answer is for another session".into())
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
    let mut lock = state.pending.lock().await;
    match lock.remove(&key) {
        Some((bound, tx)) if bound == session => {
            let _ = tx.send(if allow {
                Approval::Allow
            } else {
                Approval::Deny
            });
            Ok(())
        }
        Some(other) => {
            lock.insert(key, other);
            Err("approval is for another session".into())
        }
        None => Err("approval expired or unknown".into()),
    }
}

#[cfg(test)]
mod unsaved_tests {
    use super::unsaved_tail;
    use serde_json::json;

    #[test]
    fn only_the_words_the_store_lacks_are_added() {
        let events = vec![
            json!({"kind": "user", "text": "hi"}),
            json!({"kind": "tool_call", "id": "1"}),
            json!({"kind": "assistant", "text": "First part. "}),
        ];
        assert_eq!(
            unsaved_tail(&events, "First part. Second"),
            Some(("Second".into(), 18))
        );
        // Words from before the last step were saved with it.
        let fresh = vec![json!({"kind": "user", "text": "hi"})];
        assert_eq!(unsaved_tail(&fresh, "Hé"), Some(("Hé".into(), 2)));
        // All of it saved already: nothing to add.
        assert_eq!(unsaved_tail(&events, "First part. "), None);
    }
}
