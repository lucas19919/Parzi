use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Event, RunCause, RunEndKind, SessionMeta, SessionStatus, SessionStore};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::handler::{system_parts_for, HarnessBridge, RunEvent, RunSink};
use crate::run::{EngineRun, EngineRunParts, RunTally};
use crate::toolhost::{ToolHost, ToolHostParts};
use crate::tools::{ApprovalMode, Approver, DenyApprover, ToolExecutor};
use parzi_providers::display_name;

use super::queue::{
    clear_queued, run_session, set_posture, set_run_note, switch_agent, Posture, Pump, QueuedRun,
};
use super::{next_run_id, normalize_effort, Handle, Orchestrator, SessionSlot};

pub(super) enum Started {
    Running(mpsc::UnboundedReceiver<RunEvent>),
    Queued,
}

impl Started {
    fn events(self) -> mpsc::UnboundedReceiver<RunEvent> {
        match self {
            Self::Running(rx) => rx,
            Self::Queued => Orchestrator::closed_rx(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ContextParts {
    pub instructions: u64,
    pub notes: u64,
    pub tools: u64,
}

const LAUNCH_WAIT: std::time::Duration = std::time::Duration::from_secs(90);

impl Orchestrator {
    pub fn context_parts(&self, cwd: &str) -> ContextParts {
        let tokens = |chars: usize| (chars as u64).div_ceil(4);
        let notes = parzi_core::brain::context_for(std::path::Path::new(cwd.trim()))
            .map_or(0, |c| tokens(c.chars));
        let all: usize = crate::handler::system_parts(cwd)
            .iter()
            .map(|p| p.chars().count())
            .sum();
        let (_, allowed) = Self::tool_policy(&self.config());
        let tools: usize = crate::tools::all_defs()
            .iter()
            .filter(|t| allowed.iter().any(|a| a == &t.name))
            .map(|t| t.name.len() + t.description.chars().count() + t.schema.to_string().len())
            .sum();
        ContextParts {
            instructions: tokens(all).saturating_sub(notes),
            notes,
            tools: tokens(tools),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn spawn(
        &self,
        project: &str,
        model_spec: &str,
        prompt: &str,
        approver: Option<Arc<dyn Approver>>,
        cwd: &str,
        effort: &str,
        attachments: Vec<parzi_core::context::AttachedFile>,
        mode_override: Option<String>,
    ) -> Result<(SessionMeta, mpsc::UnboundedReceiver<RunEvent>)> {
        self.spawn_as(
            RunCause::Send,
            project,
            model_spec,
            prompt,
            approver,
            cwd,
            effort,
            attachments,
            mode_override,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn spawn_as(
        &self,
        cause: RunCause,
        project: &str,
        model_spec: &str,
        prompt: &str,
        approver: Option<Arc<dyn Approver>>,
        cwd: &str,
        effort: &str,
        attachments: Vec<parzi_core::context::AttachedFile>,
        mode_override: Option<String>,
    ) -> Result<(SessionMeta, mpsc::UnboundedReceiver<RunEvent>)> {
        let live = {
            let mut h = self.handles.lock().await;
            h.retain(|_, handle| !handle.finished());
            h.len()
        };
        let effort = normalize_effort(effort);
        let title = parzi_core::text::name_from_prompt(prompt);

        let mut meta = self.store.create(&title, project, model_spec)?;
        if !cwd.trim().is_empty() {
            self.store.set_cwd(&meta.id, cwd)?;
        }

        let q = QueuedRun {
            session_id: meta.id.clone(),
            project: project.to_string(),
            model_spec: model_spec.to_string(),
            prompt: prompt.to_string(),
            cwd: cwd.to_string(),
            effort,
            attachments,
            approver,
            restore_approver: false,
            prompt_recorded: false,
            inbox_from: None,
            mode_override,
            cause,
        };
        let snap = self.config();
        if live >= snap.orchestrator.max_concurrent.max(1) {
            if !snap.orchestrator.queue_when_busy {
                return Err(ParziError::Store(format!(
                    "busy: {live} runs active (max {}); kill one first",
                    snap.orchestrator.max_concurrent
                )));
            }
            self.store.set_status(&meta.id, SessionStatus::Queued)?;
            self.pump_parts().enqueue(q).await;
            meta = self.store.get(&meta.id)?;
            return Ok((meta, Self::closed_rx()));
        }
        let rx = Self::launch(self.pump_parts(), q).await?.events();
        meta = self.store.get(&meta.id)?;
        Ok((meta, rx))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_to(
        &self,
        id: &str,
        prompt: &str,
        approver: Option<Arc<dyn Approver>>,
        cwd: &str,
        effort: &str,
        attachments: Vec<parzi_core::context::AttachedFile>,
        model_override: Option<String>,
        mode_override: Option<String>,
    ) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        self.send_as(
            RunCause::Send,
            id,
            prompt,
            approver,
            cwd,
            effort,
            attachments,
            model_override,
            mode_override,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send_as(
        &self,
        cause: RunCause,
        id: &str,
        prompt: &str,
        approver: Option<Arc<dyn Approver>>,
        cwd: &str,
        effort: &str,
        attachments: Vec<parzi_core::context::AttachedFile>,
        model_override: Option<String>,
        mode_override: Option<String>,
    ) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        let opts = SendOpts::new(cwd, effort, model_override, mode_override);
        self.send_inner(cause, id, prompt, approver, attachments, opts, false)
            .await
    }

    /// Sends the session's last message again, to the agent picked now (a
    /// retry after a usage limit, say). It is already in the thread, so it is
    /// not recorded a second time. Errors when the session has no message.
    #[allow(clippy::too_many_arguments)]
    pub async fn resend_last(
        &self,
        id: &str,
        approver: Option<Arc<dyn Approver>>,
        cwd: &str,
        effort: &str,
        attachments: Vec<parzi_core::context::AttachedFile>,
        model_override: Option<String>,
        mode_override: Option<String>,
    ) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        let last = self
            .store
            .events(id)?
            .into_iter()
            .rev()
            .find_map(|e| match e {
                Event::User { text } => Some(text),
                _ => None,
            })
            .ok_or_else(|| {
                ParziError::Validation("this session has no message to send again".into())
            })?;
        let prompt = strip_attached(&last);
        let opts = SendOpts::new(cwd, effort, model_override, mode_override);
        self.send_inner(
            RunCause::Send,
            id,
            &prompt,
            approver,
            attachments,
            opts,
            true,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_inner(
        &self,
        cause: RunCause,
        id: &str,
        prompt: &str,
        approver: Option<Arc<dyn Approver>>,
        attachments: Vec<parzi_core::context::AttachedFile>,
        opts: SendOpts<'_>,
        recorded: bool,
    ) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        let SendOpts {
            cwd,
            effort,
            model_override,
            mode_override,
        } = opts;
        let meta = self.store.get(id)?;
        // A person's message is their answer to a waiting plan proposal,
        // whichever way it came (the window, the CLI, a phone): the card goes.
        if cause.from_person() && meta.proposal.is_some() {
            let _ = self.store.set_proposal(id, None);
        }
        if cause.from_person() && meta.status == SessionStatus::Queued {
            tracing::info!(
                session = %id,
                status = meta.status.word(),
                "queue: composer holds the send (still working); no PersistedRun"
            );
            return Err(ParziError::Store(format!("still working on {id}")));
        }
        {
            let mut h = self.handles.lock().await;
            h.retain(|_, handle| !handle.finished());
            if h.contains_key(id) {
                if cause.from_person() {
                    tracing::info!(
                        session = %id,
                        "queue: composer holds the send (still working); a run is live"
                    );
                    return Err(ParziError::Store(format!("still working on {id}")));
                }
                return Err(ParziError::Store(format!(
                    "run {id} is active; kill it first"
                )));
            }
            if meta.status == SessionStatus::Active {
                // Nothing is running it: a status write was lost, so the
                // session must not refuse every message from here on.
                tracing::warn!(session = %id, "session read active with no run behind it; reset");
                self.store.set_status(id, SessionStatus::Idle)?;
            }
        }
        // A draft starts with this send. Its brief always reaches the agent:
        // a message of the person's own goes after it, not instead of it.
        let mut prompt = prompt.to_string();
        if meta.status == SessionStatus::Draft {
            if let Some(brief) = meta
                .brief
                .as_deref()
                .map(str::trim)
                .filter(|b| !b.is_empty())
            {
                if !prompt.contains(brief) {
                    prompt = if prompt.trim().is_empty() {
                        brief.to_string()
                    } else {
                        format!("{brief}\n\n{prompt}")
                    };
                }
            }
            self.store.take_brief(id)?;
        }
        let prompt = prompt.as_str();
        let spec = model_override
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| meta.model.clone());
        let effort = normalize_effort(effort);
        let q = QueuedRun {
            session_id: id.to_string(),
            project: meta.project.clone(),
            model_spec: spec,
            prompt: prompt.to_string(),
            cwd: if cwd.is_empty() {
                meta.cwd.clone()
            } else {
                cwd.to_string()
            },
            effort,
            attachments,
            approver,
            restore_approver: false,
            prompt_recorded: recorded,
            inbox_from: None,
            mode_override,
            cause,
        };
        let live = {
            let mut h = self.handles.lock().await;
            h.retain(|_, handle| !handle.finished());
            h.len()
        };
        let snap = self.config();
        if live >= snap.orchestrator.max_concurrent.max(1) {
            if !snap.orchestrator.queue_when_busy {
                return Err(ParziError::Store(
                    "busy: max concurrent runs reached".into(),
                ));
            }
            tracing::info!(
                session = %id,
                "queue: PersistedRun (at the concurrent limit; the launch has not started)"
            );
            self.store.set_status(id, SessionStatus::Queued)?;
            self.pump_parts().enqueue(q).await;
            return Ok(Self::closed_rx());
        }
        Ok(Self::launch(self.pump_parts(), q).await?.events())
    }

    pub async fn compact(&self, id: &str, focus: &str) -> Result<String> {
        let provider = run_session(id).map(|s| s.provider).ok_or_else(|| {
            ParziError::Store("nothing to compact yet: this thread has no conversation".into())
        })?;
        let prompt = focus.trim().to_string();
        let _rx = self
            .send_as(
                RunCause::Compact,
                id,
                &prompt,
                Some(Arc::new(DenyApprover)),
                "",
                "medium",
                vec![],
                None,
                None,
            )
            .await?;
        Ok(format!(
            "{} is compacting the conversation",
            display_name(&provider)
        ))
    }

    async fn route(
        cfg: &ParziConfig,
        store: &SessionStore,
        session_id: &str,
        spec: &str,
        prompt_recorded: bool,
    ) -> Result<(String, Option<String>)> {
        let bound = run_session(session_id).map(|s| s.provider);
        let legacy_auto = spec.trim().is_empty() || spec == "auto" || spec.starts_with("auto/");
        let (provider, model) = match parzi_providers::split_spec(spec) {
            Some((id, model)) => (id.to_string(), model),
            None if !legacy_auto => {
                return Err(ParziError::Store(format!(
                    "`{spec}` is not a provider Parzi runs (claude, codex, opencode, grok, antigravity, cursor)"
                )))
            }
            None => {
                if let Some(b) = &bound {
                    (b.clone(), None)
                } else {
                    return Err(ParziError::Store(
                        "No model chosen. Pick one in the composer before sending.".into(),
                    ));
                }
            }
        };
        let entry = cfg.provider(&provider);
        if !entry.enabled {
            return Err(ParziError::Store(format!(
                "{} is switched off in Settings → Providers.",
                display_name(&provider)
            )));
        }
        // Another agent takes the thread over. The old agent's own session is
        // parked for when the thread comes back to it; the new agent resumes
        // its own parked session if it has one (hearing what happened while
        // it was away), else starts fresh with the transcript replayed
        // (EngineRun::history). The thread stays one Parzi session.
        // This runs whenever the agent is not the one bound, even with no
        // binding: an agent that never kept a session still leaves the parked
        // one of the agent coming back. A message queued for the new agent is
        // already on record, so the returning agent hears it too.
        if bound.as_deref() != Some(provider.as_str()) {
            tracing::info!(session = %session_id, from = ?bound, to = %provider, "agent switch: the thread carries over");
            let events_now = store.event_count(session_id).unwrap_or(0);
            let left_at = events_now.saturating_sub(usize::from(prompt_recorded));
            switch_agent(session_id, &provider, left_at);
        }
        let model = model.or_else(|| Some(entry.default_model).filter(|m| !m.trim().is_empty()));
        Ok((provider, model))
    }

    fn tool_policy(cfg: &ParziConfig) -> (ApprovalMode, Vec<String>) {
        let mode = ApprovalMode::parse(&cfg.lanes.default_mode);
        let mut allowed = cfg.lanes.default_allowed_tools.clone();
        for u in [
            "ui.show_artifact",
            "ui.show_file",
            "ui.offer_replies",
            "models.list",
            "ask.user",
            "shell.exec",
            "shell.start",
            "shell.logs",
            "shell.kill",
            "wait",
            "brain.search",
            "brain.read",
            "brain.write",
            "brain.delete",
            "project.create",
            "project.switch",
            "project.archive",
            "project.delete",
            "project.plan",
            "plan.read",
            "plan.write",
            "session.rename",
            "session.format_message",
            "session.spawn",
            "session.send",
            "session.read",
            "session.list",
            "session.draft",
        ] {
            if !allowed.contains(&u.to_string()) {
                allowed.push(u.into());
            }
        }
        if cfg.lanes.no_shell {
            allowed.retain(|t| !t.starts_with("shell."));
        }
        for u in crate::tools::DESK_TOOLS {
            if !allowed.iter().any(|a| a == u) {
                allowed.push((*u).to_string());
            }
        }
        if super::headless() {
            allowed.retain(|t| !t.starts_with("browser.") && !t.starts_with("desk."));
        }
        // Every enabled connector; each tool's own choice and the session's
        // approval mode still decide whether a call runs.
        let mut connectors: Vec<String> = cfg
            .mcp
            .servers
            .iter()
            .filter(|(_, s)| s.enabled)
            .map(|(name, _)| crate::tools::connector_entry(name))
            .collect();
        connectors.sort();
        allowed.extend(connectors);
        (mode, allowed)
    }

    fn effective_mode(default: ApprovalMode, choice: Option<&str>) -> (ApprovalMode, bool, bool) {
        match choice {
            None => (default, false, false),
            Some(_) if default == ApprovalMode::Deny => (ApprovalMode::Deny, false, false),
            Some(o) if ApprovalMode::is_full_override(Some(o)) => (ApprovalMode::Auto, false, true),
            Some(o) => {
                let mode = ApprovalMode::parse(o);
                (
                    mode,
                    o.trim() == "edits" && mode != ApprovalMode::Deny,
                    false,
                )
            }
        }
    }

    pub(super) async fn launch(p: Pump, q: QueuedRun) -> Result<Started> {
        let sid = q.session_id.clone();
        let cause = q.cause;
        clear_queued(&sid);
        let cancel = CancellationToken::new();
        let claim = {
            let mut h = p.handles.lock().await;
            h.retain(|_, handle| !handle.finished());
            if h.contains_key(&sid) {
                None
            } else {
                let id = next_run_id();
                h.insert(
                    sid.clone(),
                    Handle {
                        id,
                        cancel: cancel.clone(),
                        task: None,
                    },
                );
                let seen = p.store.event_count(&sid).unwrap_or(0);
                Some((id, seen))
            }
        };
        let Some((id, seen)) = claim else {
            if q.cause.from_person() {
                tracing::info!(
                    session = %sid,
                    "queue: lost the claim; composer holds the send (still working)"
                );
                return Err(ParziError::Store(format!("still working on {sid}")));
            }
            if q.inbox_from.is_none() && p.cfg_snapshot().orchestrator.queue_when_busy {
                tracing::info!(
                    session = %sid,
                    "queue: PersistedRun (internal launch lost the claim)"
                );
                p.enqueue(q).await;
                return Ok(Started::Queued);
            }
            return Err(ParziError::Store(format!(
                "session {sid} already has a live run; kill it first"
            )));
        };
        let started = tokio::time::timeout(
            LAUNCH_WAIT,
            Self::launch_inner(p.clone(), q, id, cancel, seen),
        )
        .await
        .unwrap_or_else(|_| {
            Err(ParziError::Store(format!(
                "the run did not start within {}s",
                LAUNCH_WAIT.as_secs()
            )))
        });
        match started {
            Ok(rx) => Ok(Started::Running(rx)),
            Err(e) => {
                tracing::warn!(session = %sid, "run failed to start: {e}");
                {
                    let mut h = p.handles.lock().await;
                    if h.get(&sid).is_some_and(|x| x.id == id) {
                        h.remove(&sid);
                    }
                }
                p.store.set_status(&sid, SessionStatus::Idle)?;
                p.store.append(
                    &sid,
                    &Event::System {
                        text: format!("run failed to start: {e}"),
                    },
                )?;
                crate::run_hooks::launch_failed(&p.store, &sid, cause, &e.to_string());
                let _ = p.bus.send((sid, RunEvent::Error(e.to_string())));
                Err(e)
            }
        }
    }

    async fn launch_inner(
        p: Pump,
        q: QueuedRun,
        id: u64,
        cancel: CancellationToken,
        seen: usize,
    ) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        let snap = p.cfg_snapshot();
        let (provider_id, model) = Self::route(
            &snap,
            &p.store,
            &q.session_id,
            &q.model_spec,
            q.prompt_recorded,
        )
        .await?;
        let provider = (p.source)(&provider_id, &snap).ok_or_else(|| {
            ParziError::Store(format!("{provider_id} is not on this build's roster"))
        })?;
        let shown = match &model {
            Some(m) => format!("{provider_id}/{m}"),
            None => provider_id.clone(),
        };
        if p.store.get(&q.session_id)?.model != shown {
            p.store.set_model(&q.session_id, &shown)?;
        }
        let cwd = if q.cwd.trim().is_empty() {
            let dir = parzi_core::paths::scratch_dir(&q.session_id)?;
            std::fs::create_dir_all(&dir)?;
            dir.display().to_string()
        } else if std::path::Path::new(q.cwd.trim()).is_absolute() {
            q.cwd.clone()
        } else {
            return Err(ParziError::Validation(format!(
                "the folder `{}` is not a full path; pick the folder again",
                q.cwd.trim()
            )));
        };
        let (start, allowed) = Self::tool_policy(&snap);
        let (mode, edits_auto, full) = Self::effective_mode(start, q.mode_override.as_deref());
        let tools = Arc::new(ToolExecutor {
            cwd: cwd.clone(),
            mcp: p.mcp.clone(),
            allowed,
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let sink = RunSink::new(&q.session_id, tx, Some(p.bus.clone()));
        let tally = {
            let mut t = RunTally::new(&q.session_id, q.cause, &cwd);
            t.first_event = seen;
            Arc::new(std::sync::Mutex::new(t))
        };
        let bridge: Arc<dyn HarnessBridge> = Arc::new(p.clone());
        let project = p
            .store
            .get(&q.session_id)
            .ok()
            .map(|m| m.project)
            .filter(|p| !p.is_empty() && p != "default");
        let mut instructions = system_parts_for(&cwd, project.as_deref());
        if provider_id != "claude" {
            if let Some(files) =
                parzi_core::system::claude_files_for_others(std::path::Path::new(&cwd))
            {
                instructions.push(format!("# Repository instructions\n\n{files}"));
            }
        }
        if let Some(note) = p.mcp.status_note() {
            instructions.push(note);
        }
        if let Ok(dir) = parzi_core::paths::scratch_dir(&q.session_id) {
            if std::path::Path::new(&cwd) != dir.as_path() && std::fs::create_dir_all(&dir).is_ok()
            {
                instructions.push(format!(
                    "# Scratch\n\nYour scratch folder is {}: yours for temporary files, notes and experiments that do not belong in the project. Writing there needs no approval. Never leave scratch files in the project.",
                    dir.display()
                ));
            }
        }
        if mode == ApprovalMode::Deny && !provider.gated() {
            return Err(ParziError::Validation(format!(
                "agents are locked to read-only in Settings, and {} cannot run read-only; pick another agent for it",
                display_name(&provider_id)
            )));
        }
        let parts = ToolHostParts {
            session_id: q.session_id.clone(),
            mode,
            edits_auto,
            full,
            cfg: snap.clone(),
            store: p.store.clone(),
            tools,
            approver: q
                .approver
                .clone()
                .or_else(|| q.restore_approver.then(|| p.host_approver()).flatten())
                .unwrap_or_else(|| Arc::new(DenyApprover)),
            asker: p.asker.lock().await.clone(),
            shell: p.shells_for(&q.session_id),
            harness: Some(bridge),
            sink: sink.clone(),
            cancel: cancel.clone(),
        };
        let host = Arc::new(ToolHost::new(parts).calling_as(&provider_id));
        let run = EngineRun::new(EngineRunParts {
            session_id: q.session_id.clone(),
            provider_id,
            provider,
            model,
            effort: Some(q.effort.clone()).filter(|e| !e.is_empty()),
            instructions: instructions.join("\n\n---\n\n"),
            cwd: cwd.clone(),
            attachments: q.attachments.clone(),
            store: p.store.clone(),
            host,
            mcp: p.tools_server().await,
            status: p.status.clone(),
            sink: sink.clone(),
            cancel: cancel.clone(),
            budget: snap.budget,
            prompt_recorded: q.prompt_recorded,
            slot: SessionSlot {
                handles: p.handles.clone(),
                marks: p.marks.clone(),
                sid: q.session_id.clone(),
                id,
            },
            seen,
            inbox_from: q.inbox_from,
            tally: tally.clone(),
            compact: q.cause == RunCause::Compact,
        });
        set_run_note(&q.session_id, None);
        if q.cause.recorded() {
            let _ = p.store.set_effort(&q.session_id, &q.effort);
            if q.cause != RunCause::Message || q.mode_override.is_some() {
                set_posture(
                    &q.session_id,
                    Posture {
                        mode_override: q.mode_override.clone(),
                        effort: q.effort.clone(),
                        ask: q.approver.is_some() || q.restore_approver,
                    },
                );
            }
        }
        p.store.set_status(&q.session_id, SessionStatus::Active)?;
        crate::run_hooks::run_started(&tally);
        let end = RunEnd {
            parts: p.clone(),
            sid: q.session_id.clone(),
            id,
            done: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancel: cancel.clone(),
            tally,
            sink,
        };
        let prompt = q.prompt.clone();
        let done = end.done.clone();
        let task = tokio::spawn(async move {
            let _end = end;
            match run.run(&prompt).await {
                Ok(()) => done.store(true, std::sync::atomic::Ordering::SeqCst),
                Err(e) => tracing::warn!("run could not go on: {e}"),
            }
        });
        if let Some(h) = p.handles.lock().await.get_mut(&q.session_id) {
            if h.id == id {
                h.task = Some(task);
            }
        }
        Ok(rx)
    }
}

struct RunEnd {
    parts: Pump,
    sid: String,
    id: u64,
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    cancel: CancellationToken,
    tally: crate::run_hooks::Tally,
    sink: RunSink,
}

impl Drop for RunEnd {
    fn drop(&mut self) {
        let (p, sid, id) = (self.parts.clone(), std::mem::take(&mut self.sid), self.id);
        let done = self.done.clone();
        let cancel = self.cancel.clone();
        let tally = self.tally.clone();
        let sink = self.sink.clone();
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        rt.spawn(async move {
            {
                let mut h = p.handles.lock().await;
                if h.get(&sid).is_some_and(|x| x.id == id) {
                    h.remove(&sid);
                }
            }
            if !done.load(std::sync::atomic::Ordering::SeqCst) && !cancel.is_cancelled() {
                let sentence = "run task ended without settling; transcript kept, nothing was sent";
                tracing::warn!(session = %sid, "{sentence}");
                if let Ok(m) = p.store.get(&sid) {
                    if m.status == SessionStatus::Active {
                        let _ = p.store.set_status(&sid, SessionStatus::Idle);
                        let _ = p.store.append(
                            &sid,
                            &Event::Error {
                                message: sentence.into(),
                                class: "harness".into(),
                            },
                        );
                    }
                }
                let mut t = crate::run_hooks::lock(&tally);
                if t.end.is_none() {
                    t.end = Some((RunEndKind::Interrupted, Some(sentence.into())));
                }
                drop(t);
                sink.emit(RunEvent::Error(sentence.into()));
            }
            p.notify.notify_one();
            let settled = done.load(std::sync::atomic::Ordering::SeqCst);
            let stopped = cancel.is_cancelled();
            crate::run_hooks::run_ended(&p.store, &tally, stopped, settled).await;
            p.report_to_parent(&sid, stopped).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_session_gets_every_tool_and_the_shell() {
        let cfg = ParziConfig::default();
        let (_mode, allowed) = Orchestrator::tool_policy(&cfg);
        for t in [
            "shell.exec",
            "shell.start",
            "shell.logs",
            "shell.kill",
            "wait",
            "browser.open",
            "browser.read",
            "browser.click",
            "ui.show_artifact",
            "session.format_message",
            "brain.search",
            "brain.write",
            "session.spawn",
            "session.send",
            "session.read",
            "session.list",
            "session.rename",
            "project.create",
            "plan.read",
            "plan.write",
            "ui.show_file",
        ] {
            assert!(allowed.iter().any(|a| a == t), "missing {t}");
        }
        assert!(
            allowed.iter().all(|a| !a.starts_with("lane.")),
            "no lanes left"
        );
    }

    #[test]
    fn enabled_connectors_join_the_session_and_off_ones_stay_out() {
        let mut cfg = ParziConfig::default();
        cfg.mcp.servers.insert(
            "shell".into(),
            parzi_core::config::McpServerCfg {
                command: "x".into(),
                enabled: true,
                ..Default::default()
            },
        );
        cfg.mcp.servers.insert(
            "linear".into(),
            parzi_core::config::McpServerCfg {
                url: "https://mcp.linear.app/mcp".into(),
                enabled: false,
                ..Default::default()
            },
        );
        cfg.lanes.no_shell = true;
        let (_, allowed) = Orchestrator::tool_policy(&cfg);
        let tools = ToolExecutor {
            cwd: String::new(),
            mcp: Arc::new(crate::mcp::McpManager::new(cfg.mcp.servers.clone(), 60)),
            allowed,
        };
        assert!(
            tools.connector_allowed("diagonaldiagrams"),
            "the default one"
        );
        assert!(tools.connector_allowed("shell"));
        assert!(!tools.connector_allowed("linear"), "switched off");
        assert!(
            !tools.is_allowed("shell.exec"),
            "a connector named shell does not bring the shell back"
        );
    }

    #[test]
    fn the_shell_switch_turns_shell_off_everywhere() {
        let mut cfg = ParziConfig::default();
        let (_, on) = Orchestrator::tool_policy(&cfg);
        assert!(
            on.iter().any(|t| t == "shell.exec"),
            "shell is on by default"
        );
        cfg.lanes.no_shell = true;
        let (_, off) = Orchestrator::tool_policy(&cfg);
        assert!(off.iter().all(|t| !t.starts_with("shell.")));
        assert!(off.iter().any(|t| t == "plan.write"), "the rest stays");
    }

    #[test]
    fn the_composer_choice_runs_and_only_lockdown_holds_over_it() {
        use crate::tools::ApprovalMode::{Ask, Auto, Deny};
        let eff = Orchestrator::effective_mode;
        assert_eq!(eff(Ask, Some("full")), (Auto, false, true));
        assert_eq!(eff(Auto, Some("full")), (Auto, false, true));
        assert_eq!(eff(Ask, Some("auto")), (Auto, false, false));
        assert_eq!(eff(Auto, Some("supervised")), (Ask, false, false));
        assert_eq!(eff(Ask, Some("edits")), (Ask, true, false));
        assert_eq!(eff(Auto, Some("deny")), (Deny, false, false));
        assert_eq!(eff(Deny, Some("full")), (Deny, false, false));
        assert_eq!(eff(Deny, Some("auto")), (Deny, false, false));
        assert_eq!(eff(Deny, Some("edits")), (Deny, false, false));
        assert_eq!(eff(Auto, None), (Auto, false, false));
        assert_eq!(eff(Ask, None), (Ask, false, false));
    }
}

/// How a send runs: the folder, effort, model and mode it was sent with.
struct SendOpts<'a> {
    cwd: &'a str,
    effort: &'a str,
    model_override: Option<String>,
    mode_override: Option<String>,
}

impl<'a> SendOpts<'a> {
    fn new(
        cwd: &'a str,
        effort: &'a str,
        model_override: Option<String>,
        mode_override: Option<String>,
    ) -> Self {
        Self {
            cwd,
            effort,
            model_override,
            mode_override,
        }
    }
}

/// A recorded message without the "[attached: ...]" line the engine adds:
/// the files go along as attachments instead.
fn strip_attached(text: &str) -> String {
    match text.rfind(
        "
[attached: ",
    ) {
        Some(at) if text[at..].trim_end().ends_with(']') => text[..at].to_string(),
        _ => text.to_string(),
    }
}
