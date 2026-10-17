use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use parzi_core::config::Budget;
use parzi_core::context::{AttachedFile, InterSessionMessage};
use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Check, Event, RunCause, RunEndKind, SessionStatus, SessionStore};
use parzi_core::text::{clip, fold};
use parzi_providers::{
    ErrorClass, PermissionGate, Provider, ProviderError, ProviderEvent, TurnEnd, TurnSpec,
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::handler::{user_event_text, RunEvent, RunSink};
use crate::mcp_host::McpHost;
use crate::orchestrator::{set_run_session, SessionSlot, VendorSession};
use crate::run_hooks::Tally;
use crate::status::StatusBoard;
use crate::toolhost::ToolHost;
use crate::tools::{display_name, edit_paths, humanize_tool_call, shell_command};

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
    pub(crate) tally: Tally,
    /// The project's plan (`<parzi:plan>`), for the opening turn.
    pub brief: Option<String>,
}

pub struct EngineRun {
    p: EngineRunParts,
    resume: std::sync::Mutex<Option<Value>>,
    spent: std::sync::Mutex<(u64, f64)>,
    cost_seen: AtomicBool,
    // Token estimate for providers that never emit Usage (else caps never trip).
    usage_seen: AtomicBool,
    est_tokens: std::sync::Mutex<u64>,
    /// The hash of the plan this run's opening carried, kept on the session
    /// once a turn with it completed (a turn that failed never showed it).
    brief_sent: std::sync::Mutex<Option<String>>,
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
            brief_sent: std::sync::Mutex::new(None),
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
        let mut input = self.first_input(prompt, seen);
        let mut text = self.opening(&input);
        let mut turns = 0u32;
        let mut restarted = false;
        let mut cap_noted = false;
        loop {
            turns += 1;
            if let Some(reason) = self.budget_hit() {
                self.pause_for_budget(&reason).await;
                return Ok(());
            }
            match self.turn(&text).await {
                Outcome::Completed => self.brief_seen(),
                Outcome::Interrupted => {
                    self.tally(|t| t.end = Some((RunEndKind::Stopped, None)));
                    self.end(SessionStatus::Killed).await;
                    self.p.sink.emit(RunEvent::Error("cancelled".into()));
                    return Ok(());
                }
                Outcome::Paused(reason) => {
                    self.pause_for_budget(&reason).await;
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
                    self.tally(|t| t.end = Some((RunEndKind::Failed, Some(e.message.clone()))));
                    self.end(SessionStatus::Idle).await;
                    self.p.sink.emit(RunEvent::Error(e.message.clone()));
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
            let (mut fresh, mut upto) = self.inbox(seen, None);
            if fresh.is_empty() {
                // Quiet: the outcome (git's look included) is kept while the
                // run still holds its slot, so nothing new starts on the
                // session before it is on record and a Stop cannot catch a
                // run that has finished. Then the last look, under the lock
                // that deliveries take, settles it and gives the slot back.
                self.tally(|t| t.end = Some((RunEndKind::Completed, None)));
                self.record().await;
                (fresh, upto) = self
                    .p
                    .slot
                    .last_look(
                        || self.inbox(seen, None),
                        || self.settle(SessionStatus::Done),
                    )
                    .await;
                if fresh.is_empty() {
                    tracing::info!(session = %sid, turns, "run done");
                    self.p.sink.emit(RunEvent::Done { turns });
                    return Ok(());
                }
                // A message came while the outcome was kept: it is answered,
                // and the outcome kept again when the run ends.
                self.tally(|t| {
                    t.end = None;
                    t.recorded = false;
                });
            }
            if turns >= MAX_TURNS {
                // Left unread, not lost: the next run of this session reads
                // on from the first of them.
                self.p.slot.set_read_mark(seen);
                self.note(format!(
                    "Stopped after {MAX_TURNS} turns in one run: {} message(s) from other \
                     sessions are left unanswered. Send a message to continue.",
                    fresh.len()
                ));
                self.tally(|t| t.end = Some((RunEndKind::Completed, None)));
                self.end(SessionStatus::Done).await;
                tracing::info!(session = %sid, turns, "run stopped at the turn cap");
                self.p.sink.emit(RunEvent::Done { turns });
                return Ok(());
            }
            self.p.slot.set_read_mark(upto);
            seen = upto;
            input = fresh.join("\n\n");
            text = input.clone();
        }
    }

    /// What the first turn answers: a message run's messages (or its
    /// prompt when they were read already); otherwise the prompt, after any
    /// messages an earlier run left unanswered (see `MAX_TURNS`).
    fn first_input(&self, prompt: &str, seen: usize) -> String {
        let Some(start) = self.p.slot.read_mark().or(self.p.inbox_from) else {
            return prompt.to_string();
        };
        let (msgs, upto) = self.inbox(start, Some(seen));
        self.p.slot.set_read_mark(upto);
        match (msgs.is_empty(), self.p.inbox_from.is_some()) {
            (true, _) => prompt.to_string(),
            (false, true) => msgs.join("\n\n"),
            (false, false) => format!("{}\n\n{prompt}", msgs.join("\n\n")),
        }
    }

    fn tally(&self, f: impl FnOnce(&mut RunTally)) {
        f(&mut crate::run_hooks::lock(&self.p.tally));
    }

    /// A run that ends here: its outcome kept, then its status, with the
    /// slot given back in the same step (the window hears of it after).
    async fn end(&self, status: SessionStatus) {
        self.record().await;
        self.p.slot.release(|| self.settle(status)).await;
    }

    /// The run's outcome kept on its session, before the window hears it
    /// ended: what it reads then is already this run's.
    async fn record(&self) {
        let stopped = self.p.cancel.is_cancelled();
        crate::run_hooks::run_ended(&self.p.store, &self.p.tally, stopped, true).await;
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
        if let Some(b) = self.brief(fresh_session) {
            parts.push(b);
        }
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

    /// The plan, unless this conversation has seen this exact text: a fresh
    /// one never has; a resumed one saw what run.json's hash says. Never
    /// written to the transcript, and never part of the instructions (those
    /// are re-sent every turn and a changing plan would spoil their cache).
    fn brief(&self, fresh: bool) -> Option<String> {
        let text = self.p.brief.as_ref()?;
        let hash = parzi_core::hash::fnv_hex(text.as_bytes());
        let sid = &self.p.session_id;
        if !fresh && crate::orchestrator::brief_hash(sid).as_deref() == Some(hash.as_str()) {
            return None;
        }
        if let Ok(mut sent) = self.brief_sent.lock() {
            *sent = Some(hash);
        }
        Some(text.clone())
    }

    /// A turn completed: the plan it carried is the conversation's now.
    fn brief_seen(&self) {
        let sent = self.brief_sent.lock().ok().and_then(|mut s| s.take());
        if let Some(hash) = sent {
            crate::orchestrator::set_brief_hash(&self.p.session_id, Some(&hash));
        }
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
                    let a: String = fold(&args.to_string()).chars().take(200).collect();
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
                crate::run_hooks::tool_started(&self.p.tally, &id, &shown, &input);
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
                self.tally(|t| t.tool_finished(&id, ok));
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
                self.tally(|t| t.usage(cost_usd));
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

    async fn pause_for_budget(&self, reason: &str) {
        let text = format!(
            "Paused: {reason}. Send again to go on, or raise the spending limit."
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
        self.tally(|t| t.end = Some((RunEndKind::Paused, Some(reason.to_string()))));
        self.end(SessionStatus::Idle).await;
        // The UI's running set clears on this, as it does on done.
        self.p.sink.emit(RunEvent::Paused {
            reason: reason.to_string(),
        });
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

/// Whether a command line builds, tests or lints: its result is a check.
#[must_use]
pub(crate) fn is_check(cmd: &str) -> bool {
    cmd.split([';', '|', '&', '\n'])
        .any(|part| check_words(part.split_whitespace().collect()))
}

fn check_words(mut words: Vec<&str>) -> bool {
    // `FOO=1 npx tsc`, `python -m pytest`, `uv run pytest`: look past the wrapper.
    while let Some(first) = words.first().copied() {
        let env = first.contains('=') && !first.starts_with('-');
        let module = matches!(first, "python" | "python3" | "py") && words.get(1) == Some(&"-m");
        let runner = matches!(first, "uv" | "poetry" | "pipenv") && words.get(1) == Some(&"run");
        let plain = matches!(first, "npx" | "bunx" | "pnpx" | "time" | "sudo");
        let skip = if module || runner {
            2
        } else if env || plain {
            1
        } else {
            break;
        };
        words.drain(..skip.min(words.len()));
    }
    let Some(tool) = words.first() else {
        return false;
    };
    let tool = tool
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(tool)
        .trim_end_matches(".exe")
        .trim_end_matches(".cmd")
        .trim_end_matches(".bat");
    let rest = &words[1..];
    match tool {
        "cargo" => matches!(
            subcommand(rest),
            Some("test" | "build" | "check" | "clippy" | "fmt" | "nextest")
        ),
        "go" => matches!(subcommand(rest), Some("test" | "build" | "vet")),
        "dotnet" => matches!(subcommand(rest), Some("test" | "build")),
        "npm" | "pnpm" | "yarn" | "bun" => package_script_check(rest),
        // `make` alone builds the default target; `make clean` does not.
        "make" => match subcommand(rest) {
            None => true,
            Some(target) => target == "all" || checks_by_name(target),
        },
        "pytest" | "py.test" | "tsc" | "eslint" | "ruff" | "mvn" | "gradle" | "gradlew"
        | "vitest" | "jest" | "unittest" => true,
        _ => false,
    }
}

/// Names a script or target that builds, tests or lints.
fn checks_by_name(name: &str) -> bool {
    ["test", "build", "check", "lint", "tsc"]
        .iter()
        .any(|k| name.contains(k))
}

/// The first word that is not a flag (`-j8`, `+nightly`), skipping the
/// value of the flags that take one (`--prefix ui`, `-C dir`).
fn subcommand<'a>(rest: &[&'a str]) -> Option<&'a str> {
    const TAKES_VALUE: &[&str] = &[
        "--prefix",
        "-C",
        "--dir",
        "--cwd",
        "--filter",
        "-F",
        "--workspace",
        "--manifest-path",
        "-p",
        "--package",
    ];
    let mut words = rest.iter();
    while let Some(w) = words.next() {
        if TAKES_VALUE.contains(w) {
            words.next();
        } else if !w.starts_with('-') && !w.starts_with('+') {
            return Some(w);
        }
    }
    None
}

/// `npm test`, `npm t`, `npm run build`, `pnpm lint`, `yarn test:unit`:
/// the test command or a script named for a check. Installs (`pnpm add -D
/// vitest`, `npm i jest`) are not checks, whatever they install.
fn package_script_check(rest: &[&str]) -> bool {
    let Some(sub) = subcommand(rest) else {
        return false;
    };
    match sub {
        "test" | "t" | "tst" => true,
        "run" | "run-script" => {
            let after = rest
                .iter()
                .position(|w| *w == sub)
                .map_or(rest, |i| &rest[i + 1..]);
            subcommand(after).is_some_and(checks_by_name)
        }
        "exec" | "dlx" | "x" => {
            let after = rest
                .iter()
                .position(|w| *w == sub)
                .map_or(rest, |i| &rest[i + 1..]);
            check_words(after.to_vec())
        }
        "add" | "i" | "install" | "ci" | "remove" | "rm" | "uninstall" | "update" | "up"
        | "upgrade" | "link" | "publish" | "init" | "create" => false,
        // pnpm, yarn and bun run a script by its bare name.
        script => checks_by_name(script),
    }
}

/// A command shown as a check: one line, at most 120 chars.
fn check_label(cmd: &str) -> String {
    clip(&fold(cmd), 120)
}

/// A file path as the run's folder sees it: relative inside it, with
/// forward slashes; anything outside stays absolute.
#[must_use]
pub(crate) fn display_path(cwd: &str, path: &str) -> String {
    let p = path.trim().replace('\\', "/");
    let c = cwd.trim().replace('\\', "/");
    let c = c.trim_end_matches('/');
    let inside = (!c.is_empty())
        .then(|| parzi_core::paths::strip_prefix_fold(&p, c)?.strip_prefix('/'))
        .flatten();
    inside
        .unwrap_or_else(|| p.trim_start_matches("./"))
        .to_string()
}

/// What a run did, gathered while it runs: the session's `last_run` comes
/// from this when the run ends.
pub(crate) struct RunTally {
    pub(crate) session: String,
    pub(crate) cause: RunCause,
    pub(crate) cwd: String,
    /// Where the run's events start in the transcript.
    pub(crate) first_event: usize,
    pub(crate) started: Instant,
    edits: Vec<String>,
    shells: HashMap<String, String>,
    pub(crate) checks: Vec<Check>,
    pub(crate) cost: Option<f64>,
    /// How it ended (and the error), set by the run itself.
    pub(crate) end: Option<(RunEndKind, Option<String>)>,
    pub(crate) git: Option<tokio::task::JoinHandle<Option<crate::gitmark::GitMark>>>,
    /// Its outcome is on the session: the run's guard need not keep it.
    pub(crate) recorded: bool,
}

impl RunTally {
    pub(crate) fn new(session: &str, cause: RunCause, cwd: &str) -> Self {
        Self {
            session: session.to_string(),
            cause,
            cwd: cwd.to_string(),
            first_event: 0,
            started: Instant::now(),
            edits: vec![],
            shells: HashMap::new(),
            checks: vec![],
            cost: None,
            end: None,
            git: None,
            recorded: false,
        }
    }

    /// Counts a call: the files an edit names, and a check it runs.
    pub(crate) fn tool_started(&mut self, id: &str, name: &str, input: &Value) {
        for p in edit_paths(name, input) {
            let shown = display_path(&self.cwd, &p);
            if !self
                .edits
                .iter()
                .any(|e| parzi_core::paths::same_path(e, &shown))
            {
                self.edits.push(shown);
            }
        }
        if let Some(cmd) = shell_command(name, input).filter(|c| is_check(c)) {
            self.shells.insert(id.to_string(), check_label(&cmd));
        }
    }

    pub(crate) fn tool_finished(&mut self, id: &str, ok: bool) {
        if let Some(cmd) = self.shells.remove(id) {
            self.checks.push(Check { cmd, ok });
        }
    }

    pub(crate) fn usage(&mut self, cost: Option<f64>) {
        if let Some(c) = cost {
            self.cost = Some(self.cost.unwrap_or(0.0) + c);
        }
    }

    /// Files the run's tool calls edited, relative to its folder.
    pub(crate) fn edits(&self) -> &[String] {
        &self.edits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tally() -> RunTally {
        RunTally::new("s1", RunCause::Send, r"D:\Repos\app")
    }

    #[test]
    fn edits_from_claude_and_codex_are_named_once() {
        let mut t = tally();
        t.tool_started("1", "Edit", &json!({"file_path": r"D:\Repos\app\src\a.rs"}));
        t.tool_started(
            "2",
            "Write",
            &json!({"file_path": "D:/Repos/app/README.md"}),
        );
        t.tool_started("3", "MultiEdit", &json!({"file_path": "src/a.rs"}));
        t.tool_started("4", "NotebookEdit", &json!({"notebook_path": "nb/x.ipynb"}));
        t.tool_started("5", "edit", &json!({"paths": ["src/b.rs", "./src/c.rs"]}));
        t.tool_started("6", "Read", &json!({"file_path": "src/z.rs"}));
        t.tool_started("7", "Write", &json!({"file_path": "C:/elsewhere/y.txt"}));
        t.tool_started(
            "8",
            "Run create_file?",
            &json!({"TargetFile": "D:/Repos/app/notes.txt", "CodeContent": "hello"}),
        );
        t.tool_started(
            "9",
            "Running edit_file",
            &json!({"target_file": "src/d.rs"}),
        );
        assert_eq!(
            t.edits(),
            [
                "src/a.rs",
                "README.md",
                "nb/x.ipynb",
                "src/b.rs",
                "src/c.rs",
                "C:/elsewhere/y.txt",
                "notes.txt",
                "src/d.rs"
            ]
        );
    }

    #[test]
    fn checks_are_the_builds_and_tests_and_how_they_went() {
        for cmd in [
            "cargo test -p parzi-core",
            "cd ui && npm run check",
            "npm test",
            "pnpm build",
            "PARZI_HOME=/tmp cargo clippy --workspace",
            "python -m pytest tests/",
            "npx tsc --noEmit",
            "go test ./...",
            "make",
            "./gradlew build",
            "dotnet test",
            "uv run pytest",
        ] {
            assert!(is_check(cmd), "{cmd}");
        }
        for cmd in [
            "ls -la",
            "git status",
            "npm install",
            "cargo run",
            "echo test",
            "cat Makefile",
            "pnpm add -D vitest",
            "npm i @testing-library/react",
            "yarn add jest",
            "make clean",
            "make install",
            "npm run dev",
            "cargo add serde --features test-util",
            "go run ./cmd/test",
        ] {
            assert!(!is_check(cmd), "{cmd}");
        }
        for cmd in [
            "npm t",
            "npm run test:unit",
            "npm --prefix ui run build",
            "yarn lint",
            "pnpm -C ui typecheck",
            "make test",
            "make -j8",
            "cargo +nightly fmt --check",
            "pnpm exec vitest run",
        ] {
            assert!(is_check(cmd), "{cmd}");
        }
        let mut t = tally();
        t.tool_started("1", "shell.exec", &json!({"cmd": "cargo  test\n"}));
        t.tool_started("2", "shell", &json!({"command": ["npm", "test"]}));
        t.tool_started("3", "Bash", &json!({"command": "ls"}));
        t.tool_finished("2", false);
        t.tool_finished("1", true);
        t.tool_finished("3", true);
        assert_eq!(
            t.checks,
            [
                Check {
                    cmd: "npm test".into(),
                    ok: false
                },
                Check {
                    cmd: "cargo test".into(),
                    ok: true
                },
            ]
        );
    }

    #[test]
    fn usage_adds_up_and_cost_stays_unknown_until_reported() {
        let mut t = tally();
        t.usage(None);
        assert_eq!(t.cost, None);
        t.usage(Some(0.25));
        t.usage(Some(0.25));
        assert_eq!(t.cost, Some(0.5));
    }
}
