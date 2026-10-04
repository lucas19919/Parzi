use parzi_core::context::{InterKind, InterSessionMessage};
use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Event, SessionMeta, SessionStatus};

use crate::handler::HarnessBridge;
use crate::inter;

use super::queue::{Pump, QueuedRun};

const HARNESS_WAIT_SECS: u64 = 180;

impl Pump {
    fn in_scope(&self, caller_id: &str, target_id: &str) -> Result<SessionMeta> {
        let target = self.store.get(target_id)?;
        if caller_id == target_id {
            return Ok(target);
        }
        let caller = self.store.get(caller_id)?;
        let same_project = caller.project == target.project;
        let related = target.parent_id.as_deref() == Some(caller_id)
            || caller.parent_id.as_deref() == Some(target_id)
            || parzi_core::store::subtree_ids(&self.store.list()?, caller_id).contains(target_id);
        if same_project || related {
            return Ok(target);
        }
        Err(ParziError::Store(format!(
            "session {target_id} is outside this project"
        )))
    }

    async fn await_settled(&self, session_id: &str) -> Option<SessionMeta> {
        let ticks = HARNESS_WAIT_SECS * 4;
        for _ in 0..ticks {
            match self.store.get(session_id) {
                Ok(m)
                    if matches!(
                        m.status,
                        SessionStatus::Done | SessionStatus::Idle | SessionStatus::Killed
                    ) =>
                {
                    return Some(m)
                }
                Err(_) => return None,
                _ => tokio::time::sleep(std::time::Duration::from_millis(250)).await,
            }
        }
        None
    }

    fn last_reply(&self, session_id: &str) -> String {
        let mut reply = String::new();
        if let Ok(events) = self.store.events(session_id) {
            for e in events {
                if let Event::Assistant { text, .. } = e {
                    reply = text;
                }
            }
        }
        let cut: String = reply.chars().take(4_000).collect();
        cut
    }

    fn reply_json(&self, session_id: &str, meta: &SessionMeta) -> String {
        let status = format!("{:?}", meta.status).to_lowercase();
        let reply = self.last_reply(session_id);
        serde_json::json!({
            "session_id": session_id,
            "status": status,
            "title": meta.title,
            "response": reply,
        })
        .to_string()
    }
}

#[async_trait::async_trait]
impl HarnessBridge for Pump {
    async fn spawn_session(
        &self,
        caller_id: &str,
        title: &str,
        prompt: &str,
        is_subsession: bool,
        model: Option<String>,
        lane: Option<String>,
        wait: bool,
    ) -> Result<String> {
        let caller = self.store.get(caller_id)?;
        let title: String = if title.trim().is_empty() {
            prompt
                .lines()
                .next()
                .unwrap_or("subsession")
                .chars()
                .take(80)
                .collect()
        } else {
            title.chars().take(80).collect()
        };
        let model_spec = model.unwrap_or_else(|| caller.model.clone());
        let lane_name = lane.unwrap_or_else(|| caller.lane.clone());
        let parent = if is_subsession { Some(caller_id) } else { None };
        let meta = self.store.create_with_parent(
            &title,
            &caller.project,
            &lane_name,
            &model_spec,
            parent,
        )?;
        if !caller.cwd.is_empty() {
            let _ = self.store.set_cwd(&meta.id, &caller.cwd);
        }
        let q = QueuedRun {
            session_id: meta.id.clone(),
            project: caller.project.clone(),
            lane: lane_name,
            model_spec,
            prompt: prompt.to_string(),
            cwd: caller.cwd.clone(),
            effort: "medium".into(),
            attachments: vec![],
            approver: None,
            prompt_recorded: false,
            inbox_from: None,
            mode_override: None,
        };
        let launched = self.dispatch(q).await;
        let meta = self.store.get(&meta.id)?;
        if !wait {
            return Ok(serde_json::json!({
                "session_id": meta.id,
                "status": format!("{:?}", meta.status).to_lowercase(),
                "title": meta.title,
            })
            .to_string());
        }
        if !launched {
            return Ok(serde_json::json!({
                "session_id": meta.id,
                "status": "queued",
                "title": meta.title,
                "note": "no run slot free; parent would deadlock waiting — poll with session.read_session",
            })
            .to_string());
        }
        match self.await_settled(&meta.id).await {
            Some(done) => Ok(self.reply_json(&meta.id, &done)),
            None => Ok(serde_json::json!({
                "session_id": meta.id,
                "status": "timeout",
                "note": "child still running; poll with session.read_session",
            })
            .to_string()),
        }
    }

    async fn send_message(
        &self,
        caller_id: &str,
        session_id: &str,
        message: &str,
        kind: InterKind,
        wait: bool,
    ) -> Result<String> {
        let target = self.in_scope(caller_id, session_id)?;
        let caller = self.store.get(caller_id)?;
        let msg = inter::from_caller(&caller, kind, message);
        let (at, still_live) = {
            let mut h = self.handles.lock().await;
            h.retain(|_, handle| !handle.finished());
            let at = self.store.events(session_id)?.len();
            inter::deliver(&self.store, session_id, &msg)?;
            (at, h.contains_key(session_id))
        };
        if still_live {
            if !wait {
                return Ok(serde_json::json!({
                    "session_id": session_id,
                    "status": "active",
                    "note": "target is running; message appended to its transcript",
                })
                .to_string());
            }
            return match self.await_settled(session_id).await {
                Some(done) => Ok(self.reply_json(session_id, &done)),
                None => Ok(serde_json::json!({
                    "session_id": session_id,
                    "status": "timeout",
                    "note": "target still running; poll with session.read_session",
                })
                .to_string()),
            };
        }
        let q = QueuedRun {
            session_id: session_id.to_string(),
            project: target.project.clone(),
            lane: target.lane.clone(),
            model_spec: target.model.clone(),
            prompt: msg.render(),
            cwd: target.cwd.clone(),
            effort: "medium".into(),
            attachments: vec![],
            approver: None,
            prompt_recorded: true,
            mode_override: None,
            inbox_from: Some(at),
        };
        let launched = self.dispatch(q).await;
        if !wait {
            let meta = self.store.get(session_id)?;
            return Ok(serde_json::json!({
                "session_id": session_id,
                "status": format!("{:?}", meta.status).to_lowercase(),
            })
            .to_string());
        }
        if !launched {
            return Ok(serde_json::json!({
                "session_id": session_id,
                "status": "queued",
                "note": "no run slot free — poll with session.read_session",
            })
            .to_string());
        }
        match self.await_settled(session_id).await {
            Some(done) => Ok(self.reply_json(session_id, &done)),
            None => Ok(serde_json::json!({
                "session_id": session_id,
                "status": "timeout",
                "note": "target still running; poll with session.read_session",
            })
            .to_string()),
        }
    }

    async fn read_session(
        &self,
        caller_id: &str,
        session_id: &str,
        tail_events: Option<usize>,
    ) -> Result<String> {
        let meta = self.in_scope(caller_id, session_id)?;
        let tail = tail_events.unwrap_or(20).clamp(1, 60);
        let events = self.store.events(session_id).unwrap_or_default();
        let start = events.len().saturating_sub(tail);
        let mut tail_text = String::new();
        for e in events.iter().skip(start) {
            let line = match e {
                Event::System { text } => match InterSessionMessage::decode(text) {
                    Some(m) => format!("> inter: {}\n", truncate(&m.summary(), 500)),
                    None => format!("> system: {text}\n"),
                },
                Event::User { text } => format!("user: {}\n", truncate(text, 1_000)),
                Event::Assistant { text, .. } => format!("assistant: {}\n", truncate(text, 2_000)),
                Event::ToolCall { name, .. } => format!("tool_call: {name}\n"),
                Event::ToolResult {
                    name, ok, output, ..
                } => {
                    format!("tool_result({name}, ok={ok}): {}\n", truncate(output, 500))
                }
                Event::Reasoning { text } => format!("reasoning: {}\n", truncate(text, 300)),
                Event::Checkpoint { summary } => {
                    format!("checkpoint: {}\n", truncate(summary, 300))
                }
                Event::Widget { .. } => "[widget]\n".to_string(),
                Event::Artifact {
                    id, title, version, ..
                } => {
                    format!("artifact: {title} ({id} v{version})\n")
                }
                Event::Error { message, .. } => format!("error: {}\n", truncate(message, 300)),
            };
            tail_text.push_str(&line);
            if tail_text.len() > 8_000 {
                tail_text.push_str("…(truncated)\n");
                break;
            }
        }
        Ok(serde_json::json!({
            "session_id": meta.id,
            "title": meta.title,
            "status": format!("{:?}", meta.status).to_lowercase(),
            "model": meta.model,
            "parent_id": meta.parent_id,
            "transcript_tail": tail_text,
        })
        .to_string())
    }

    async fn list_sessions(&self, caller_id: &str, only_subsessions: bool) -> Result<String> {
        let all = self.store.list()?;
        let caller = self.store.get(caller_id)?;
        let subtree = parzi_core::store::subtree_ids(&all, caller_id);
        let rows: Vec<serde_json::Value> = all
            .iter()
            .filter(|m| m.project == caller.project || subtree.contains(&m.id))
            .filter(|m| !only_subsessions || m.parent_id.as_deref() == Some(caller_id))
            .take(50)
            .map(|m| {
                serde_json::json!({
                    "session_id": m.id,
                    "title": m.title,
                    "status": format!("{:?}", m.status).to_lowercase(),
                    "parent_id": m.parent_id,
                    "model": m.model,
                })
            })
            .collect();
        Ok(serde_json::Value::Array(rows).to_string())
    }
}

fn truncate(s: &str, n: usize) -> String {
    let cut: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{cut}…")
    } else {
        cut
    }
}
