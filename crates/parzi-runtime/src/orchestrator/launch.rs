use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Event, SessionMeta, SessionStatus};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::handler::{system_parts, HarnessBridge, RunEvent, RunSink};
use crate::run::{EngineRun, EngineRunParts};
use crate::toolhost::{ToolHost, ToolHostParts};
use crate::tools::{ApprovalMode, Approver, DenyApprover, ToolExecutor};
use parzi_providers::display_name;

use super::queue::{clear_queued, run_session, set_run_note, Pump, QueuedRun};
use super::{next_run_id, normalize_effort, Handle, Orchestrator, SessionSlot};

impl Orchestrator {
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn(
        &self,
        project: &str,
        lane: &str,
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
        let title: String = prompt
            .lines()
            .next()
            .unwrap_or("untitled")
            .chars()
            .take(80)
            .collect();

        let mut meta = self.store.create(&title, project, lane, model_spec)?;
        if !cwd.trim().is_empty() {
            self.store.set_cwd(&meta.id, cwd)?;
        }

        let q = QueuedRun {
            session_id: meta.id.clone(),
            project: project.to_string(),
            lane: lane.to_string(),
            model_spec: model_spec.to_string(),
            prompt: prompt.to_string(),
            cwd: cwd.to_string(),
            effort,
            attachments,
            approver,
            prompt_recorded: false,
            inbox_from: None,
            mode_override,
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
        let rx = Self::launch(self.pump_parts(), q).await?;
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
        {
            let mut h = self.handles.lock().await;
            h.retain(|_, handle| !handle.finished());
            if h.contains_key(id) {
                return Err(ParziError::Store(format!(
                    "run {id} is active; kill it first"
                )));
            }
        }
        let meta = self.store.get(id)?;
        let spec = model_override
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| meta.model.clone());
        let effort = normalize_effort(effort);
        let q = QueuedRun {
            session_id: id.to_string(),
            project: meta.project.clone(),
            lane: meta.lane.clone(),
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
            prompt_recorded: false,
            inbox_from: None,
            mode_override,
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
            self.store.set_status(id, SessionStatus::Queued)?;
            self.pump_parts().enqueue(q).await;
            return Ok(Self::closed_rx());
        }
        Self::launch(self.pump_parts(), q).await
    }

    pub async fn compact(&self, id: &str, focus: &str) -> Result<String> {
        let provider = run_session(id).map(|s| s.provider).ok_or_else(|| {
            ParziError::Store("nothing to compact yet: this thread has no conversation".into())
        })?;
        if !matches!(provider.as_str(), "claude" | "opencode") {
            return Err(ParziError::Store(format!(
                "{} compacts its conversation on its own",
                display_name(&provider)
            )));
        }
        let prompt = format!("/compact {}", focus.trim()).trim_end().to_string();
        let _rx = self
            .send_to(
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
        session_id: &str,
        spec: &str,
    ) -> Result<(String, Option<String>)> {
        let bound = run_session(session_id).map(|s| s.provider);
        // No automatic routing. An empty spec (or a legacy "auto" one) keeps
        // a bound thread on its own provider, else uses the default model
        // the human picked in Settings. Nothing guesses a provider.
        let legacy_auto =
            spec.trim().is_empty() || spec == "auto" || spec.starts_with("auto/");
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
                    match parzi_providers::split_spec(cfg.default_model.trim()) {
                        Some((id, model)) => (id.to_string(), model),
                        None => {
                            return Err(ParziError::Store(
                                "No default model set. Pick one in Settings → Providers, or choose a model in the composer."
                                    .into(),
                            ))
                        }
                    }
                }
            }
        };
        if let Some(b) = bound.filter(|b| *b != provider) {
            return Err(ParziError::Store(format!(
                "This thread's conversation lives on {}. Start a new thread to use {}.",
                display_name(&b),
                display_name(&provider)
            )));
        }
        let entry = cfg.provider(&provider);
        if !entry.enabled {
            return Err(ParziError::Store(format!(
                "{} is switched off in Settings → Providers.",
                display_name(&provider)
            )));
        }
        let model = model.or_else(|| Some(entry.default_model).filter(|m| !m.trim().is_empty()));
        Ok((provider, model))
    }

    fn lane_policy_for(cfg: &ParziConfig, lane: &str) -> (ApprovalMode, Vec<String>) {
        let mode = ApprovalMode::parse(&cfg.lanes.default_mode);
        let mut allowed = cfg.lanes.default_allowed_tools.clone();
        for u in [
            "ui.show_markdown",
            "ui.show_artifact",
            "browser.open",
            "browser.tabs",
            "browser.read",
            "browser.click",
            "browser.type",
            "browser.shot",
            "image.generate",
            "doc.read",
            "models.list",
            "ask.user",
            "plan.write",
            "plan.read",
            "shell.exec",
            "shell.start",
            "shell.logs",
            "shell.kill",
            "brain.search",
            "brain.read",
            "brain.list",
            "brain.write",
        ] {
            if !allowed.contains(&u.to_string()) {
                allowed.push(u.into());
            }
        }
        // Work staffs its own read/write workers (a paper is a project);
        // only shell execution and cross-lane dispatch stay Build-only.
        // (Spawns still go through the permission mode like any other tool.)
        for u in [
            "session.spawn",
            "session.send_message",
            "session.read_session",
            "session.list_sessions",
            "project.create",
        ] {
            if !allowed.contains(&u.to_string()) {
                allowed.push(u.into());
            }
        }
        if lane != "work" {
            if !allowed.contains(&"lane.dispatch".to_string()) {
                allowed.push("lane.dispatch".into());
            }
        } else {
            allowed.retain(|t| !t.starts_with("shell."));
        }
        (mode, allowed)
    }

    fn restrict_mode(a: ApprovalMode, b: ApprovalMode) -> ApprovalMode {
        use ApprovalMode::{Ask, Auto, Deny};
        match (a, b) {
            (Deny, _) | (_, Deny) => Deny,
            (Ask, _) | (_, Ask) => Ask,
            (Auto, Auto) => Auto,
        }
    }

    pub(super) async fn launch(p: Pump, q: QueuedRun) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        let sid = q.session_id.clone();
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
                let seen = p.store.events(&sid).map_or(0, |e| e.len());
                Some((id, seen))
            }
        };
        let Some((id, seen)) = claim else {
            if q.inbox_from.is_none() {
                p.enqueue(q).await;
            }
            return Ok(Self::closed_rx());
        };
        match Self::launch_inner(p.clone(), q, id, cancel, seen).await {
            Ok(rx) => Ok(rx),
            Err(e) => {
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
        let (provider_id, model) = Self::route(&snap, &q.session_id, &q.model_spec).await?;
        let spec_empty =
            q.model_spec.trim().is_empty() || q.model_spec == "auto" || q.model_spec.starts_with("auto/");
        let mut model = model;
        if q.lane == "work" && spec_empty && model.is_none() {
            model = Some(snap.quick_model.clone()).filter(|m| !m.trim().is_empty());
        }
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
        } else {
            q.cwd.clone()
        };
        let (mut mode, allowed) = Self::lane_policy_for(&snap, &q.lane);
        let mut edits_auto = false;
        let mut full = false;
        if let Some(o) = q.mode_override.as_deref() {
            if ApprovalMode::is_full_override(Some(o)) {
                mode = ApprovalMode::Auto;
                full = true;
            } else {
                mode = Self::restrict_mode(mode, ApprovalMode::parse(o));
                edits_auto = o.trim() == "edits";
            }
        }
        let tools = Arc::new(ToolExecutor {
            cwd: cwd.clone(),
            mcp: p.mcp.clone(),
            allowed,
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let sink = RunSink::new(&q.session_id, tx, Some(p.bus.clone()));
        let bridge: Arc<dyn HarnessBridge> = Arc::new(p.clone());
        let instructions = system_parts(&q.lane, &cwd);
        if mode == ApprovalMode::Deny && !provider.gated() {
            return Err(ParziError::Validation(format!(
                "this lane is read-only, and {} cannot run read-only; pick another agent for it",
                display_name(&provider_id)
            )));
        }
        let host = Arc::new(ToolHost::new(ToolHostParts {
            session_id: q.session_id.clone(),
            lane: q.lane.clone(),
            mode,
            edits_auto,
            full,
            cfg: snap.clone(),
            store: p.store.clone(),
            tools,
            approver: q.approver.clone().unwrap_or_else(|| Arc::new(DenyApprover)),
            asker: p.asker.lock().await.clone(),
            shell: p.shells_for(&q.session_id),
            harness: Some(bridge),
            sink: sink.clone(),
            cancel: cancel.clone(),
        }));
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
            sink,
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
        });
        set_run_note(&q.session_id, None);
        p.store.set_status(&q.session_id, SessionStatus::Active)?;
        let end = RunEnd {
            parts: p.clone(),
            sid: q.session_id.clone(),
            id,
        };
        let prompt = q.prompt.clone();
        let task = tokio::spawn(async move {
            let _end = end;
            let _ = run.run(&prompt).await;
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
}

impl Drop for RunEnd {
    fn drop(&mut self) {
        let (p, sid, id) = (self.parts.clone(), std::mem::take(&mut self.sid), self.id);
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
            p.notify.notify_one();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_policy_offers_the_page_and_not_the_crew() {
        let cfg = ParziConfig::default();
        let (_mode, allowed) = Orchestrator::lane_policy_for(&cfg, "work");
        for t in [
            "browser.open",
            "browser.tabs",
            "browser.read",
            "browser.click",
            "browser.type",
            "browser.shot",
            "image.generate",
            "doc.read",
            "models.list",
            "ui.show_artifact",
            "brain.search",
            "brain.write",
            "session.spawn",
            "session.send_message",
            "session.read_session",
            "session.list_sessions",
            "project.create",
        ] {
            assert!(allowed.iter().any(|a| a == t), "lane missing {t}");
        }
        assert!(allowed.iter().all(|a| a != "lane.dispatch"));
    }

    #[test]
    fn both_lanes_orchestrate_but_only_build_dispatches() {
        let cfg = ParziConfig::default();
        for lane in ["build", "code", ""] {
            let (_, allowed) = Orchestrator::lane_policy_for(&cfg, lane);
            for t in [
                "session.spawn",
                "session.send_message",
                "session.read_session",
                "session.list_sessions",
                "lane.dispatch",
            ] {
                assert!(allowed.iter().any(|a| a == t), "{lane} lane missing {t}");
            }
        }
        let (_, allowed) = Orchestrator::lane_policy_for(&cfg, "work");
        for t in ["session.spawn", "session.send_message", "project.create"] {
            assert!(
                allowed.iter().any(|a| a == t),
                "work lane missing {t}"
            );
        }
        assert!(
            allowed.iter().all(|a| a != "lane.dispatch"),
            "cross-lane dispatch stays Build-only"
        );
    }

    #[test]
    fn work_lane_is_stripped_of_shell() {
        let cfg = ParziConfig::default();
        let (_, build) = Orchestrator::lane_policy_for(&cfg, "build");
        for t in ["shell.exec", "shell.start", "shell.logs", "shell.kill"] {
            assert!(build.iter().any(|a| a == t), "build lane missing {t}");
        }
        let (_, research) = Orchestrator::lane_policy_for(&cfg, "work");
        for t in ["shell.exec", "shell.start", "shell.logs", "shell.kill"] {
            assert!(
                research.iter().all(|a| a != t),
                "work lane must not offer {t}"
            );
        }
    }

    #[test]
    fn mode_floor_never_lifts_only_tightens() {
        use crate::tools::ApprovalMode::{Ask, Auto, Deny};
        assert_eq!(Orchestrator::restrict_mode(Auto, Auto), Auto);
        assert_eq!(Orchestrator::restrict_mode(Auto, Ask), Ask);
        assert_eq!(Orchestrator::restrict_mode(Ask, Auto), Ask);
        assert_eq!(Orchestrator::restrict_mode(Auto, Deny), Deny);
        assert_eq!(Orchestrator::restrict_mode(Deny, Auto), Deny);
        assert_eq!(Orchestrator::restrict_mode(Ask, Ask), Ask);
    }
}
