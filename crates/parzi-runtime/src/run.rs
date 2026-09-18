//! One run of a thread. Parzi hands the turn to the provider's own agent
//! and writes down what comes back — words, tool calls, usage, and the
//! failure when there is one. The transcript is the record: a failed run
//! leaves an error event in the thread, never only a toast.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use parzi_core::config::Budget;
use parzi_core::context::{AttachedFile, InterSessionMessage};
use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Event, SessionStatus, SessionStore};
use parzi_providers::{
    ErrorClass, PermissionGate, Provider, ProviderError, ProviderEvent, TurnEnd, TurnSpec,
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::handler::{user_event_text, RunEvent, RunSink};
use crate::mcp_host::McpHost;
use crate::orchestrator::{set_run_session, SessionSlot, VendorSession};
use crate::status::StatusBoard;
use crate::toolhost::ToolHost;
use crate::tools::{display_name, humanize_tool_call};

/// Turns one run may take: the prompt, then messages other sessions sent
/// while it worked. A ceiling, so two sessions cannot ping-pong forever.
const MAX_TURNS: u32 = 8;
/// Earlier conversation a new vendor session is given, at most.
const HISTORY_CHARS: usize = 12_000;
/// A tool's output as the transcript keeps it, at most.
const OUTPUT_CHARS: usize = 20_000;

pub struct EngineRunParts {
    pub session_id: String,
    pub provider_id: String,
    pub provider: Arc<dyn Provider>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Standing instructions, already joined.
    pub instructions: String,
    pub cwd: String,
    pub attachments: Vec<AttachedFile>,
    pub store: SessionStore,
    pub host: Arc<ToolHost>,
    /// Where Parzi's tools are served. `None`: this run offers none.
    pub mcp: Option<Arc<McpHost>>,
    pub status: Arc<StatusBoard>,
    pub sink: RunSink,
    pub cancel: CancellationToken,
    /// R-4: what this run may spend before it pauses.
    pub budget: Budget,
    /// The opening turn is already in the transcript (queued at enqueue, or
    /// a message from another session): do not write a second User turn.
    pub prompt_recorded: bool,
    /// This run's hold on its session, for its last look for messages.
    pub(crate) slot: SessionSlot,
    /// The transcript's length when the launch claimed the session.
    pub seen: usize,
    /// Started by a message from another session: where it landed.
    pub inbox_from: Option<usize>,
}

pub struct EngineRun {
    p: EngineRunParts,
    /// The vendor's handle for this thread's conversation.
    resume: std::sync::Mutex<Option<Value>>,
    spent: std::sync::Mutex<(u64, f64)>,
    /// The agent put a dollar figure on some of this run.
    cost_seen: AtomicBool,
}

enum Outcome {
    Completed,
    Interrupted,
    Paused(String),
    Failed(ProviderError),
}

/// What a turn keeps between provider events.
#[derive(Default)]
struct Turn {
    /// The last finished message: written once we know if it ends the turn.
    pending: Option<String>,
    /// Text streamed since the last finished message (kept on a stop).
    partial: String,
    started: HashMap<String, Instant>,
}

impl EngineRun {
    pub fn new(p: EngineRunParts) -> Self {
        let resume = crate::orchestrator::run_session(&p.session_id)
            .filter(|s| s.provider == p.provider_id)
            .map(|s| s.resume);
        Self {
            p,
            resume: std::sync::Mutex::new(resume),
            spent: std::sync::Mutex::new((0, 0.0)),
            cost_seen: AtomicBool::new(false),
        }
    }

    pub async fn run(&self, prompt: &str) -> Result<()> {
        let sid = self.p.session_id.clone();
        let name = parzi_providers::display_name(&self.p.provider_id);
        if !self.p.prompt_recorded {
            let text = user_event_text(prompt, &self.p.attachments);
            self.p.store.append(&sid, &Event::User { text })?;
        }
        self.p.store.set_status(&sid, SessionStatus::Active)?;
        tracing::info!(
            session = %sid,
            provider = %self.p.provider_id,
            model = self.p.model.as_deref().unwrap_or("default"),
            cwd = %self.p.cwd,
            "run started"
        );
        if !self.p.provider.gated() {
            self.note_once(format!(
                "{name} applies some changes without asking Parzi first, so file leases and \
                 the folder fence cannot stop them."
            ));
        }
        // Everything before the launch claimed the session is either this
        // run's opening or was answered already; later messages are news.
        let mut seen = self.p.seen;
        // This turn's own words, and what goes to the agent with them.
        let mut input = match self.p.inbox_from {
            // Started by a message from another session: every one not yet
            // handed to the agent, from there on, is this turn, so a sender
            // that raced the launch is answered too.
            Some(from) => {
                let start = self.p.slot.read_mark().unwrap_or(from);
                let (msgs, upto) = self.inbox(start, Some(seen));
                self.p.slot.set_read_mark(upto);
                if msgs.is_empty() {
                    prompt.to_string()
                } else {
                    msgs.join(
                        "

",
                    )
                }
            }
            None => prompt.to_string(),
        };
        let mut text = self.opening(&input);
        let mut turns = 0u32;
        let mut released = false;
        let mut restarted = false;
        let mut cap_noted = false;
        loop {
            turns += 1;
            // R-4: every turn costs, so the cap is checked before one is bought.
            if let Some(reason) = self.budget_hit() {
                self.pause_for_budget(&reason).await;
                return Ok(());
            }
            match self.turn(&text).await {
                Outcome::Completed => {}
                Outcome::Interrupted => {
                    self.finish(SessionStatus::Killed).await;
                    self.p.sink.emit(RunEvent::Error("cancelled".into()));
                    return Ok(());
                }
                Outcome::Paused(reason) => {
                    self.pause_for_budget(&reason).await;
                    return Ok(());
                }
                // The agent no longer has the conversation: once, start a
                // new one and hand it the thread so far.
                Outcome::Failed(e) if e.class == ErrorClass::SessionLost && !restarted => {
                    restarted = true;
                    tracing::info!(session = %sid, "vendor conversation lost: {}", e.message);
                    self.forget_session();
                    self.note(format!(
                        "{name} no longer had this conversation; it goes on in a new one, \
                         handed the thread so far."
                    ));
                    text = self.opening(&input);
                    continue;
                }
                Outcome::Failed(e) => {
                    tracing::warn!(
                        session = %sid,
                        provider = %self.p.provider_id,
                        class = e.class.as_str(),
                        "turn failed: {}",
                        e.message
                    );
                    let _ = self.p.store.append(
                        &sid,
                        &Event::Error {
                            message: e.message.clone(),
                            class: e.class.as_str().to_string(),
                        },
                    );
                    self.p.sink.emit(RunEvent::Error(e.message.clone()));
                    self.finish(SessionStatus::Idle).await;
                    return Err(ParziError::Provider(self.p.provider_id.clone(), e.message));
                }
            }
            if !cap_noted
                && self.p.budget.max_cost_usd.is_some()
                && !self.cost_seen.load(Ordering::Relaxed)
            {
                cap_noted = true;
                self.note(format!(
                    "{name} reports no dollar cost for this run (a plan, or no price), so the \
                     dollar cap cannot stop it. The token cap still does."
                ));
            }
            // H-5: messages other sessions sent while this turn ran are the
            // next turn, as the untrusted data they are. Looked for under
            // the lock senders deliver under: with none, the run settles and
            // lets go of the session before any sender can see it live.
            let (fresh, upto) = self
                .p
                .slot
                .last_look(
                    || self.inbox(seen, None),
                    || self.settle(SessionStatus::Done),
                )
                .await;
            seen = upto;
            if fresh.is_empty() {
                released = true;
                break;
            }
            if turns >= MAX_TURNS {
                self.note(format!(
                    "Stopped after {MAX_TURNS} turns in one run: {} message(s) from other \
                     sessions are left unanswered. Send a message to continue.",
                    fresh.len()
                ));
                break;
            }
            input = fresh.join("\n\n");
            text = input.clone();
        }
        if !released {
            self.finish(SessionStatus::Done).await;
        }
        tracing::info!(session = %sid, turns, "run done");
        self.p.sink.emit(RunEvent::Done { turns });
        Ok(())
    }

    /// Messages from other sessions in the transcript from index `from` to
    /// `until` (or the end), as the untrusted data they are, and the index
    /// read up to.
    fn inbox(&self, from: usize, until: Option<usize>) -> (Vec<String>, usize) {
        let events = self.p.store.events(&self.p.session_id).unwrap_or_default();
        let end = until.map_or(events.len(), |u| u.min(events.len()));
        let msgs = events[from.min(end)..end]
            .iter()
            .filter_map(|e| match e {
                Event::System { text } => InterSessionMessage::decode(text).map(|m| m.render()),
                _ => None,
            })
            .collect();
        (msgs, end)
    }

    /// The vendor's conversation is gone: the next turn opens a new one.
    fn forget_session(&self) {
        if let Ok(mut r) = self.resume.lock() {
            *r = None;
        }
        set_run_session(&self.p.session_id, None);
    }

    /// Say something in the thread: kept in the transcript, shown live.
    fn note(&self, text: String) {
        let _ = self
            .p
            .store
            .append(&self.p.session_id, &Event::System { text: text.clone() });
        self.p.sink.emit(RunEvent::Notice { text });
    }

    /// A note the thread needs once, not on every run.
    fn note_once(&self, text: String) {
        let said = self.p.store.events(&self.p.session_id).is_ok_and(|evs| {
            evs.iter()
                .any(|e| matches!(e, Event::System { text: t } if *t == text))
        });
        if !said {
            self.note(text);
        }
    }

    /// The first turn's text: earlier conversation when the vendor has none
    /// of it yet (a thread from before this provider, a fork), text
    /// attachments inline, then the prompt.
    fn opening(&self, prompt: &str) -> String {
        let mut parts = vec![];
        let fresh_session = self.resume.lock().map(|r| r.is_none()).unwrap_or(true);
        if fresh_session {
            if let Some(h) = self.history(prompt) {
                parts.push(h);
            }
        }
        for a in self.p.attachments.iter().filter(|a| !a.is_image()) {
            parts.push(format!(
                "<file path=\"{}\">\n{}\n</file>",
                a.path, a.snippet
            ));
        }
        parts.push(prompt.to_string());
        parts.join("\n\n")
    }

    /// The thread so far, newest last, as plain lines — without the turn
    /// being sent now.
    fn history(&self, prompt: &str) -> Option<String> {
        let events = self.p.store.events(&self.p.session_id).ok()?;
        let mut lines: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                Event::User { text } => Some(format!("user: {text}")),
                Event::Assistant { text, .. } => Some(format!("assistant: {text}")),
                Event::Checkpoint { summary } => {
                    Some(format!("summary of earlier turns: {summary}"))
                }
                _ => None,
            })
            .collect();
        // This turn's own prompt is already in the transcript; it goes out
        // as the prompt, not as history.
        let own = format!("user: {}", user_event_text(prompt, &self.p.attachments));
        if lines.last() == Some(&own) {
            lines.pop();
        }
        if !lines.iter().any(|l| l.starts_with("assistant: ")) {
            return None;
        }
        let mut kept: Vec<String> = vec![];
        let mut size = 0;
        for line in lines.into_iter().rev() {
            size += line.len();
            if size > HISTORY_CHARS {
                break;
            }
            kept.push(line);
        }
        kept.reverse();
        Some(format!(
            "<earlier-conversation>\nThis thread started before you joined it. What was said so far:\n{}\n</earlier-conversation>",
            kept.join("\n")
        ))
    }

    fn images(&self) -> Vec<PathBuf> {
        let base = PathBuf::from(&self.p.cwd);
        self.p
            .attachments
            .iter()
            .filter(|a| a.is_image())
            .map(|a| {
                let p = PathBuf::from(&a.path);
                if p.is_absolute() {
                    p
                } else {
                    base.join(p)
                }
            })
            .collect()
    }

    async fn turn(&self, text: &str) -> Outcome {
        let registration = self.p.mcp.as_ref().map(|m| m.register(self.p.host.clone()));
        let spec = TurnSpec {
            session_id: self.p.session_id.clone(),
            cwd: PathBuf::from(&self.p.cwd),
            model: self.p.model.clone(),
            effort: self.p.effort.clone(),
            instructions: Some(self.p.instructions.clone()).filter(|i| !i.trim().is_empty()),
            resume: self.resume.lock().ok().and_then(|r| r.clone()),
            prompt: text.to_string(),
            images: self.images(),
            tools: registration.as_ref().map(|r| r.server.clone()),
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let turn_cancel = self.p.cancel.child_token();
        let task = {
            let provider = self.p.provider.clone();
            let gate: Arc<dyn PermissionGate> = self.p.host.clone();
            let cancel = turn_cancel.clone();
            tokio::spawn(async move { provider.run_turn(spec, gate, tx, cancel).await })
        };
        let mut turn = Turn::default();
        let mut paused: Option<String> = None;
        while let Some(ev) = rx.recv().await {
            if let Some(reason) = self.on_event(ev, &mut turn).await {
                if paused.is_none() {
                    paused = Some(reason);
                    turn_cancel.cancel();
                }
            }
        }
        let result = task
            .await
            .unwrap_or_else(|e| Err(ProviderError::process(format!("the provider stopped: {e}"))));
        drop(registration);
        let completed = paused.is_none() && matches!(result, Ok(TurnEnd::Completed));
        self.flush(&mut turn, completed);
        if !completed {
            self.keep_partial(&mut turn);
        }
        if let Some(reason) = paused {
            return Outcome::Paused(reason);
        }
        match result {
            Ok(TurnEnd::Completed) => Outcome::Completed,
            Ok(TurnEnd::Interrupted) => Outcome::Interrupted,
            Err(_) if self.p.cancel.is_cancelled() => Outcome::Interrupted,
            Err(e) => Outcome::Failed(e),
        }
    }

    /// Write the held message; `done` marks it as the turn's answer.
    fn flush(&self, turn: &mut Turn, done: bool) {
        if let Some(text) = turn.pending.take() {
            let _ = self
                .p
                .store
                .append(&self.p.session_id, &Event::Assistant { text, done });
        }
    }

    /// Words streamed after the last finished message, on a stop or failure.
    fn keep_partial(&self, turn: &mut Turn) {
        let partial = std::mem::take(&mut turn.partial);
        if !partial.trim().is_empty() {
            let _ = self.p.store.append(
                &self.p.session_id,
                &Event::Assistant {
                    text: partial,
                    done: false,
                },
            );
        }
    }

    /// One provider event. `Some(reason)` = the budget ran out.
    async fn on_event(&self, ev: ProviderEvent, turn: &mut Turn) -> Option<String> {
        let sid = &self.p.session_id;
        match ev {
            ProviderEvent::Session { resume } => {
                if let Ok(mut r) = self.resume.lock() {
                    *r = Some(resume.clone());
                }
                set_run_session(
                    sid,
                    Some(VendorSession {
                        provider: self.p.provider_id.clone(),
                        resume,
                    }),
                );
            }
            ProviderEvent::TextDelta(t) => {
                turn.partial.push_str(&t);
                self.p.sink.emit(RunEvent::Text(t));
            }
            ProviderEvent::ReasoningDelta(text) => {
                self.p.sink.emit(RunEvent::Reasoning { text });
            }
            ProviderEvent::Message(text) => {
                self.flush(turn, false);
                turn.pending = Some(text);
                turn.partial.clear();
            }
            ProviderEvent::Reasoning(text) => {
                let _ = self.p.store.append(sid, &Event::Reasoning { text });
            }
            ProviderEvent::ToolStarted { id, name, input } => {
                self.flush(turn, false);
                self.p.host.tool_started(&id, &name).await;
                turn.started.insert(id.clone(), Instant::now());
                let shown = display_name(&name);
                let label = humanize_tool_call(&name, &input);
                let _ = self.p.store.append(
                    sid,
                    &Event::ToolCall {
                        id: id.clone(),
                        name: shown.clone(),
                        args: input,
                    },
                );
                self.p.sink.emit(RunEvent::ToolCall {
                    id,
                    name: shown,
                    label,
                });
            }
            ProviderEvent::ToolFinished {
                id,
                name,
                ok,
                output,
            } => {
                let (ok, output) = match self.p.host.tool_finished(&id).await {
                    Some(refusal) => (false, format!("{refusal}\n\n{output}")),
                    None => (ok, output),
                };
                let ms = turn.started.remove(&id).map_or(0, |t| {
                    t.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
                });
                let shown = display_name(&name);
                let output: String = output.chars().take(OUTPUT_CHARS).collect();
                let _ = self.p.store.append(
                    sid,
                    &Event::ToolResult {
                        id: id.clone(),
                        name: shown.clone(),
                        ok,
                        output,
                        ms,
                    },
                );
                self.p.sink.emit(RunEvent::ToolResult {
                    id,
                    name: shown,
                    ok,
                    ms,
                });
            }
            ProviderEvent::Usage {
                input,
                output,
                cost_usd,
            } => {
                if cost_usd.is_some() {
                    self.cost_seen.store(true, Ordering::Relaxed);
                }
                let cost = cost_usd.unwrap_or(0.0);
                let _ = self.p.store.add_usage(sid, input, output, cost);
                self.p.sink.emit(RunEvent::Usage {
                    tokens_in: input,
                    tokens_out: output,
                    cost_usd: cost,
                });
                let spent = self.spent.lock().ok().map(|mut s| {
                    s.0 = s.0.saturating_add(input.saturating_add(output));
                    s.1 += cost;
                    *s
                })?;
                if !self.p.budget.is_unlimited() {
                    return self.p.budget.exceeded(spent.0, spent.1);
                }
            }
            ProviderEvent::Context { used, limit } => {
                let _ = self.p.store.set_context(sid, used, limit);
                self.p.sink.emit(RunEvent::Context { used, limit });
            }
            ProviderEvent::Limits(windows) => {
                self.p.status.update_usage(&self.p.provider_id, &windows);
            }
            ProviderEvent::Notice(text) => self.note(text),
        }
        None
    }

    /// The reason this run must pause, or `None` while inside the budget.
    fn budget_hit(&self) -> Option<String> {
        if self.p.budget.is_unlimited() {
            return None;
        }
        let (tokens, cost) = self.spent.lock().ok().map(|s| *s)?;
        self.p.budget.exceeded(tokens, cost)
    }

    /// Budget stop: say it in the timeline, mark the session, park it Idle.
    /// Never silent, never a kill — the thread continues once the cap moves.
    async fn pause_for_budget(&self, reason: &str) {
        let text = format!(
            "Paused: {reason}. Raise the budget in Settings or PROJECT.md, or re-scope, \
             then send the thread on."
        );
        let _ = self
            .p
            .store
            .append(&self.p.session_id, &Event::System { text: text.clone() });
        crate::orchestrator::set_run_note(
            &self.p.session_id,
            Some(crate::orchestrator::BUDGET_NOTE),
        );
        self.p.sink.emit(RunEvent::Notice { text });
        self.finish(SessionStatus::Idle).await;
    }

    /// R-1: a killed run stays killed. Whatever the run wanted to write, a
    /// cancelled token (or a session already marked Killed) wins.
    async fn finish(&self, status: SessionStatus) {
        self.settle(status);
    }

    fn settle(&self, status: SessionStatus) {
        let killed = self.p.cancel.is_cancelled()
            || matches!(
                self.p.store.get(&self.p.session_id).map(|m| m.status),
                Ok(SessionStatus::Killed)
            );
        let status = if killed {
            SessionStatus::Killed
        } else {
            status
        };
        let _ = self.p.store.set_status(&self.p.session_id, status);
    }
}
