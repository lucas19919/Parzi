//! What a run is when it is not running: the sidecar next to the transcript
//! (R-5 — a queued prompt, the project it belongs to, the note it stopped
//! on), the parked run itself, and the pump that starts parked runs as slots
//! free up.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::error::Result;
use parzi_core::store::{Event, SessionMeta, SessionStatus, SessionStore};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex, Notify};

use crate::circuit_breaker::CircuitBreaker;
use crate::handler::{RunEvent, RunEventBus};
use crate::lease_tools::LeaseHub;
use crate::mcp::McpManager;
use crate::roles::RoleBinding;
use crate::tools::Approver;

use super::{Handle, Orchestrator, ProviderFactory};

/// The note a run that hit its budget leaves behind (R-4).
pub const BUDGET_NOTE: &str = "budget_exceeded";

/// Per-session run state that is not session metadata: the prompt a queued
/// run still owes (R-5 — it used to live only in memory, so a restart lost
/// it), the hub project the session belongs to (R-4, so follow-up turns keep
/// the project's budget) and the note a paused run left. One small file next
/// to the transcript, written atomically.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RunSidecar {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    queued: Option<PersistedRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project: Option<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    /// The project role this session runs as (§1.2), if it is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    role: Option<RoleBinding>,
}

impl RunSidecar {
    fn is_empty(&self) -> bool {
        self.queued.is_none()
            && self.project.is_none()
            && self.note.is_none()
            && self.role.is_none()
    }
}

/// Everything a queued run needs to be launched again after a restart.
/// Attachment bytes are deliberately not persisted: a re-enqueued run carries
/// its prompt (which names them), not a copy of the files.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRun {
    project: String,
    lane: String,
    model_spec: String,
    prompt: String,
    cwd: String,
    effort: String,
    #[serde(default)]
    workspace_project: Option<(String, String)>,
    #[serde(default)]
    prompt_recorded: bool,
}

/// H-7: only a valid uuid ever becomes a path segment.
fn sidecar_path(session_id: &str) -> Option<std::path::PathBuf> {
    uuid::Uuid::parse_str(session_id).ok()?;
    parzi_core::paths::sessions_dir()
        .ok()
        .map(|d| d.join(session_id).join("run.json"))
}

fn load_sidecar(session_id: &str) -> RunSidecar {
    let Some(path) = sidecar_path(session_id) else {
        return RunSidecar::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_sidecar(session_id: &str, sidecar: &RunSidecar) {
    let Some(path) = sidecar_path(session_id) else {
        return;
    };
    let outcome = if sidecar.is_empty() {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other.map_err(parzi_core::ParziError::Io),
        }
    } else {
        serde_json::to_vec(sidecar)
            .map_err(parzi_core::ParziError::Json)
            .and_then(|bytes| parzi_core::atomic_write(&path, &bytes))
    };
    if let Err(e) = outcome {
        tracing::warn!("run state for {session_id} not written: {e}");
    }
}

/// Mark why a run stopped (today: `budget_exceeded`). `None` clears it.
pub fn set_run_note(session_id: &str, note: Option<&str>) {
    let mut sidecar = load_sidecar(session_id);
    let next = note.map(str::to_string);
    if sidecar.note == next {
        return;
    }
    sidecar.note = next;
    save_sidecar(session_id, &sidecar);
}

/// The note a paused run left, for the deck and the header agent.
#[must_use]
pub fn run_note(session_id: &str) -> Option<String> {
    load_sidecar(session_id).note
}

/// Bind a session to a hub project `(workspace, slug)`. Every later turn of
/// that session then honours the project's `budget:` too, not just the turn
/// that was dispatched with it.
pub fn set_run_project(session_id: &str, project: Option<(String, String)>) {
    let mut sidecar = load_sidecar(session_id);
    if sidecar.project == project {
        return;
    }
    sidecar.project = project;
    save_sidecar(session_id, &sidecar);
}

/// The hub project a session belongs to, if any.
#[must_use]
pub fn run_project(session_id: &str) -> Option<(String, String)> {
    load_sidecar(session_id).project
}

/// Bind a session to a project role (§1.2). Every turn of that session is
/// then briefed with `roles::brief` instead of the lane's SYSTEM.md and may
/// call only that role's tools — including turns the shell sends later.
pub fn set_run_role(session_id: &str, role: Option<RoleBinding>) {
    let mut sidecar = load_sidecar(session_id);
    if sidecar.role == role {
        return;
    }
    sidecar.role = role;
    save_sidecar(session_id, &sidecar);
}

/// The role a session runs as, if any.
#[must_use]
pub fn run_role(session_id: &str) -> Option<RoleBinding> {
    load_sidecar(session_id).role
}

/// A run waiting for a slot. Pumped headless (transcript persists, no live
/// channel) with AutoApprover; lane Ask mode still logs denials visibly.
pub(super) struct QueuedRun {
    pub(super) session_id: String,
    pub(super) project: String,
    pub(super) lane: String,
    pub(super) model_spec: String,
    pub(super) prompt: String,
    pub(super) cwd: String,
    pub(super) effort: String,
    pub(super) attachments: Vec<parzi_core::context::AttachedFile>,
    pub(super) approver: Option<Arc<dyn Approver>>,
    /// Hub project this run belongs to: `(workspace, slug)`. Set by the role
    /// dispatch path; the run then honours `budget:` from its PROJECT.md.
    /// (Named `workspace_project` because `project` is already the legacy
    /// lane project name on this struct.)
    pub(super) workspace_project: Option<(String, String)>,
    /// The opening turn is already in the transcript (queued at enqueue, or
    /// an inter-session message): the run must not append it again.
    pub(super) prompt_recorded: bool,
}

impl QueuedRun {
    fn persisted(&self) -> PersistedRun {
        PersistedRun {
            project: self.project.clone(),
            lane: self.lane.clone(),
            model_spec: self.model_spec.clone(),
            prompt: self.prompt.clone(),
            cwd: self.cwd.clone(),
            effort: self.effort.clone(),
            workspace_project: self.workspace_project.clone(),
            prompt_recorded: self.prompt_recorded,
        }
    }

    fn from_persisted(session_id: &str, p: PersistedRun) -> Self {
        Self {
            session_id: session_id.to_string(),
            project: p.project,
            lane: p.lane,
            model_spec: p.model_spec,
            prompt: p.prompt,
            cwd: p.cwd,
            effort: p.effort,
            attachments: vec![],
            approver: None,
            workspace_project: p.workspace_project,
            prompt_recorded: p.prompt_recorded,
        }
    }
}

/// Cloneable handles for the pump, which runs inside finished tasks.
/// `cfg` is shared (not cloned) so Settings saves apply to the next launch
/// without an app restart.
#[derive(Clone)]
pub(super) struct Pump {
    pub(super) queue: Arc<Mutex<VecDeque<QueuedRun>>>,
    pub(super) notify: Arc<Notify>,
    pub(super) cfg: std::sync::Arc<std::sync::RwLock<ParziConfig>>,
    pub(super) factory: ProviderFactory,
    pub(super) mcp: Arc<McpManager>,
    pub(super) store: SessionStore,
    pub(super) handles: Arc<Mutex<HashMap<String, Handle>>>,
    pub(super) circuit_breaker: Arc<CircuitBreaker>,
    pub(super) bus: RunEventBus,
    pub(super) leases: Arc<LeaseHub>,
}

impl Pump {
    pub(super) fn cfg_snapshot(&self) -> ParziConfig {
        self.cfg.read().map(|c| c.clone()).unwrap_or_default()
    }

    /// Park a run: persist it (R-5 — a restart must not lose the prompt),
    /// record the opening turn once, and push it onto the queue.
    pub(super) async fn enqueue(&self, mut q: QueuedRun) {
        record_prompt(&self.store, &mut q);
        let mut sidecar = load_sidecar(&q.session_id);
        sidecar.queued = Some(q.persisted());
        save_sidecar(&q.session_id, &sidecar);
        self.queue.lock().await.push_back(q);
    }
}

/// Write the queued run's opening User turn now, so the thread shows what it
/// is waiting to say and a re-enqueue after a restart does not double it.
fn record_prompt(store: &SessionStore, q: &mut QueuedRun) {
    if q.prompt_recorded {
        return;
    }
    let text = crate::handler::user_event_text(&q.prompt, &q.attachments);
    match store.append(&q.session_id, &Event::User { text }) {
        Ok(()) => q.prompt_recorded = true,
        Err(e) => tracing::warn!("queued prompt for {} not recorded: {e}", q.session_id),
    }
}

/// Forget a run's parked copy once it launches, is killed, or is dropped.
pub(super) fn clear_queued(session_id: &str) {
    let mut sidecar = load_sidecar(session_id);
    if sidecar.queued.take().is_some() {
        save_sidecar(session_id, &sidecar);
    }
}

impl Orchestrator {
    /// R-5: put `Queued` sessions back on the queue after a restart. Their
    /// prompts live next to the transcript, so nothing is lost with the
    /// process. Sessions whose parked copy is gone fall back to Idle rather
    /// than sitting "queued" forever.
    pub async fn recover_queue(&self) -> Result<usize> {
        let mut n = 0;
        for m in self.store.list()? {
            if m.status != SessionStatus::Queued {
                continue;
            }
            let queued = load_sidecar(&m.id).queued;
            match queued {
                Some(p) => {
                    self.queue
                        .lock()
                        .await
                        .push_back(QueuedRun::from_persisted(&m.id, p));
                    n += 1;
                }
                None => self.store.set_status(&m.id, SessionStatus::Idle)?,
            }
        }
        if n > 0 {
            self.notify.notify_one();
        }
        Ok(n)
    }

    /// Long-lived pump loop. Spawn once per process; the driver task only
    /// notifies (sync — this keeps the async graph acyclic for Send).
    pub async fn pump_loop(self: Arc<Self>) {
        loop {
            self.notify.notified().await;
            Self::pump(self.pump_parts()).await;
        }
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
                // A child works on the parent's project, so it inherits its
                // budget (R-4).
                workspace_project: run_project(parent_id),
                prompt_recorded: false,
            };
            if q.workspace_project.is_some() {
                set_run_project(&meta.id, q.workspace_project.clone());
            }
            let live = {
                let mut h = self.handles.lock().await;
                h.retain(|_, handle| !handle._task.is_finished());
                h.len()
            };
            let snap = self.config();
            if live >= snap.orchestrator.max_concurrent.max(1) {
                if snap.orchestrator.queue_when_busy {
                    let _ = self.store.set_status(&meta.id, SessionStatus::Queued);
                    self.pump_parts().enqueue(q).await;
                }
            } else if let Err(e) = Self::launch(self.pump_parts(), q).await {
                let _ = self.store.append(
                    &meta.id,
                    &Event::System {
                        text: format!("subsession run failed to start: {e}"),
                    },
                );
            }
        }
        self.store.get(&meta.id)
    }

    /// Live-run count with finished-task pruning. Call wherever the
    /// concurrency cap or `is active` guard is read so a just-finished task
    /// that hasn't self-removed yet never blocks a new run.
    async fn live_count(p: &Pump) -> usize {
        let mut h = p.handles.lock().await;
        h.retain(|_, handle| !handle._task.is_finished());
        h.len()
    }

    /// Start queued runs while slots are free. Skips killed/missing sessions.
    async fn pump(p: Pump) {
        loop {
            let next = {
                let max = p.cfg_snapshot().orchestrator.max_concurrent.max(1);
                let live = Self::live_count(&p).await;
                if live >= max {
                    return;
                }
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
            // R-5: a run that cannot start is parked Idle by `launch` and its
            // error goes out on the bus; the queue keeps draining instead of
            // stalling behind one bad entry.
            if Self::launch(p.clone(), q).await.is_err() {
                continue;
            }
        }
    }

    /// Kick the pump (boot recovery, kills). No-op when nothing is queued.
    pub async fn kick(&self) {
        self.notify.notify_one();
    }

    pub(super) fn closed_rx() -> mpsc::UnboundedReceiver<RunEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(tx);
        rx
    }
}

impl Pump {
    pub(super) fn max_live(&self) -> usize {
        self.cfg_snapshot().orchestrator.max_concurrent.max(1)
    }

    /// Enqueue-or-launch a queued run, mirroring `Orchestrator::spawn`.
    /// Returns true when the run launched immediately (slot free).
    pub(super) async fn dispatch(&self, q: QueuedRun) -> bool {
        let live = Orchestrator::live_count(self).await;
        if live >= self.max_live() {
            if self.cfg_snapshot().orchestrator.queue_when_busy {
                let _ = self.store.set_status(&q.session_id, SessionStatus::Queued);
                self.enqueue(q).await;
            }
            return false;
        }
        Orchestrator::launch(self.clone(), q).await.is_ok()
    }
}
