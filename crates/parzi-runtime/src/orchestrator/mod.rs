//! Multi-session process manager. Spawn / focus / fork / kill / retry.
//! Over the concurrency limit, runs queue (config) instead of dying loudly.
//!
//! Four files, one type: this one holds the `Orchestrator` itself and the
//! reads a host asks it for; `queue` holds the parked-run state and the pump,
//! `launch` turns a queued run into a live one, and `harness` is the
//! `session.*` bridge agents talk to each other through.

mod harness;
mod launch;
mod queue;

pub use queue::{
    run_note, run_project, run_role, run_session, set_run_note, set_run_project, set_run_role,
    set_run_session, VendorSession, BUDGET_NOTE,
};

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::error::Result;
use parzi_core::store::{SessionMeta, SessionStatus, SessionStore};
use tokio::sync::{broadcast, mpsc, Mutex, Notify};
use tokio_util::sync::CancellationToken;

use crate::handler::{HarnessBridge, RunEvent, RunEventBus};
use crate::lease_tools::LeaseHub;
use crate::mcp::McpManager;
use crate::mcp_host::McpHost;
use crate::status::{roster_source, ProviderSource, StatusBoard};
use crate::roles::{Role, RoleBinding, RoleCtx};
use crate::tools::{ApprovalMode, Approver};

use queue::{clear_queued, Pump, QueuedRun};

/// How many events the host bus keeps for a slow subscriber before it lags.
const BUS_CAPACITY: usize = 4_096;

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

pub struct Orchestrator {
    cfg: std::sync::Arc<std::sync::RwLock<ParziConfig>>,
    store: SessionStore,
    mcp: Arc<McpManager>,
    handles: Arc<Mutex<HashMap<String, Handle>>>,
    queue: Arc<Mutex<VecDeque<QueuedRun>>>,
    notify: Arc<Notify>,
    /// Where runs get their provider: the roster, or a test's fakes.
    source: ProviderSource,
    /// Where each provider stands; Smart Auto and Settings read it.
    status: Arc<StatusBoard>,
    /// The MCP endpoint Parzi's tools are served on, started lazily.
    tools_server: Arc<tokio::sync::OnceCell<Option<Arc<McpHost>>>>,
    bus: RunEventBus,
    /// PLAN §4: one lease table per process, shared by every lane that runs
    /// in it. Round 1 is hub-less, so this *is* the hub.
    leases: Arc<LeaseHub>,
}

impl Orchestrator {
    pub fn new(cfg: ParziConfig, store: SessionStore) -> Self {
        let mcp = Arc::new(McpManager::new(
            cfg.mcp.servers.clone(),
            cfg.orchestrator.mcp_idle_kill_secs,
        ));
        let (bus, _) = broadcast::channel(BUS_CAPACITY);
        Self {
            cfg: std::sync::Arc::new(std::sync::RwLock::new(cfg)),
            store,
            mcp,
            handles: Arc::new(Mutex::new(HashMap::new())),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            notify: Arc::new(Notify::new()),
            source: roster_source(),
            status: Arc::new(StatusBoard::load()),
            tools_server: Arc::new(tokio::sync::OnceCell::new()),
            leases: Arc::new(LeaseHub::new().with_bus(bus.clone())),
            bus,
        }
    }

    /// The process's lease layer: who holds which files, and where a lease
    /// request is delivered. `project_flow` registers lanes on it.
    #[must_use]
    pub fn leases(&self) -> Arc<LeaseHub> {
        self.leases.clone()
    }

    /// Runs get their providers from `source` (tests inject fakes).
    #[must_use]
    pub fn with_source(mut self, source: ProviderSource) -> Self {
        self.source = source;
        self
    }

    /// A status board other than the one on disk (tests).
    #[must_use]
    pub fn with_status(mut self, status: Arc<StatusBoard>) -> Self {
        self.status = status;
        self
    }

    fn pump_parts(&self) -> Pump {
        Pump {
            queue: self.queue.clone(),
            notify: self.notify.clone(),
            cfg: self.cfg.clone(),
            source: self.source.clone(),
            mcp: self.mcp.clone(),
            store: self.store.clone(),
            handles: self.handles.clone(),
            status: self.status.clone(),
            tools_server: self.tools_server.clone(),
            bus: self.bus.clone(),
            leases: self.leases.clone(),
        }
    }

    /// R-5: every run's events, on one channel. A host subscribes once in
    /// setup and sees queued runs, harness runs and re-enqueued runs too —
    /// the per-run receiver from `spawn`/`send_to` only covers its own call.
    pub fn subscribe(&self) -> broadcast::Receiver<(String, RunEvent)> {
        self.bus.subscribe()
    }

    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    pub fn config(&self) -> ParziConfig {
        self.cfg.read().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn mcp(&self) -> &Arc<McpManager> {
        &self.mcp
    }

    /// Hot-apply a saved config (Settings path): swaps the shared config and
    /// pushes fresh MCP server configs into the live manager. Next launch
    /// uses the new policy; no restart needed.
    pub async fn apply_config(&self, cfg: ParziConfig) {
        self.mcp.set_configs(cfg.mcp.servers.clone()).await;
        if let Ok(mut w) = self.cfg.write() {
            *w = cfg;
        }
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
                lane: if m.lane.is_empty() {
                    m.project.clone()
                } else {
                    format!("{}/{}", m.project, m.lane)
                },
                tokens_in: m.tokens_in,
                tokens_out: m.tokens_out,
                cost_usd: m.cost_usd,
            });
        }
        Ok(out)
    }

    // ------------------------------------------------------------- roles

    /// Start a project role (§1.2) in a new session: header, orchestrator or
    /// coder, on the model the project's roster binds to it. The session is
    /// bound to the role before it runs, so its briefing and its tool list
    /// come from `roles`, not from the lane's SYSTEM.md.
    pub async fn spawn_role(
        &self,
        workspace: &str,
        slug: &str,
        role: Role,
        prompt: &str,
        approver: Option<Arc<dyn Approver>>,
    ) -> Result<(SessionMeta, mpsc::UnboundedReceiver<RunEvent>)> {
        let cwd = parzi_core::project::dir(workspace, slug)
            .display()
            .to_string();
        self.spawn_role_in(workspace, slug, role, "", None, &cwd, prompt, approver)
            .await
    }

    /// `spawn_role` for a coder: its lane, its one task and the worktree it
    /// works in.
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn_role_in(
        &self,
        workspace: &str,
        slug: &str,
        role: Role,
        lane: &str,
        task: Option<&str>,
        cwd: &str,
        prompt: &str,
        approver: Option<Arc<dyn Approver>>,
    ) -> Result<(SessionMeta, mpsc::UnboundedReceiver<RunEvent>)> {
        let meta = self.role_session(workspace, slug, role, lane, task, cwd)?;
        let rx = self
            .send_to(
                &meta.id,
                prompt,
                approver,
                cwd,
                "medium",
                vec![],
                None,
                None,
            )
            .await?;
        Ok((self.store.get(&meta.id)?, rx))
    }

    /// Send a turn to a session that already runs as a role. The binding is
    /// re-asserted (idempotent) so a session that was created elsewhere — the
    /// project's one header session, say — is briefed like a role run.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_role(
        &self,
        workspace: &str,
        slug: &str,
        role: Role,
        session_id: &str,
        text: &str,
        approver: Option<Arc<dyn Approver>>,
    ) -> Result<mpsc::UnboundedReceiver<RunEvent>> {
        let meta = self.store.get(session_id)?;
        // A session already bound to this role keeps its binding — it carries
        // the lane and the task, which a plain "ask" does not know about.
        if run_role(session_id).map(|b| b.role) != Some(role) {
            let lane = if role == Role::Coder { &meta.lane } else { "" };
            self.bind_role(session_id, workspace, slug, role, lane, None);
        }
        self.send_to(session_id, text, approver, "", "medium", vec![], None, None)
            .await
    }

    /// Create the session a role run lives in, on the roster's model, and
    /// bind it — without running anything. Opening a project costs no tokens.
    /// An empty roster entry falls back to `auto` rather than refusing to
    /// start: a project is allowed to be vague about a model.
    pub fn role_session(
        &self,
        workspace: &str,
        slug: &str,
        role: Role,
        lane: &str,
        task: Option<&str>,
        cwd: &str,
    ) -> Result<SessionMeta> {
        let project = parzi_core::project::load(workspace, slug)?;
        let model = crate::roles::model_for(&project, role);
        let model = if model.trim().is_empty() {
            "auto"
        } else {
            model.trim()
        };
        let lane = if lane.is_empty() {
            role.lane_name()
        } else {
            lane
        };
        let title = match task {
            Some(t) => format!("{slug} · {} · {t}", role.as_str()),
            None => format!("{slug} · {}", role.as_str()),
        };
        let meta = self.store.create(&title, slug, lane, model)?;
        self.store.set_cwd(&meta.id, cwd)?;
        self.bind_role(&meta.id, workspace, slug, role, lane, task);
        Ok(meta)
    }

    /// Record what a session is: its role (briefing + tools) and its project
    /// (budget). Both live next to the transcript, so a restart and every
    /// later turn keep them.
    fn bind_role(
        &self,
        session_id: &str,
        workspace: &str,
        slug: &str,
        role: Role,
        lane: &str,
        task: Option<&str>,
    ) {
        let mut ctx = RoleCtx::new(workspace, slug).lane(lane);
        if let Some(t) = task {
            ctx = ctx.task(t);
        }
        set_run_role(session_id, Some(RoleBinding::new(role, ctx)));
        set_run_project(session_id, Some((workspace.to_string(), slug.to_string())));
    }

    /// Where each provider stands, as last checked (no probe).
    pub fn provider_statuses(&self) -> Vec<parzi_providers::ProviderStatus> {
        self.status.all()
    }

    /// Ask the providers where they stand now (`ids` empty = all). Each
    /// probe runs the vendor's own program and spends no quota.
    pub async fn refresh_providers(&self, ids: &[String]) -> Vec<parzi_providers::ProviderStatus> {
        let cfg = self.config();
        self.status.refresh(&cfg, ids, &self.source).await
    }

    pub async fn kill(&self, id: &str) -> Result<()> {
        if let Some(h) = self.handles.lock().await.remove(id) {
            h.cancel.cancel();
            // The provider gets a moment to stop its turn and close the
            // vendor program with everything it started.
            for _ in 0..30 {
                if h._task.is_finished() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            // R-1 backstop: a run stuck anyway is aborted, so the slot frees.
            h._task.abort();
        }
        // Dequeue anything waiting for this session too — in memory and on
        // disk, so a restart does not resurrect a killed run (R-5).
        self.queue.lock().await.retain(|q| q.session_id != id);
        clear_queued(id);
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

    #[allow(dead_code)]
    fn lane_policy(&self, project: &str, lane: &str) -> (ApprovalMode, Vec<String>) {
        let snap = self.config();
        Self::lane_policy_for(&snap, project, lane)
    }
}
