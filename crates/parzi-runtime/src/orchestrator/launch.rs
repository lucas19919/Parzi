//! From a queued run to a live one: the provider chain it will talk to, the
//! lane policy it runs under, the tools it is armed with, and the driver task
//! that owns it until it ends.

use std::sync::Arc;

use parzi_core::config::{Budget, ParziConfig};
use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Event, SessionMeta, SessionStatus};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::handler::{system_parts, AgentRun, HarnessBridge, RunEvent};
use crate::lease_tools::LeaseCtx;
use crate::tools::{ApprovalMode, Approver, DenyApprover, ToolExecutor};

use super::queue::{
    clear_queued, run_project, run_role, set_run_note, set_run_project, Pump, QueuedRun,
};
use super::{effort_tokens, normalize_effort, sticky_spec, Handle, Orchestrator, ProviderFactory};

/// R-4: what a run may spend — the tighter of the machine's config budget and
/// the project's `budget:` line. A project the store cannot read (deleted,
/// mid-write) simply does not tighten anything.
fn budget_for(cfg: &ParziConfig, project: Option<&(String, String)>) -> Budget {
    let mut budget = cfg.budget;
    if let Some((workspace, slug)) = project {
        if let Ok(p) = parzi_core::project::load(workspace, slug) {
            budget = budget.tightest(Budget {
                max_cost_usd: p.budget_usd,
                max_tokens: None,
            });
        }
    }
    budget
}

impl Orchestrator {
    /// Spawn a run. Returns the session id + live event channel.
    #[allow(clippy::too_many_arguments)]
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
        self.spawn_in_project(
            None,
            project,
            lane,
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

    /// `spawn`, bound to a hub project `(workspace, slug)`: the run then also
    /// honours that project's `budget:` (R-4).
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn_in_project(
        &self,
        workspace_project: Option<(String, String)>,
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
        // B4: prune finished tasks before measuring capacity.
        let live = {
            let mut h = self.handles.lock().await;
            h.retain(|_, handle| !handle._task.is_finished());
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
        meta.cwd = cwd.to_string();
        // R-4: remember the project on the session, so its later turns keep
        // the project budget without every caller repeating it.
        if workspace_project.is_some() {
            set_run_project(&meta.id, workspace_project.clone());
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
            workspace_project,
            prompt_recorded: false,
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

    /// Continue an existing session with a new user message.
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
        // B4: a finished run must not block its own session. Prune first so a
        // second message to a completed thread succeeds.
        {
            let mut h = self.handles.lock().await;
            h.retain(|_, handle| !handle._task.is_finished());
            if h.contains_key(id) {
                return Err(ParziError::Store(format!(
                    "run {id} is active; kill it first"
                )));
            }
        }
        let meta = self.store.get(id)?;
        // Per-message model switch: the thread follows the newly picked model.
        // A requested `auto` sticks to the previously resolved route.
        let requested = model_override.unwrap_or_else(|| meta.model.clone());
        let spec = sticky_spec(&requested, &meta.model);
        if spec != meta.model {
            self.store.set_model(id, &spec)?;
        }
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
            workspace_project: run_project(id),
            prompt_recorded: false,
            mode_override,
        };
        let live = {
            let mut h = self.handles.lock().await;
            h.retain(|_, handle| !handle._task.is_finished());
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

    /// Compact a thread on request: the thread's model summarizes it (falling
    /// over like a run would), the summary lands as a `Checkpoint`, and the
    /// next message is read from there. Refused while the thread is running.
    pub async fn compact(&self, id: &str, focus: &str) -> Result<String> {
        {
            let mut h = self.handles.lock().await;
            h.retain(|_, handle| !handle._task.is_finished());
            if h.contains_key(id) {
                return Err(ParziError::Store(
                    "this thread is running; compact it once the run ends".into(),
                ));
            }
        }
        let meta = self.store.get(id)?;
        let events = self.store.events(id)?;
        if !parzi_core::context::compactable(&events) {
            return Err(ParziError::Store("nothing to compact yet".into()));
        }
        let mut last = ParziError::Store("no working provider for this thread's model".into());
        for slot in self.slots_for(&meta.model, "low")? {
            if matches!(
                slot.provider.auth_status(),
                parzi_providers::AuthStatus::Missing(_)
            ) {
                continue;
            }
            let limit = AgentRun::lookup_context_limit(&slot.provider_id, &slot.model_id);
            match crate::compact::summarize(
                slot.provider.as_ref(),
                &slot.model_id,
                &events,
                focus,
                limit,
                id,
            )
            .await
            {
                Ok(summary) => {
                    self.store.append(
                        id,
                        &Event::Checkpoint {
                            summary: summary.clone(),
                        },
                    )?;
                    let used = parzi_core::context::ContextBuilder::estimate(&summary);
                    self.store.set_context(id, used, limit)?;
                    let _ = self
                        .bus
                        .send((id.to_string(), RunEvent::Context { used, limit }));
                    return Ok(summary);
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Build the provider chain: `auto` expands via the smart router,
    /// an explicit spec is a single slot. Never empty on success.
    fn slots_for(
        &self,
        model_spec: &str,
        effort: &str,
    ) -> Result<Vec<crate::handler::ProviderSlot>> {
        let snap = self.config();
        Self::slots_for_static(&snap, &self.factory, model_spec, effort)
    }

    fn slots_for_static(
        cfg: &ParziConfig,
        factory: &ProviderFactory,
        model_spec: &str,
        effort: &str,
    ) -> Result<Vec<crate::handler::ProviderSlot>> {
        use crate::handler::ProviderSlot;
        // Catalog prices are API-key rates; a subscription credential bills
        // flat, so its slot costs 0 and the cost meter stays honest.
        let priced = |provider: &dyn parzi_providers::Provider, pid: &str, mid: &str| {
            if provider.billing() == parzi_providers::Billing::Subscription {
                (0.0, 0.0)
            } else {
                AgentRun::lookup_price(pid, mid)
            }
        };
        if model_spec == "auto" || model_spec.starts_with("auto/") {
            let mut slots = vec![];
            for route in parzi_providers::router::auto_chain(effort, cfg) {
                let provider = factory(&route.provider, cfg)?;
                let (price_in, price_out) =
                    priced(provider.as_ref(), &route.provider, &route.model);
                slots.push(ProviderSlot {
                    provider_id: route.provider,
                    provider,
                    model_id: route.model,
                    price_in,
                    price_out,
                    reason: route.reason,
                });
            }
            if slots.is_empty() {
                return Err(ParziError::Store(
                    "Smart Auto has nothing to route: sign in to a subscription (Claude Code, \
                     Codex, Google/Antigravity or opencode serve) or pick a model with a key"
                        .into(),
                ));
            }
            return Ok(slots);
        }
        let (provider_id, model_id) = cfg.resolve_model(model_spec);
        // Old threads still say `claude-code/…` or `anthropic/…`.
        let provider_id = parzi_providers::canonical_id(&provider_id)
            .map(str::to_string)
            .unwrap_or(provider_id);
        // Adaptive failover (default): expand an explicit pick into a
        // tier-preserving chain so a 429 hops instead of killing the turn.
        // Strict mode (auto_failover=false): single slot, 429s halt.
        if cfg.routing.auto_failover {
            let mut slots = vec![];
            for route in
                parzi_providers::router::tier_fallback_chain(&provider_id, &model_id, effort, cfg)
            {
                match factory(&route.provider, cfg) {
                    Ok(provider) => {
                        let (price_in, price_out) =
                            priced(provider.as_ref(), &route.provider, &route.model);
                        slots.push(ProviderSlot {
                            provider_id: route.provider,
                            provider,
                            model_id: route.model,
                            price_in,
                            price_out,
                            reason: route.reason,
                        });
                    }
                    Err(_) => continue,
                }
            }
            if slots.is_empty() {
                return Err(ParziError::Store("router found nothing usable".into()));
            }
            return Ok(slots);
        }
        let provider = factory(&provider_id, cfg)?;
        let (price_in, price_out) = priced(provider.as_ref(), &provider_id, &model_id);
        Ok(vec![ProviderSlot {
            provider_id,
            provider,
            model_id,
            price_in,
            price_out,
            reason: "explicit pick",
        }])
    }

    pub(super) fn lane_policy_for(
        cfg: &ParziConfig,
        project: &str,
        lane: &str,
    ) -> (ApprovalMode, Vec<String>) {
        let mut mode = ApprovalMode::parse(&cfg.lanes.default_mode);
        let mut allowed = cfg.lanes.default_allowed_tools.clone();
        if let Ok(scan) = parzi_core::lanes::scan_projects() {
            for (p, lane_list) in scan {
                if p.name != project {
                    continue;
                }
                mode = ApprovalMode::parse(
                    &p.defaults
                        .mode
                        .clone()
                        .unwrap_or_else(|| cfg.lanes.default_mode.clone()),
                );
                if !p.defaults.allowed_tools.is_empty() {
                    allowed.clone_from(&p.defaults.allowed_tools);
                }
                for l in lane_list {
                    if l.name == lane {
                        mode = ApprovalMode::parse(&l.mode);
                        if !l.allowed_tools.is_empty() {
                            allowed.clone_from(&l.allowed_tools);
                        }
                    }
                }
            }
        }
        // Hub-workspace floor: workspace.toml [policy] mode. Deck slugs are
        // not workspaces, so role runs are floored by their workspace at the
        // launch site instead (a slug cannot smuggle itself out from under
        // its workspace's lockdown by being the project key).
        if parzi_core::workspace::load(project).is_ok() {
            if let Some(floor) = Self::workspace_policy_mode(project) {
                mode = Self::restrict_mode(mode, floor);
            }
        }
        // UI tools are always available: rendering is not execution.
        for u in ["ui.show_markdown", "ui.show_widget", "ui.show_diagram"] {
            if !allowed.contains(&u.to_string()) {
                allowed.push(u.into());
            }
        }
        // Teamwork harness tools are first-class: agents can delegate to
        // subsessions and message across sessions by default. Spawning and
        // messaging still pause for approval in lane Ask mode (handler gate).
        // Plan + lane-dispatch tools ship with the project workspace.
        for s in [
            "session.spawn",
            "session.send_message",
            "session.read_session",
            "session.list_sessions",
            "plan.read",
            "plan.update",
            "lane.dispatch",
            "knowledge.read",
            "knowledge.record",
        ] {
            if !allowed.contains(&s.to_string()) {
                allowed.push(s.into());
            }
        }
        (mode, allowed)
    }

    /// Most restrictive wins: Deny beats everything, Ask beats Auto. Used
    /// for workspace floors and chat overrides alike — nothing ever lifts a
    /// stricter setting, it can only tighten.
    pub(super) fn restrict_mode(a: ApprovalMode, b: ApprovalMode) -> ApprovalMode {
        use ApprovalMode::{Ask, Auto, Deny};
        match (a, b) {
            (Deny, _) | (_, Deny) => Deny,
            (Ask, _) | (_, Ask) => Ask,
            (Auto, Auto) => Auto,
        }
    }

    /// The `[policy] mode` of a hub workspace, if it states one. Unknown
    /// words parse to Ask (fail closed, like every other name gate).
    pub(super) fn workspace_policy_mode(workspace: &str) -> Option<ApprovalMode> {
        let ws = parzi_core::workspace::load(workspace).ok()?;
        let m = ws.policy.mode.trim();
        if m.is_empty() {
            None
        } else {
            Some(ApprovalMode::parse(m))
        }
    }

    /// Build + launch a run for an existing session. Inserts the handle and
    /// spawns the driver, which releases the handle and pumps the queue when
    /// this run finishes. B4: handles are always removed on completion so the
    /// concurrency cap counts live runs only.
    pub(super) async fn launch(p: Pump, q: QueuedRun) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        let sid = q.session_id.clone();
        // The parked copy has served its purpose the moment we try to start.
        clear_queued(&sid);
        match Self::launch_inner(p.clone(), q).await {
            Ok(rx) => Ok(rx),
            Err(e) => {
                // R-5: a launch that fails must not leave the session sitting
                // `Queued` forever with nobody to tell. Park it Idle, say so
                // in the transcript, and put the error on the host bus —
                // queued runs have no per-run receiver to fail into.
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

    async fn launch_inner(p: Pump, q: QueuedRun) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        let snap = p.cfg_snapshot();
        let slots = Self::slots_for_static(&snap, &p.factory, &q.model_spec, &q.effort)?;
        // Persist the resolved route so the next `auto` turn sticks to it
        // (failover still hops on 429/overload within each run).
        if (q.model_spec == "auto" || q.model_spec.starts_with("auto/")) && !slots.is_empty() {
            let first = &slots[0];
            p.store.set_model(
                &q.session_id,
                &format!("{}/{}", first.provider_id, first.model_id),
            )?;
        }
        // §1.2: a run that is a project role is briefed and armed by its role,
        // not by the lane's SYSTEM.md. The lane still decides the approval
        // mode — a machine's Ask/Deny is never lifted by a project.
        let role = run_role(&q.session_id);
        let (mut mode, mut allowed) = Self::lane_policy_for(&snap, &q.project, &q.lane);
        // Deck roles file under their slug, so the floor above missed them:
        // a role run honours its workspace's policy, never less.
        if let Some(binding) = &role {
            if let Some(floor) = Self::workspace_policy_mode(&binding.ctx.workspace) {
                mode = Self::restrict_mode(mode, floor);
            }
        }
        // Chat intent (the composer's permission pill): tightens, never lifts.
        // "edits" rides as Ask with file writes pre-approved.
        let mut edits_auto = false;
        if let Some(o) = q.mode_override.as_deref() {
            mode = Self::restrict_mode(mode, ApprovalMode::parse(o));
            edits_auto = o.trim() == "edits";
        }
        if let Some(binding) = &role {
            allowed = binding.tools();
            for u in ["ui.show_markdown", "ui.show_widget", "ui.show_diagram"] {
                allowed.push(u.into());
            }
        }
        // A run the lease layer knows (a project lane, registered with its
        // holder before dispatch) gets the lease + board tools and the write
        // gate; an ordinary thread gets neither.
        let lease_ctx = p
            .leases
            .holder_of_run(&q.session_id)
            .await
            .map(|_| LeaseCtx::new(p.leases.clone(), &q.session_id));
        let tools = Arc::new(ToolExecutor {
            cwd: q.cwd.clone(),
            mcp: p.mcp.clone(),
            allowed,
            leases: lease_ctx,
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        // Every run gets the teamwork bridge so agents can spawn subsessions
        // and message across sessions with `session.*` tools.
        let bridge: Arc<dyn HarnessBridge> = Arc::new(p.clone());
        let run = AgentRun::new(
            q.session_id.clone(),
            slots,
            match &role {
                Some(binding) => binding.brief(),
                None => system_parts(&snap, &q.project, &q.lane),
            },
            q.lane.clone(),
            mode,
            edits_auto,
            snap.lanes.max_steps,
            effort_tokens(&q.effort),
            q.attachments.clone(),
            q.effort.clone(),
            p.store.clone(),
            tools,
            // B2: harness-spawned children must never silently run as Auto.
            // No approver carried over = deny by default; explicit callers
            // (GUI/CLI) always pass Some(...).
            q.approver.clone().unwrap_or_else(|| Arc::new(DenyApprover)),
            tx,
            cancel.clone(),
        )
        .with_circuit_breaker(p.circuit_breaker.clone())
        .with_harness(bridge)
        // R-4 / R-5: the run's spend cap, and the host bus every event is
        // mirrored to (the caller's `rx` above only exists for direct calls).
        .with_budget(budget_for(&snap, q.workspace_project.as_ref()))
        .with_bus(p.bus.clone())
        .with_prompt_recorded(q.prompt_recorded)
        .with_role(role);
        // A run that starts again is no longer paused: clear the stop note.
        set_run_note(&q.session_id, None);
        p.store.set_status(&q.session_id, SessionStatus::Active)?;
        let parts = p.clone();
        let handles = p.handles.clone();
        let handles_task = handles.clone();
        let sid = q.session_id.clone();
        let sid_task = sid.clone();
        let prompt = q.prompt.clone();
        let store = p.store.clone();
        let task = tokio::spawn(async move {
            if let Err(e) = run.run(&prompt).await {
                let _ = store.append(
                    &sid_task,
                    &parzi_core::store::Event::System {
                        text: format!("run failed: {e}"),
                    },
                );
            }
            // B4: release the slot before waking the pump. Finished runs must
            // not pin `max_concurrent` forever.
            handles_task.lock().await.remove(&sid_task);
            // PLAN §15.6: a lane that ended — cleanly or not — stops holding
            // files now. Only a crashed *process* waits for the TTL.
            parts.leases.unregister(&sid_task).await;
            // Sync wake-up only: awaiting pump() here would close a
            // launch→driver→pump→launch await cycle that Send cannot prove.
            parts.notify.notify_one();
        });
        handles.lock().await.insert(
            sid,
            Handle {
                cancel,
                _task: task,
            },
        );
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_policy_equips_session_harness_tools() {
        let cfg = ParziConfig::default();
        let (_mode, allowed) = Orchestrator::lane_policy_for(&cfg, "default", "");
        for t in [
            "session.spawn",
            "session.send_message",
            "session.read_session",
            "session.list_sessions",
        ] {
            assert!(allowed.iter().any(|a| a == t), "lane missing {t}");
        }
    }

    #[test]
    fn mode_floor_never_lifts_only_tightens() {
        use super::Orchestrator;
        use crate::tools::ApprovalMode::{Ask, Auto, Deny};
        assert_eq!(Orchestrator::restrict_mode(Auto, Auto), Auto);
        assert_eq!(Orchestrator::restrict_mode(Auto, Ask), Ask);
        assert_eq!(Orchestrator::restrict_mode(Ask, Auto), Ask);
        assert_eq!(Orchestrator::restrict_mode(Auto, Deny), Deny);
        assert_eq!(Orchestrator::restrict_mode(Deny, Auto), Deny);
        assert_eq!(Orchestrator::restrict_mode(Ask, Ask), Ask);
    }
}
