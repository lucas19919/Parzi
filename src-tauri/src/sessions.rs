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
    let meta = state.orch.store().get(&id).map_err(|e| e.to_string())?;
    let stamped = state
        .orch
        .store()
        .events_with_times(&id)
        .map_err(|e| e.to_string())?;
    Ok((meta, stamped.into_iter().map(|(e, at)| stamped_of(&e, at)).collect()))
}

/// A transcript event as the thread shows it, with its Unix time when the
/// line said (lines from before timestamps existed say nothing).
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

/// `remote` starts a new session on the linked server; an `r:` id continues
/// one there. Either way the reply is the session's (tagged) id. `quotes`
/// are plan items the person quoted: each goes after their text as a
/// `<plan-item>` block, and the session is linked to the first.
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
    quotes: Option<Vec<crate::plans::Quote>>,
    parent: Option<String>,
) -> Result<String, String> {
    let quoted = match quotes.filter(|q| !q.is_empty()) {
        Some(q) => {
            let found = tauri::async_runtime::spawn_blocking(move || crate::plans::resolve(&q))
                .await
                .map_err(|e| e.to_string())?;
            if found.is_empty() && prompt.trim().is_empty() {
                return Err(
                    "Nothing to send: the quoted plan items are gone from the plan.".into(),
                );
            }
            found
        }
        None => vec![],
    };
    // A new session that says little more than "start this" is the item's.
    let name = quoted
        .first()
        .filter(|_| session_id.is_none() && crate::plans::named_after_item(&prompt))
        .map(|(_, item)| item.title.clone());
    let text = prompt;
    let prompt = if quoted.is_empty() {
        text
    } else {
        crate::plans::quoted_prompt(&text, &quoted, &cwd)
    };
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
        let item = quoted.first().map(|(_, i)| i.id.as_str());
        let id = remote::send(
            &state.app, &far, target, &model, &prompt, &cwd, &effort, attached, mode, lane, item,
        )
        .await?;
        if let (Some(title), Some(rid)) = (name, remote_id(&id)) {
            let renamed = json!({ "id": rid, "title": title });
            if let Err(e) = remote::call(&state.app, &far, "session.rename", renamed).await {
                tracing::warn!("session {id} not named after its item: {e}");
            }
        }
        return Ok(id);
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
    // A side task: the new session lists under the one it came from, which
    // goes on undisturbed (nothing is sent to it).
    if let Some(parent) = parent.filter(|p| !p.is_empty() && *p != sid) {
        if let Err(e) = state.orch.store().set_parent(&sid, &parent) {
            tracing::warn!("side task {sid} not linked to {parent}: {e}");
        }
    }
    if !quoted.is_empty() {
        let store = state.orch.store().clone();
        let (sid, quoted) = (sid.clone(), quoted.clone());
        let _ =
            tauri::async_runtime::spawn_blocking(move || link(&store, &sid, &quoted, name)).await;
    }
    Ok(sid)
}

/// A session sent quoted plan items works on the first of its project's,
/// and, new and told little else, is named after it.
fn link(
    store: &parzi_core::store::SessionStore,
    sid: &str,
    quoted: &[(String, parzi_core::plan::PlanItem)],
    name: Option<String>,
) {
    let Ok(meta) = store.get(sid) else {
        return;
    };
    if let Some(item) = crate::plans::link_for(&meta.project, quoted) {
        if let Err(e) = store.set_plan_item(sid, Some(item)) {
            tracing::warn!("session {sid} not linked to its plan item: {e}");
        }
    }
    if let Some(title) = name {
        if let Err(e) = store.set_title(sid, &title) {
            tracing::warn!("session {sid} not named after its item: {e}");
        }
    }
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
    state.orch.fork(&id, None).map_err(|e| e.to_string())
}

/// The session as it stood before the person's `nth` message (0 is the
/// first), as a new session under the same title; the original stays.
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
    state.orch.delete_thread(&id).await.map_err(|e| e.to_string())
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
    match lock.get(&key) {
        Some((bound, _)) if bound != &session => {
            Err("answer is for another session".into())
        }
        Some(_) => {
            let (_, tx) = lock.remove(&key).unwrap();
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
    let mut lock = state.pending.lock().await;
    match lock.get(&key) {
        Some((bound, _)) if bound != &session => {
            Err("approval is for another session".into())
        }
        Some(_) => {
            let (_, tx) = lock.remove(&key).unwrap();
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
