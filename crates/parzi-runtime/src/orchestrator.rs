//! Multi-session process manager. Spawn / focus / fork / kill / retry.
//! Over the concurrency limit, runs queue (config) instead of dying loudly.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Event, SessionMeta, SessionStatus, SessionStore};
use tokio::sync::{Mutex, Notify, mpsc};
use tokio_util::sync::CancellationToken;

use crate::circuit_breaker::CircuitBreaker;
use crate::handler::{AgentRun, HarnessBridge, RunEvent, system_parts};
use crate::mcp::McpManager;
use crate::tools::{ApprovalMode, Approver, AutoApprover, ToolExecutor};

/// Effort pill → output budget. Single place both CLI and GUI derive from.
/// Legacy `"med"` still resolves to medium.
pub fn effort_tokens(effort: &str) -> u32 {
    match effort {
        "low" => 4_096,
        "medium" | "med" => 16_384,
        "high" => 65_536,
        "extra" => 131_072,
        "ultra" => 262_144,
        _ => 16_384,
    }
}

pub fn normalize_effort(effort: &str) -> String {
    match effort {
        "low" | "medium" | "high" | "extra" | "ultra" => effort.to_string(),
        "med" => "medium".to_string(),
        _ => "medium".to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct RunInfo {
    pub id: String,
    pub status: SessionStatus,
    pub model: String,
    pub lane: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
}

struct Handle {
    cancel: CancellationToken,
    _task: tokio::task::JoinHandle<()>,
}

/// Provider construction seam. Default is the registry; tests inject hangs.
pub type ProviderFactory = Arc<
    dyn Fn(&str, &ParziConfig) -> parzi_core::error::Result<Box<dyn parzi_providers::Provider>>
        + Send
        + Sync,
>;

fn default_factory(
    id: &str,
    cfg: &ParziConfig,
) -> parzi_core::error::Result<Box<dyn parzi_providers::Provider>> {
    parzi_providers::provider(id, cfg)
}

/// A run waiting for a slot. Pumped headless (transcript persists, no live
/// channel) with AutoApprover; lane Ask mode still logs denials visibly.
struct QueuedRun {
    session_id: String,
    project: String,
    lane: String,
    model_spec: String,
    prompt: String,
    cwd: String,
    effort: String,
    attachments: Vec<parzi_core::context::AttachedFile>,
    approver: Option<Arc<dyn Approver>>,
}

/// Cloneable handles for the pump, which runs inside finished tasks.
#[derive(Clone)]
struct Pump {
    queue: Arc<Mutex<VecDeque<QueuedRun>>>,
    notify: Arc<Notify>,
    cfg: ParziConfig,
    factory: ProviderFactory,
    mcp: Arc<McpManager>,
    store: SessionStore,
    handles: Arc<Mutex<HashMap<String, Handle>>>,
    circuit_breaker: Arc<CircuitBreaker>,
}

pub struct Orchestrator {
    cfg: ParziConfig,
    store: SessionStore,
    mcp: Arc<McpManager>,
    handles: Arc<Mutex<HashMap<String, Handle>>>,
    queue: Arc<Mutex<VecDeque<QueuedRun>>>,
    notify: Arc<Notify>,
    factory: ProviderFactory,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl Orchestrator {
    pub fn new(cfg: ParziConfig, store: SessionStore) -> Self {
        let mcp = Arc::new(McpManager::new(cfg.mcp.servers.clone(), cfg.orchestrator.mcp_idle_kill_secs));
        Self {
            cfg,
            store,
            mcp,
            handles: Arc::new(Mutex::new(HashMap::new())),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            notify: Arc::new(Notify::new()),
            factory: Arc::new(default_factory),
            circuit_breaker: Arc::new(CircuitBreaker::new()),
        }
    }

    pub fn with_factory(mut self, factory: ProviderFactory) -> Self {
        self.factory = factory;
        self
    }

    fn pump_parts(&self) -> Pump {
        Pump {
            queue: self.queue.clone(),
            notify: self.notify.clone(),
            cfg: self.cfg.clone(),
            factory: self.factory.clone(),
            mcp: self.mcp.clone(),
            store: self.store.clone(),
            handles: self.handles.clone(),
            circuit_breaker: self.circuit_breaker.clone(),
        }
    }

    /// Long-lived pump loop. Spawn once per process; the driver task only
    /// notifies (sync — this keeps the async graph acyclic for Send).
    pub async fn pump_loop(self: Arc<Self>) {
        loop {
            self.notify.notified().await;
            Self::pump(self.pump_parts()).await;
        }
    }

    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    pub fn config(&self) -> &ParziConfig {
        &self.cfg
    }

    /// Mark crashed Active sessions Idle on boot. Nothing is ever lost:
    /// transcripts are on disk.
    pub fn recover(&self) -> Result<usize> {
        let mut n = 0;
        for m in self.store.list()? {
            if m.status == SessionStatus::Active {
                self.store.set_status(&m.id, SessionStatus::Idle)?;
                n += 1;
            }
        }
        Ok(n)
    }

    pub async fn list_runs(&self) -> Result<Vec<RunInfo>> {
        let mut out = vec![];
        for m in self.store.list()? {
            out.push(RunInfo {
                id: m.id.clone(),
                status: m.status,
                model: m.model.clone(),
                lane: if m.lane.is_empty() { m.project.clone() } else { format!("{}/{}", m.project, m.lane) },
                tokens_in: m.tokens_in,
                tokens_out: m.tokens_out,
                cost_usd: m.cost_usd,
            });
        }
        Ok(out)
    }

    /// Spawn a run. Returns the session id + live event channel.
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
    ) -> Result<(SessionMeta, mpsc::UnboundedReceiver<RunEvent>)> {
        let live = self.handles.lock().await.len();
        let effort = normalize_effort(effort);
        let title: String = prompt.lines().next().unwrap_or("untitled").chars().take(80).collect();

        let mut meta = self.store.create(&title, project, lane, model_spec)?;
        meta.cwd = cwd.to_string();

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
        };
        if live >= self.cfg.orchestrator.max_concurrent.max(1) {
            if !self.cfg.orchestrator.queue_when_busy {
                return Err(ParziError::Store(format!(
                    "busy: {live} runs active (max {}); kill one first",
                    self.cfg.orchestrator.max_concurrent
                )));
            }
            self.store.set_status(&meta.id, SessionStatus::Queued)?;
            self.queue.lock().await.push_back(q);
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
    ) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        if self.handles.lock().await.contains_key(id) {
            return Err(ParziError::Store(format!("run {id} is active; kill it first")));
        }
        let meta = self.store.get(id)?;
        // Per-message model switch: the thread follows the newly picked model.
        let spec = model_override.unwrap_or_else(|| meta.model.clone());
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
            cwd: if cwd.is_empty() { meta.cwd.clone() } else { cwd.to_string() },
            effort,
            attachments,
            approver,
        };
        let live = self.handles.lock().await.len();
        if live >= self.cfg.orchestrator.max_concurrent.max(1) {
            if !self.cfg.orchestrator.queue_when_busy {
                return Err(ParziError::Store("busy: max concurrent runs reached".into()));
            }
            self.store.set_status(id, SessionStatus::Queued)?;
            self.queue.lock().await.push_back(q);
            return Ok(Self::closed_rx());
        }
        Self::launch(self.pump_parts(), q).await
    }

    /// Build the provider chain: `auto` expands via the smart router,
    /// an explicit spec is a single slot. Never empty on success.
    fn slots_for(
        &self,
        model_spec: &str,
        effort: &str,
    ) -> Result<Vec<crate::handler::ProviderSlot>> {
        Self::slots_for_static(&self.cfg, &self.factory, model_spec, effort)
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
                let (price_in, price_out) = priced(provider.as_ref(), &route.provider, &route.model);
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
            for route in parzi_providers::router::tier_fallback_chain(
                &provider_id,
                &model_id,
                effort,
                cfg,
            ) {
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

    /// Live provider health for the Omnibar dots + Settings quota panel.
    /// Merges each provider's auth/tier state with circuit-breaker cooldowns.
    pub fn provider_health(&self) -> Vec<parzi_providers::ProviderHealth> {
        let mut out = vec![];
        for id in parzi_providers::PROVIDERS.iter().copied() {
            let mut h = match (self.factory)(id, &self.cfg) {
                Ok(p) => p.health(),
                Err(e) => parzi_providers::ProviderHealth {
                    provider: id.to_string(),
                    status: "missing".to_string(),
                    cooldown_until: None,
                    last_error: Some(e.to_string()),
                    active_account: None,
                    tier: "none".to_string(),
                },
            };
            if let Some(rem) = self.circuit_breaker.cooldown_remaining(id) {
                h.status = "rate_limited".to_string();
                h.cooldown_until = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() + rem)
                        .unwrap_or(0),
                );
                if h.last_error.is_none() {
                    h.last_error = self.circuit_breaker.last_error(id);
                }
            } else if let Some(err) = self.circuit_breaker.last_error(id) {
                if h.last_error.is_none() {
                    h.last_error = Some(err);
                }
            }
            out.push(h);
        }
        out
    }

    /// Clear cooldowns (one provider or all). Used by Settings retry buttons.
    pub fn reset_circuit_breaker(&self, provider: Option<&str>) {
        self.circuit_breaker.reset(provider);
    }

    pub async fn kill(&self, id: &str) -> Result<()> {
        if let Some(h) = self.handles.lock().await.remove(id) {
            h.cancel.cancel();
        }
        // Dequeue anything waiting for this session too.
        self.queue.lock().await.retain(|q| q.session_id != id);
        self.mcp.stop(id).await;
        self.store.set_status(id, SessionStatus::Killed)?;
        // A freed slot may unblock the queue.
        self.notify.notify_one();
        Ok(())
    }

    pub async fn fork(&self, id: &str, at_step: Option<usize>) -> Result<SessionMeta> {
        self.store.fork(id, at_step)
    }

    /// Handle to the teamwork harness (`session.*` tools) bound to this
    /// orchestrator's store and run queue. Used by tests and (via commands)
    /// the Tauri backend.
    pub fn harness(&self) -> Arc<dyn HarnessBridge> {
        Arc::new(self.pump_parts())
    }

    /// Create a child subsession record under `parent_id`, inheriting the
    /// parent's project/lane/cwd and — unless overridden — model. When
    /// `prompt` is given, a run starts on the child (launched or queued like
    /// any other run); otherwise the subsession waits empty for its first
    /// message. Never fails the creation when the run cannot start: the
    /// reason is appended to the child's transcript instead.
    pub async fn create_subsession(
        &self,
        parent_id: &str,
        title: &str,
        prompt: Option<&str>,
        model: Option<&str>,
    ) -> Result<SessionMeta> {
        let parent = self.store.get(parent_id)?;
        let title: String = if title.trim().is_empty() {
            prompt
                .and_then(|p| p.lines().next())
                .unwrap_or("subsession")
                .chars()
                .take(80)
                .collect()
        } else {
            title.chars().take(80).collect()
        };
        let model_spec = model
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| parent.model.clone());
        let meta = self.store.create_with_parent(
            &title,
            &parent.project,
            &parent.lane,
            &model_spec,
            Some(parent_id),
        )?;
        if !parent.cwd.is_empty() {
            let _ = self.store.set_cwd(&meta.id, &parent.cwd);
        }
        if let Some(p) = prompt.filter(|s| !s.trim().is_empty()) {
            let q = QueuedRun {
                session_id: meta.id.clone(),
                project: parent.project.clone(),
                lane: parent.lane.clone(),
                model_spec,
                prompt: p.to_string(),
                cwd: parent.cwd.clone(),
                effort: "medium".into(),
                attachments: vec![],
                approver: None,
            };
            let live = self.handles.lock().await.len();
            if live >= self.cfg.orchestrator.max_concurrent.max(1) {
                if self.cfg.orchestrator.queue_when_busy {
                    let _ = self.store.set_status(&meta.id, SessionStatus::Queued);
                    self.queue.lock().await.push_back(q);
                }
            } else if let Err(e) = Self::launch(self.pump_parts(), q).await {
                let _ = self.store.append(
                    &meta.id,
                    &Event::System { text: format!("subsession run failed to start: {e}") },
                );
            }
        }
        self.store.get(&meta.id)
    }

    fn lane_policy(&self, project: &str, lane: &str) -> (ApprovalMode, Vec<String>) {
        Self::lane_policy_for(&self.cfg, project, lane)
    }

    fn lane_policy_for(
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
                    &p.defaults.mode.clone().unwrap_or_else(|| cfg.lanes.default_mode.clone()),
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
        // UI tools are always available: rendering is not execution.
        for u in ["ui.show_markdown", "ui.show_widget", "ui.show_diagram"] {
            if !allowed.contains(&u.to_string()) {
                allowed.push(u.into());
            }
        }
        // Teamwork harness tools are first-class: agents can delegate to
        // subsessions and message across sessions by default. Spawning and
        // messaging still pause for approval in lane Ask mode (handler gate).
        for s in [
            "session.spawn",
            "session.send_message",
            "session.read_session",
            "session.list_sessions",
        ] {
            if !allowed.contains(&s.to_string()) {
                allowed.push(s.into());
            }
        }
        (mode, allowed)
    }

    /// Build + launch a run for an existing session. Inserts the handle and
    /// spawns the driver, which pumps the queue when this run finishes.
    async fn launch(p: Pump, q: QueuedRun) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        let slots = Self::slots_for_static(&p.cfg, &p.factory, &q.model_spec, &q.effort)?;
        let (mode, allowed) = Self::lane_policy_for(&p.cfg, &q.project, &q.lane);
        let tools = Arc::new(ToolExecutor {
            cwd: q.cwd.clone(),
            mcp: p.mcp.clone(),
            allowed,
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        // Every run gets the teamwork bridge so agents can spawn subsessions
        // and message across sessions with `session.*` tools.
        let bridge: Arc<dyn HarnessBridge> = Arc::new(p.clone());
        let run = AgentRun::new(
            q.session_id.clone(),
            slots,
            system_parts(&p.cfg, &q.project, &q.lane),
            q.lane.clone(),
            mode,
            p.cfg.lanes.max_steps,
            effort_tokens(&q.effort),
            q.attachments.clone(),
            q.effort.clone(),
            p.store.clone(),
            tools,
            q.approver.clone().unwrap_or_else(|| Arc::new(AutoApprover)),
            tx,
            cancel.clone(),
        )
        .with_circuit_breaker(p.circuit_breaker.clone())
        .with_harness(bridge);
        p.store.set_status(&q.session_id, SessionStatus::Active)?;
        let parts = p.clone();
        let handles = p.handles.clone();
        let sid = q.session_id.clone();
        let sid_task = sid.clone();
        let prompt = q.prompt.clone();
        let store = p.store.clone();
        let task = tokio::spawn(async move {
            if let Err(e) = run.run(&prompt).await {
                let _ = store.append(
                    &sid_task,
                    &parzi_core::store::Event::System { text: format!("run failed: {e}") },
                );
            }
            // Sync wake-up only: awaiting pump() here would close a
            // launch→driver→pump→launch await cycle that Send cannot prove.
            parts.notify.notify_one();
        });
        handles.lock().await.insert(sid, Handle { cancel, _task: task });
        Ok(rx)
    }

    /// Start queued runs while slots are free. Skips killed/missing sessions.
    async fn pump(p: Pump) {
        loop {
            let next = {
                let h = p.handles.lock().await;
                if h.len() >= p.cfg.orchestrator.max_concurrent.max(1) {
                    return;
                }
                drop(h);
                p.queue.lock().await.pop_front()
            };
            let Some(q) = next else { return };
            let killed = match p.store.get(&q.session_id) {
                Ok(m) => m.status == SessionStatus::Killed,
                Err(_) => true,
            };
            if killed {
                continue;
            }
            if Self::launch(p.clone(), q).await.is_err() {
                return;
            }
        }
    }

    /// Kick the pump (boot recovery, kills). No-op when nothing is queued.
    pub async fn kick(&self) {
        self.notify.notify_one();
    }

    fn closed_rx() -> mpsc::UnboundedReceiver<RunEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(tx);
        rx
    }
}

/// How long `wait: true` harness calls block for a child reply before
/// handing back a `timeout` status (the child keeps running; poll with
/// `session.read_session`).
const HARNESS_WAIT_SECS: u64 = 180;

impl Pump {
    fn max_live(&self) -> usize {
        self.cfg.orchestrator.max_concurrent.max(1)
    }

    /// Enqueue-or-launch a queued run, mirroring `Orchestrator::spawn`.
    /// Returns true when the run launched immediately (slot free).
    async fn dispatch(&self, q: QueuedRun) -> bool {
        let live = self.handles.lock().await.len();
        if live >= self.max_live() {
            if self.cfg.orchestrator.queue_when_busy {
                let _ = self.store.set_status(&q.session_id, SessionStatus::Queued);
                self.queue.lock().await.push_back(q);
            }
            return false;
        }
        Orchestrator::launch(self.clone(), q).await.is_ok()
    }

    /// Block until the session leaves Active/Queued (or the timeout hits).
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

    /// Last assistant text in a session (the "reply" for `wait: true`).
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
    /// Spawn a child subsession (`is_subsession`) under the caller or a full
    /// top-level session. The child inherits the caller's project, lane, cwd
    /// and — unless overridden — model. `wait: true` blocks for the reply;
    /// when no run slot is free it returns `queued` instead of deadlocking
    /// the parent against the concurrency cap.
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
            prompt.lines().next().unwrap_or("subsession").chars().take(80).collect()
        } else {
            title.chars().take(80).collect()
        };
        // Recommended default: inherit the parent's active model.
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

    /// Deliver a message to another session and optionally wait for its reply.
    /// When the target is already running, the message is appended for it to
    /// pick up; otherwise a continuation run is started on that session.
    async fn send_message(
        &self,
        _caller_id: &str,
        session_id: &str,
        message: &str,
        wait: bool,
    ) -> Result<String> {
        let target = self.store.get(session_id)?;
        if self.handles.lock().await.contains_key(session_id) {
            self.store.append(session_id, &Event::User { text: message.into() })?;
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
            prompt: message.to_string(),
            cwd: target.cwd.clone(),
            effort: "medium".into(),
            attachments: vec![],
            approver: None,
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

    async fn read_session(&self, session_id: &str, tail_events: Option<usize>) -> Result<String> {
        let meta = self.store.get(session_id)?;
        let tail = tail_events.unwrap_or(20).clamp(1, 60);
        let events = self.store.events(session_id).unwrap_or_default();
        let start = events.len().saturating_sub(tail);
        let mut tail_text = String::new();
        for e in events.iter().skip(start) {
            let line = match e {
                Event::System { text } => format!("> system: {text}\n"),
                Event::User { text } => format!("user: {}\n", truncate(text, 1_000)),
                Event::Assistant { text, .. } => format!("assistant: {}\n", truncate(text, 2_000)),
                Event::ToolCall { name, .. } => format!("tool_call: {name}\n"),
                Event::ToolResult { name, ok, output, .. } => {
                    format!("tool_result({name}, ok={ok}): {}\n", truncate(output, 500))
                }
                Event::Reasoning { text } => format!("reasoning: {}\n", truncate(text, 300)),
                Event::Checkpoint { summary } => format!("checkpoint: {}\n", truncate(summary, 300)),
                Event::Widget { .. } => "[widget]\n".to_string(),
                Event::Artifact { id, title, version, .. } => {
                    format!("artifact: {title} ({id} v{version})\n")
                }
                Event::RouteTransition { from_provider, to_provider, reason, .. } => {
                    format!("route: {from_provider} -> {to_provider} ({reason})\n")
                }
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
        let rows: Vec<serde_json::Value> = all
            .into_iter()
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
}

fn truncate(s: &str, n: usize) -> String {
    let cut: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{cut}…")
    } else {
        cut
    }
}
