use parzi_core::context::{InterKind, InterSessionMessage};
use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Event, RunCause, SessionMeta, SessionStatus};
use parzi_core::text::{clip, fold};

use crate::handler::{HarnessBridge, SpawnSpec};
use crate::inter;

use super::normalize_effort;
use super::queue::{set_report_to, take_report_to, Dispatch, Pump, QueuedRun};

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

    fn reply_json(&self, session_id: &str, meta: &SessionMeta, note: Option<&str>) -> String {
        // One read of the transcript serves reply, error, and outcome.
        let events = self.store.events(session_id).unwrap_or_default();
        let status = meta.status.word();
        let reply: String = events
            .iter()
            .rev()
            .find_map(|e| match e {
                Event::Assistant { text, .. } => Some(text.chars().take(4_000).collect()),
                _ => None,
            })
            .unwrap_or_default();
        let mut o = serde_json::json!({
            "session_id": session_id,
            "status": status,
            "title": meta.title,
            "response": reply,
        });
        // Idle with an Error event is failure, not success: pass it on.
        let last_error = events.iter().rev().find_map(|e| match e {
            Event::Error { message, .. } => Some(message.chars().take(500).collect::<String>()),
            _ => None,
        });
        if let Some(err) = last_error {
            o["error"] = err.into();
        }
        // Verifiable outcome alongside the last reply.
        o["outcome"] = run_outcome(&events).into();
        if let Some(n) = note {
            o["note"] = n.into();
        }
        o.to_string()
    }
}

fn run_outcome(events: &[Event]) -> String {
    let mut shells = 0;
    let mut last_shell_ok: Option<bool> = None;
    let mut artifacts = 0;
    let mut errors = 0;
    for e in events {
        match e {
            Event::ToolResult { name, ok, .. } if name == "shell.exec" || name == "shell.start" => {
                shells += 1;
                last_shell_ok = Some(*ok);
            }
            Event::Artifact { .. } => artifacts += 1,
            Event::Error { .. } => errors += 1,
            _ => {}
        }
    }
    let shell = match last_shell_ok {
        Some(true) => format!("last shell ok ({shells} calls)"),
        Some(false) => format!("last shell FAILED ({shells} calls)"),
        None => "no shell calls".to_string(),
    };
    format!("{shell}, {artifacts} artifacts, {errors} errors")
}

impl Pump {
    /// A child session, named by its caller or after its prompt. A child left
    /// running in the background reports back to the caller when its run
    /// ends.
    async fn spawn_child(&self, caller_id: &str, spec: SpawnSpec) -> Result<String> {
        let SpawnSpec {
            title,
            prompt,
            is_subsession,
            model,
            wait,
            mode_override,
            effort,
        } = spec;
        let caller = self.store.get(caller_id)?;
        // The tools' stand-in names say nothing: name it after its prompt.
        let title: String = match title.trim() {
            "" | "subsession" => parzi_core::text::name_from_prompt(&prompt),
            t => t.chars().take(80).collect(),
        };
        let model_spec = model.unwrap_or_else(|| caller.model.clone());
        // Ask/Deny parents pass no posture: warn that the child is crippled.
        let crippled = mode_override.is_none();
        let parent = if is_subsession { Some(caller_id) } else { None };
        let meta = self
            .store
            .create_with_parent(&title, &caller.project, &model_spec, parent)?;
        if !caller.cwd.is_empty() {
            let _ = self.store.set_cwd(&meta.id, &caller.cwd);
        }
        if !wait {
            // Set before it can start: a quick child must not end unheard.
            set_report_to(&meta.id, caller_id);
        }
        let q = QueuedRun {
            session_id: meta.id.clone(),
            project: caller.project.clone(),
            model_spec,
            prompt,
            cwd: caller.cwd.clone(),
            effort: normalize_effort(&effort.unwrap_or_default()),
            attachments: vec![],
            approver: None,
            restore_approver: false,
            prompt_recorded: false,
            inbox_from: None,
            mode_override,
            cause: RunCause::Message,
        };
        // Say it now instead of a mute child discovered 180s later.
        let crippled_note = "child runs Ask with no approver: approval-gated tools are denied. Staff autonomous work from a Full session.";
        match self.dispatch(q).await {
            Dispatch::Launched => {}
            Dispatch::Queued => {
                // Waiting would deadlock: it runs when a slot frees, and reports.
                set_report_to(&meta.id, caller_id);
                let mut o = serde_json::json!({
                    "session_id": meta.id,
                    "status": "queued",
                    "title": meta.title,
                    "note": "no run slot free; it runs when one frees",
                    "report": REPORTS,
                });
                if crippled {
                    o["warning"] = crippled_note.into();
                }
                return Ok(o.to_string());
            }
            Dispatch::Busy => {
                // Nothing will ever run it: stand it down, don't phantom-run.
                take_report_to(&meta.id);
                let _ = self.store.set_status(&meta.id, SessionStatus::Idle);
                return Ok(serde_json::json!({
                    "session_id": meta.id,
                    "status": "idle",
                    "title": meta.title,
                    "note": "no run slot free and queueing is off; retry the spawn when a slot frees",
                })
                .to_string());
            }
            Dispatch::Failed(e) => {
                // No run, so no report will ever come: say why, now.
                take_report_to(&meta.id);
                return Err(not_started(&meta.id, &e));
            }
        }
        let meta = self.store.get(&meta.id)?;
        if !wait {
            let mut o = serde_json::json!({
                "session_id": meta.id,
                "status": meta.status.word(),
                "title": meta.title,
                "report": REPORTS,
            });
            if crippled {
                o["note"] = crippled_note.into();
            }
            return Ok(o.to_string());
        }
        match self.await_settled(&meta.id).await {
            Some(done) => Ok(self.reply_json(&meta.id, &done, crippled.then_some(crippled_note))),
            None => {
                self.report_later(caller_id, &meta.id).await;
                Ok(serde_json::json!({
                    "session_id": meta.id,
                    "status": "timeout",
                    "note": "child still running",
                    "report": REPORTS,
                })
                .to_string())
            }
        }
    }

    /// A wait gave up while `target` still runs: it reports when it ends.
    /// If it ended in between, the report goes out now.
    async fn report_later(&self, caller_id: &str, target: &str) {
        set_report_to(target, caller_id);
        let running = {
            let mut h = self.handles.lock().await;
            h.retain(|_, x| !x.finished());
            h.contains_key(target)
        };
        let busy = self
            .store
            .get(target)
            .is_ok_and(|m| matches!(m.status, SessionStatus::Active | SessionStatus::Queued));
        if !running && !busy {
            self.report_to_parent(target, false).await;
        }
    }

    /// The reply to a message whose wait gave up: when the caller started
    /// the target, it hears back when the target ends.
    async fn timed_out(&self, caller_id: &str, target: &str, reports: bool) -> String {
        if reports {
            self.report_later(caller_id, target).await;
            return serde_json::json!({
                "session_id": target,
                "status": "timeout",
                "note": "target still running",
                "report": REPORTS,
            })
            .to_string();
        }
        serde_json::json!({
            "session_id": target,
            "status": "timeout",
            "note": "target still running; check later with session.read",
        })
        .to_string()
    }
}

/// What a background child's reply promises.
const REPORTS: &str = "Parzi messages you when it ends, and wakes you if you are idle; do not poll";

/// A run that could not start, as the tool call that asked for it fails.
fn not_started(session_id: &str, e: &ParziError) -> ParziError {
    let why = match e {
        ParziError::Store(m) | ParziError::Validation(m) | ParziError::Config(m) => m.clone(),
        other => other.to_string(),
    };
    ParziError::Validation(format!("session {session_id} did not start: {why}"))
}

#[async_trait::async_trait]
impl HarnessBridge for Pump {
    async fn spawn(&self, caller_id: &str, spec: SpawnSpec) -> Result<String> {
        self.spawn_child(caller_id, spec).await
    }

    async fn send_message(
        &self,
        caller_id: &str,
        session_id: &str,
        message: &str,
        kind: InterKind,
        wait: bool,
        effort: Option<String>,
    ) -> Result<String> {
        let target = self.in_scope(caller_id, session_id)?;
        let caller = self.store.get(caller_id)?;
        let msg = inter::from_caller(&caller, kind, message);
        // A session the caller started answers back when it ends.
        let reports = target.parent_id.as_deref() == Some(caller_id);
        if reports && !wait {
            set_report_to(session_id, caller_id);
        }
        let (at, still_live) = {
            let mut h = self.handles.lock().await;
            h.retain(|_, handle| !handle.finished());
            let at = self.store.event_count(session_id)?;
            inter::deliver(&self.store, session_id, &msg)?;
            (at, h.contains_key(session_id))
        };
        if still_live {
            if !wait {
                return Ok(serde_json::json!({
                    "session_id": session_id,
                    "status": SessionStatus::Active.word(),
                    "note": "target is running; message appended to its transcript",
                })
                .to_string());
            }
            return match self.await_settled(session_id).await {
                Some(done) => Ok(self.reply_json(session_id, &done, None)),
                None => Ok(self.timed_out(caller_id, session_id, reports).await),
            };
        }
        let q = QueuedRun {
            session_id: session_id.to_string(),
            project: target.project.clone(),
            model_spec: target.model.clone(),
            prompt: msg.render(),
            cwd: target.cwd.clone(),
            effort: normalize_effort(&effort.unwrap_or_default()),
            attachments: vec![],
            approver: None,
            restore_approver: false,
            prompt_recorded: true,
            mode_override: None,
            inbox_from: Some(at),
            cause: RunCause::Message,
        };
        match self.dispatch(q).await {
            Dispatch::Launched => {}
            Dispatch::Queued => {
                if reports {
                    set_report_to(session_id, caller_id);
                }
                return Ok(serde_json::json!({
                    "session_id": session_id,
                    "status": "queued",
                    "note": if reports {
                        "no run slot free; it runs when one frees. Parzi messages you when it ends"
                    } else {
                        "no run slot free; it runs when one frees"
                    },
                })
                .to_string());
            }
            Dispatch::Busy => {
                return Ok(serde_json::json!({
                    "session_id": session_id,
                    "status": "idle",
                    "note": "no run slot free and queueing is off; retry the message",
                })
                .to_string());
            }
            Dispatch::Failed(e) => {
                if reports && !wait {
                    take_report_to(session_id);
                }
                return Err(not_started(session_id, &e));
            }
        }
        if !wait {
            let meta = self.store.get(session_id)?;
            return Ok(serde_json::json!({
                "session_id": session_id,
                "status": meta.status.word(),
            })
            .to_string());
        }
        match self.await_settled(session_id).await {
            Some(done) => Ok(self.reply_json(session_id, &done, None)),
            None => Ok(self.timed_out(caller_id, session_id, reports).await),
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
                    Some(m) => format!("> inter: {}\n", clip(&m.summary(), 500)),
                    None => format!("> system: {text}\n"),
                },
                Event::User { text } => format!("user: {}\n", clip(text, 1_000)),
                Event::Assistant { text, .. } => format!("assistant: {}\n", clip(text, 2_000)),
                Event::ToolCall { name, args, .. } => {
                    format!(
                        "tool_call: {name} {}\n",
                        clip(&fold(&args.to_string()), 120)
                    )
                }
                Event::ToolResult {
                    name, ok, output, ..
                } => {
                    format!("tool_result({name}, ok={ok}): {}\n", clip(output, 1_500))
                }
                Event::Reasoning { text } => format!("reasoning: {}\n", clip(text, 300)),
                Event::Checkpoint { summary, .. } => {
                    format!("checkpoint: {}\n", clip(summary, 300))
                }
                Event::Widget { .. } => "[widget]\n".to_string(),
                Event::Artifact {
                    id, title, version, ..
                } => {
                    format!("artifact: {title} ({id} v{version})\n")
                }
                Event::Error { message, .. } => format!("error: {}\n", clip(message, 300)),
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
            "status": meta.status.word(),
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
                    "status": m.status.word(),
                    "parent_id": m.parent_id,
                    "model": m.model,
                })
            })
            .collect();
        Ok(serde_json::Value::Array(rows).to_string())
    }
}
