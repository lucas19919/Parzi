mod harness;
mod launch;
mod queue;

pub use queue::{run_note, BUDGET_NOTE};
pub(crate) use queue::{run_session, set_run_note, set_run_session, VendorSession};

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::error::Result;
use parzi_core::store::{SessionMeta, SessionStatus, SessionStore};
use tokio::sync::{broadcast, Mutex, Notify};
use tokio_util::sync::CancellationToken;

use crate::handler::{HarnessBridge, RunEvent, RunEventBus};
use crate::mcp::McpManager;
use crate::mcp_host::McpHost;
use crate::status::{roster_source, ProviderSource, StatusBoard};

use queue::{clear_queued, Pump, QueuedRun};

const BUS_CAPACITY: usize = 4_096;

static HEADLESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `parzi serve` runs with no desk, so its lanes offer no browser tools.
pub fn set_headless() {
    HEADLESS.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn headless() -> bool {
    HEADLESS.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn normalize_effort(effort: &str) -> String {
    match effort.trim() {
        e @ ("minimal" | "low" | "medium" | "high" | "extra" | "ultra" | "xhigh" | "max") => {
            e.to_string()
        }
        _ => "medium".to_string(),
    }
}

struct Handle {
    id: u64,
    cancel: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Handle {
    fn finished(&self) -> bool {
        self.task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
    }
}

fn next_run_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

type ReadMarks = Arc<std::sync::Mutex<HashMap<String, usize>>>;

pub(crate) struct SessionSlot {
    handles: Arc<Mutex<HashMap<String, Handle>>>,
    marks: ReadMarks,
    sid: String,
    id: u64,
}

impl SessionSlot {
    pub(crate) fn read_mark(&self) -> Option<usize> {
        self.marks.lock().ok()?.get(&self.sid).copied()
    }

    pub(crate) fn set_read_mark(&self, at: usize) {
        if let Ok(mut m) = self.marks.lock() {
            m.insert(self.sid.clone(), at);
        }
    }

    pub(crate) async fn last_look<L, Q>(&self, look: L, on_quiet: Q) -> (Vec<String>, usize)
    where
        L: FnOnce() -> (Vec<String>, usize),
        Q: FnOnce(),
    {
        let mut h = self.handles.lock().await;
        let (fresh, upto) = look();
        self.set_read_mark(upto);
        if fresh.is_empty() {
            on_quiet();
            if h.get(&self.sid).is_some_and(|x| x.id == self.id) {
                h.remove(&self.sid);
            }
        }
        (fresh, upto)
    }
}

pub struct Orchestrator {
    cfg: std::sync::Arc<std::sync::RwLock<ParziConfig>>,
    store: SessionStore,
    mcp: Arc<McpManager>,
    handles: Arc<Mutex<HashMap<String, Handle>>>,
    queue: Arc<Mutex<VecDeque<QueuedRun>>>,
    notify: Arc<Notify>,
    source: ProviderSource,
    status: Arc<StatusBoard>,
    tools_server: Arc<tokio::sync::OnceCell<Option<Arc<McpHost>>>>,
    bus: RunEventBus,
    marks: ReadMarks,
    asker: Arc<Mutex<Option<Arc<dyn crate::tools::Asker>>>>,
    shells: Arc<std::sync::Mutex<HashMap<String, Arc<crate::shell::ShellRegistry>>>>,
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
            bus,
            marks: ReadMarks::default(),
            asker: Arc::new(Mutex::new(None)),
            shells: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn set_asker(&self, asker: Arc<dyn crate::tools::Asker>) {
        // Never silently drop the asker: a missed GUI asker turns every
        // ask.user into "decide yourself". Called from sync setup, so a
        // blocking lock is safe (must stay out of async contexts).
        *self.asker.blocking_lock() = Some(asker);
    }

    /// Kill every background shell of a session and forget its registry.
    /// Called when sessions are deleted or purged so nothing outlives them.
    pub fn drop_shells(&self, id: &str) {
        let reg = self.shells.lock().map(|mut m| m.remove(id)).unwrap_or(None);
        if let Some(reg) = reg {
            reg.kill_all();
        }
    }

    #[must_use]
    pub fn with_source(mut self, source: ProviderSource) -> Self {
        self.source = source;
        self
    }

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
            marks: self.marks.clone(),
            asker: self.asker.clone(),
            shells: self.shells.clone(),
        }
    }

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

    pub async fn apply_config(&self, cfg: ParziConfig) {
        self.mcp.set_configs(cfg.mcp.servers.clone()).await;
        if let Ok(mut w) = self.cfg.write() {
            *w = cfg;
        }
    }

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

    pub fn provider_statuses(&self) -> Vec<parzi_providers::ProviderStatus> {
        self.with_gates(self.status.all())
    }

    pub async fn refresh_providers(&self, ids: &[String]) -> Vec<parzi_providers::ProviderStatus> {
        let cfg = self.config();
        let all = self.status.refresh(&cfg, ids, &self.source).await;
        self.with_gates(all)
    }

    fn with_gates(
        &self,
        mut all: Vec<parzi_providers::ProviderStatus>,
    ) -> Vec<parzi_providers::ProviderStatus> {
        let cfg = self.config();
        for s in &mut all {
            if let Some(p) = (self.source)(&s.provider, &cfg) {
                s.gated = p.gated();
            }
        }
        all
    }

    pub async fn kill(&self, id: &str) -> Result<()> {
        // The handle stays (cancelled, task taken) until the run has really
        // stopped, so a resend cannot start a second run in the grace window.
        let stopping = self.handles.lock().await.get_mut(id).map(|h| {
            h.cancel.cancel();
            (h.id, h.task.take())
        });
        if let Some((run, task)) = stopping {
            let mut alive = true;
            if let Some(task) = task {
                let by = tokio::time::Instant::now()
                    + parzi_providers::process::STOP_GRACE
                    + std::time::Duration::from_secs(2);
                while !task.is_finished() && tokio::time::Instant::now() < by {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                alive = !task.is_finished();
                task.abort();
            }
            {
                let mut h = self.handles.lock().await;
                if h.get(id).is_some_and(|x| x.id == run) {
                    h.remove(id);
                }
            }
            if alive {
                let _ = self
                    .bus
                    .send((id.to_string(), RunEvent::Error("cancelled".into())));
            }
        }
        self.queue.lock().await.retain(|q| q.session_id != id);
        clear_queued(id);
        self.store.set_status(id, SessionStatus::Killed)?;
        self.notify.notify_one();
        Ok(())
    }

    pub async fn fork(&self, id: &str, at_step: Option<usize>) -> Result<SessionMeta> {
        self.store.fork(id, at_step)
    }

    pub fn harness(&self) -> Arc<dyn HarnessBridge> {
        Arc::new(self.pump_parts())
    }
}
