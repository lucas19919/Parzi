use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Event, RunCause, SessionStatus, SessionStore};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex, Notify};

use crate::handler::{RunEvent, RunEventBus};
use crate::mcp::McpManager;
use crate::mcp_host::McpHost;
use crate::status::{ProviderSource, StatusBoard};
use crate::tools::Approver;

use super::{Handle, Orchestrator};

pub const BUDGET_NOTE: &str = "budget_exceeded";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RunSidecar {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    queued: Option<PersistedRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<VendorSession>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brief_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    report_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last: Option<Posture>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Posture {
    #[serde(default)]
    pub(crate) mode_override: Option<String>,
    #[serde(default)]
    pub(crate) effort: String,
    #[serde(default = "yes")]
    pub(crate) ask: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VendorSession {
    pub provider: String,
    pub resume: serde_json::Value,
}

impl RunSidecar {
    fn is_empty(&self) -> bool {
        self.queued.is_none()
            && self.note.is_none()
            && self.session.is_none()
            && self.brief_hash.is_none()
            && self.report_to.is_none()
            && self.last.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRun {
    project: String,
    model_spec: String,
    prompt: String,
    cwd: String,
    effort: String,
    #[serde(default)]
    prompt_recorded: bool,
    #[serde(default)]
    mode_override: Option<String>,
    #[serde(default)]
    inbox_from: Option<usize>,
    #[serde(default)]
    attachments: Vec<String>,
    #[serde(default)]
    ask: bool,
    #[serde(default = "restarted")]
    cause: RunCause,
}

fn restarted() -> RunCause {
    RunCause::Restart
}

fn sidecar_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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

pub fn set_run_note(session_id: &str, note: Option<&str>) {
    let _write = sidecar_lock();
    let mut sidecar = load_sidecar(session_id);
    let next = note.map(str::to_string);
    if sidecar.note == next {
        return;
    }
    sidecar.note = next;
    save_sidecar(session_id, &sidecar);
}

#[must_use]
pub fn run_note(session_id: &str) -> Option<String> {
    load_sidecar(session_id).note
}

pub fn set_run_session(session_id: &str, session: Option<VendorSession>) {
    let _write = sidecar_lock();
    let mut sidecar = load_sidecar(session_id);
    if sidecar.session == session {
        return;
    }
    sidecar.session = session;
    save_sidecar(session_id, &sidecar);
}

#[must_use]
pub fn run_session(session_id: &str) -> Option<VendorSession> {
    load_sidecar(session_id).session
}

fn edit_sidecar<R>(session_id: &str, f: impl FnOnce(&mut RunSidecar) -> R) -> R {
    let _write = sidecar_lock();
    let mut sidecar = load_sidecar(session_id);
    let before = serde_json::to_value(&sidecar).ok();
    let out = f(&mut sidecar);
    if serde_json::to_value(&sidecar).ok() != before {
        save_sidecar(session_id, &sidecar);
    }
    out
}

pub(crate) fn brief_hash(session_id: &str) -> Option<String> {
    load_sidecar(session_id).brief_hash
}

pub(crate) fn set_brief_hash(session_id: &str, hash: Option<&str>) {
    edit_sidecar(session_id, |s| s.brief_hash = hash.map(str::to_string));
}

pub(crate) fn set_report_to(session_id: &str, parent: &str) {
    edit_sidecar(session_id, |s| s.report_to = Some(parent.to_string()));
}

pub(crate) fn take_report_to(session_id: &str) -> Option<String> {
    edit_sidecar(session_id, |s| s.report_to.take())
}

pub(crate) fn posture(session_id: &str) -> Option<Posture> {
    load_sidecar(session_id).last
}

pub(crate) fn set_posture(session_id: &str, posture: Posture) {
    edit_sidecar(session_id, |s| s.last = Some(posture));
}

pub(super) struct QueuedRun {
    pub(super) session_id: String,
    pub(super) project: String,
    pub(super) model_spec: String,
    pub(super) prompt: String,
    pub(super) cwd: String,
    pub(super) effort: String,
    pub(super) attachments: Vec<parzi_core::context::AttachedFile>,
    pub(super) approver: Option<Arc<dyn Approver>>,
    pub(super) restore_approver: bool,
    pub(super) mode_override: Option<String>,
    pub(super) prompt_recorded: bool,
    pub(super) inbox_from: Option<usize>,
    pub(super) cause: RunCause,
}

impl QueuedRun {
    fn persisted(&self) -> PersistedRun {
        PersistedRun {
            project: self.project.clone(),
            model_spec: self.model_spec.clone(),
            prompt: self.prompt.clone(),
            cwd: self.cwd.clone(),
            effort: self.effort.clone(),
            prompt_recorded: self.prompt_recorded,
            mode_override: self.mode_override.clone(),
            inbox_from: self.inbox_from,
            attachments: self.attachments.iter().map(|a| a.path.clone()).collect(),
            ask: self.approver.is_some() || self.restore_approver,
            cause: self.cause,
        }
    }

    fn from_persisted(session_id: &str, p: PersistedRun) -> Self {
        Self {
            session_id: session_id.to_string(),
            project: p.project,
            model_spec: p.model_spec,
            prompt: p.prompt,
            cwd: p.cwd,
            effort: p.effort,
            attachments: p
                .attachments
                .into_iter()
                .map(|path| parzi_core::context::AttachedFile {
                    path,
                    snippet: String::new(),
                    image: None,
                })
                .collect(),
            approver: None,
            restore_approver: p.ask,
            prompt_recorded: p.prompt_recorded,
            mode_override: p.mode_override,
            inbox_from: p.inbox_from,
            cause: p.cause,
        }
    }
}

#[derive(Clone)]
pub(super) struct Pump {
    pub(super) queue: Arc<Mutex<VecDeque<QueuedRun>>>,
    pub(super) notify: Arc<Notify>,
    pub(super) cfg: std::sync::Arc<std::sync::RwLock<ParziConfig>>,
    pub(super) source: ProviderSource,
    pub(super) mcp: Arc<McpManager>,
    pub(super) store: SessionStore,
    pub(super) handles: Arc<Mutex<HashMap<String, Handle>>>,
    pub(super) status: Arc<StatusBoard>,
    pub(super) tools_server: Arc<tokio::sync::OnceCell<Option<Arc<McpHost>>>>,
    pub(super) bus: RunEventBus,
    pub(super) marks: super::ReadMarks,
    pub(super) asker: Arc<Mutex<Option<Arc<dyn crate::tools::Asker>>>>,
    pub(super) approver: super::HostApprover,
    pub(super) shells: Arc<std::sync::Mutex<HashMap<String, Arc<crate::shell::ShellRegistry>>>>,
    pub(super) wakes: super::wake::Wakes,
}

impl Pump {
    pub(super) fn shells_for(&self, sid: &str) -> Arc<crate::shell::ShellRegistry> {
        let mut shells = self.shells.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(reg) = shells.get(sid) {
            return reg.clone();
        }
        let reg = Arc::new(crate::shell::ShellRegistry::new(sid));
        shells.insert(sid.to_string(), reg.clone());
        reg
    }
}

impl Pump {
    pub(super) fn host_approver(&self) -> Option<Arc<dyn Approver>> {
        self.approver.read().ok().and_then(|a| a.clone())
    }

    pub(super) fn cfg_snapshot(&self) -> ParziConfig {
        self.cfg.read().map(|c| c.clone()).unwrap_or_default()
    }

    pub(super) async fn tools_server(&self) -> Option<Arc<McpHost>> {
        self.tools_server
            .get_or_init(|| async {
                match McpHost::start().await {
                    Ok(h) => Some(h),
                    Err(e) => {
                        tracing::warn!("Parzi tool server did not start: {e}");
                        None
                    }
                }
            })
            .await
            .clone()
    }

    pub(super) async fn enqueue(&self, mut q: QueuedRun) {
        record_prompt(&self.store, &mut q);
        {
            let _write = sidecar_lock();
            let mut sidecar = load_sidecar(&q.session_id);
            sidecar.queued = Some(q.persisted());
            save_sidecar(&q.session_id, &sidecar);
        }
        self.queue.lock().await.push_back(q);
        self.notify.notify_one();
    }
}

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

pub(super) fn clear_queued(session_id: &str) {
    let _write = sidecar_lock();
    let mut sidecar = load_sidecar(session_id);
    if sidecar.queued.take().is_some() {
        save_sidecar(session_id, &sidecar);
    }
}

impl Orchestrator {
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
                None => {
                    self.store.set_status(&m.id, SessionStatus::Idle)?;
                }
            }
        }
        if n > 0 {
            self.notify.notify_one();
        }
        Ok(n)
    }

    pub async fn pump_loop(self: Arc<Self>) {
        loop {
            self.notify.notified().await;
            Self::pump(self.pump_parts()).await;
        }
    }

    async fn live_count(p: &Pump) -> usize {
        let mut h = p.handles.lock().await;
        h.retain(|_, handle| !handle.finished());
        h.len()
    }

    async fn pump(p: Pump) {
        loop {
            let next = {
                let max = p.max_live();
                let busy: std::collections::HashSet<String> = {
                    let mut h = p.handles.lock().await;
                    h.retain(|_, handle| !handle.finished());
                    if h.len() >= max {
                        return;
                    }
                    h.keys().cloned().collect()
                };
                let mut queue = p.queue.lock().await;
                let at = queue.iter().position(|r| !busy.contains(&r.session_id));
                at.and_then(|i| queue.remove(i))
            };
            let Some(q) = next else { return };
            let killed = match p.store.get(&q.session_id) {
                Ok(m) => m.status == SessionStatus::Killed,
                Err(_) => true,
            };
            if !killed {
                let _ = Self::launch(p.clone(), q).await;
            }
        }
    }

    pub async fn kick(&self) {
        self.notify.notify_one();
    }

    pub(super) fn closed_rx() -> mpsc::UnboundedReceiver<RunEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        drop(tx);
        rx
    }
}

pub(super) enum Dispatch {
    Launched,
    Queued,
    Busy,
    Failed(ParziError),
}

impl Pump {
    fn max_live(&self) -> usize {
        self.cfg_snapshot().orchestrator.max_concurrent.max(1)
    }

    pub(super) async fn dispatch(&self, q: QueuedRun) -> Dispatch {
        let live = Orchestrator::live_count(self).await;
        if live >= self.max_live() {
            if !self.cfg_snapshot().orchestrator.queue_when_busy {
                return Dispatch::Busy;
            }
            let _ = self.store.set_status(&q.session_id, SessionStatus::Queued);
            self.enqueue(q).await;
            return Dispatch::Queued;
        }
        match Orchestrator::launch(self.clone(), q).await {
            Ok(super::launch::Started::Running(_)) => Dispatch::Launched,
            Ok(super::launch::Started::Queued) => Dispatch::Queued,
            Err(e) => Dispatch::Failed(e),
        }
    }
}
