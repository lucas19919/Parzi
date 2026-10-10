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

const MAX_TURNS: u32 = 8;
const HISTORY_CHARS: usize = 12_000;
const OUTPUT_CHARS: usize = 20_000;

pub struct EngineRunParts {
    pub session_id: String,
    pub provider_id: String,
    pub provider: Arc<dyn Provider>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub instructions: String,
    pub cwd: String,
    pub attachments: Vec<AttachedFile>,
    pub store: SessionStore,
    pub host: Arc<ToolHost>,
    pub mcp: Option<Arc<McpHost>>,
    pub status: Arc<StatusBoard>,
    pub sink: RunSink,
    pub cancel: CancellationToken,
    pub budget: Budget,
    pub prompt_recorded: bool,
    pub(crate) slot: SessionSlot,
    pub seen: usize,
    pub inbox_from: Option<usize>,
}

pub struct EngineRun {
    p: EngineRunParts,
    resume: std::sync::Mutex<Option<Value>>,
    spent: std::sync::Mutex<(u64, f64)>,
    cost_seen: AtomicBool,
    // Token estimate for providers that never emit Usage (else caps never trip).
    usage_seen: AtomicBool,
    est_tokens: std::sync::Mutex<u64>,
}

enum Outcome {
    Completed,
    Interrupted,
    Paused(String),
    Failed(ProviderError),
}

#[derive(Default)]
struct Turn {
    pending: Option<String>,
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
            usage_seen: AtomicBool::new(false),
            est_tokens: std::sync::Mutex::new(0),
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
        let mut seen = self.p.seen;
        let mut input = match self.p.inbox_from {
            Some(from) => {
                let start = self.p.slot.read_mark().unwrap_or(from);
                let (msgs, upto) = self.inbox(start, Some(seen));
                self.p.slot.set_read_mark(upto);
                if msgs.is_empty() {
                    prompt.to_string()
                } else {
                    msgs.join("\n\n")
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
            if let Some(reason) = self.budget_hit() {
                self.pause_for_budget(&reason);
                return Ok(());
            }
            match self.turn(&text).await {
                Outcome::Completed => {}
                Outcome::Interrupted => {
                    self.settle(SessionStatus::Killed);
                    self.p.sink.emit(RunEvent::Error("cancelled".into()));
                    return Ok(());
                }
                Outcome::Paused(reason) => {
                    self.pause_for_budget(&reason);
                    return Ok(());
                }
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
                    self.settle(SessionStatus::Idle);
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
            self.settle(SessionStatus::Done);
        }
        tracing::info!(session = %sid, turns, "run done");
        self.p.sink.emit(RunEvent::Done { turns });
        Ok(())
    }

    fn inbox(&self, from: usize, until: Option<usize>) -> (Vec<String>, usize) {
        let total = self.p.store.event_count(&self.p.session_id).unwrap_or(0);
        let end = until.map_or(total, |u| u.min(total));
        let from = from.min(end);
        let events = self
            .p
            .store
            .events_from(&self.p.session_id, from)
            .unwrap_or_default();
        let msgs = events
            .iter()
            .take(end - from)
            .filter_map(|e| match e {
                Event::System { text } => InterSessionMessage::decode(text).map(|m| m.render()),
                _ => None,
            })
            .collect();
        (msgs, end)
    }

    fn forget_session(&self) {
        if let Ok(mut r) = self.resume.lock() {
            *r = None;
        }
        set_run_session(&self.p.session_id, None);
    }

    fn note(&self, text: String) {
        let _ = self
            .p
            .store
            .append(&self.p.session_id, &Event::System { text: text.clone() });
        self.p.sink.emit(RunEvent::Notice { text });
    }

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
                // Replacements must also see tool calls and results, not just chat.
                Event::ToolCall { name, args, .. } => {
                    let a = args.to_string();
                    let a: String = a.split_whitespace().collect::<Vec<_>>().join(" ");
                    let a: String = a.chars().take(200).collect();
                    Some(format!("tool_call: {name} {a}"))
                }
                Event::ToolResult {
                    name, ok, output, ..
                } => {
                    let out: String = output.chars().take(500).collect();
                    Some(format!("tool_result({name}, ok={ok}): {out}"))
                }
                _ => None,
            })
            .collect();
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
            if let Some(reason) = self.on_event(ev, &mut turn) {
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

    fn flush(&self, turn: &mut Turn, done: bool) {
        if let Some(text) = turn.pending.take() {
            let _ = self
                .p
                .store
                .append(&self.p.session_id, &Event::Assistant { text, done });
        }
    }

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

    fn on_event(&self, ev: ProviderEvent, turn: &mut Turn) -> Option<String> {
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
                self.count_estimate(&t);
                self.p.sink.emit(RunEvent::Text(t));
            }
            ProviderEvent::ReasoningDelta(text) => {
                self.count_estimate(&text);
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
                self.usage_seen.store(true, Ordering::Relaxed);
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

    // ~4 chars/token estimate so silent providers still trip budget caps.
    fn count_estimate(&self, text: &str) {
        if self.usage_seen.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut est) = self.est_tokens.lock() {
            *est = est.saturating_add((text.chars().count() as u64).div_ceil(4));
        }
    }

    fn budget_hit(&self) -> Option<String> {
        if self.p.budget.is_unlimited() {
            return None;
        }
        let (tokens, cost) = self.spent.lock().ok().map(|s| *s)?;
        // Real usage wins once seen; the estimate covers silent providers only.
        let est = if self.usage_seen.load(Ordering::Relaxed) {
            0
        } else {
            self.est_tokens.lock().ok().map(|e| *e).unwrap_or(0)
        };
        self.p.budget.exceeded(tokens.max(est), cost)
    }

    fn pause_for_budget(&self, reason: &str) {
        let text = format!(
            "Paused: {reason}. Raise the budget in Settings, or re-scope, \
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
        self.settle(SessionStatus::Idle);
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
