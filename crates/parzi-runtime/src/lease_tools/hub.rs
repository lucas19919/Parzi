//! The local lease layer: one `LeaseTable` plus the per-run endpoints a
//! request has to reach. Round 1 is hub-less (PLAN §4) — the table lives in
//! this process, and "delivering" a request means the holder's own session,
//! not a daemon. H2 replaces the transport under this API, nothing above it.
//!
//! This file holds the type and the table operations; asking for a file is in
//! `requests.rs`, the write gate in `enforce.rs`.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use parzi_core::error::Result;
use parzi_core::journal::JournalKind;
use parzi_core::lease::{Claim, Holder, Lease, LeaseTable};
use parzi_core::plan::TaskId;
use parzi_core::store::SessionStore;
use tokio::sync::{mpsc, Mutex};

use crate::handler::{RunEvent, RunEventBus};
use crate::tools::Approver;

use super::naming::{held_by, join, now, same_holder, tool_err};
use super::requests::Pending;

/// No answer in this long is a denial (PLAN §4). Tests shorten it.
pub const ANSWER_TIMEOUT_SECS: u64 = 120;

/// Which project a run works in. Set by whoever dispatches the lane; without
/// it the run still leases, it just has nowhere to journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectKey {
    pub workspace: String,
    pub slug: String,
}

/// Everything the lease layer knows about one live run.
pub(super) struct RunEndpoint {
    pub(super) holder: Holder,
    /// Repo the run's worktree belongs to; lease paths are `<repo>/<rel>`.
    pub(super) repo: Option<String>,
    pub(super) project: Option<ProjectKey>,
    pub(super) notices: Option<mpsc::UnboundedSender<RunEvent>>,
    pub(super) approver: Option<Arc<dyn Approver>>,
    pub(super) store: Option<SessionStore>,
}

pub struct LeaseHub {
    pub(super) table: Arc<Mutex<LeaseTable>>,
    pub(super) runs: Mutex<HashMap<String, RunEndpoint>>,
    pub(super) pending: Mutex<HashMap<String, Pending>>,
    pub(super) answer_timeout: Duration,
    /// The host bus (R-5). A dispatched lane's own receiver is dropped by
    /// whoever started it, so a notice that only went to the run's channel
    /// reached nobody; the bus is the app's one ear on every run.
    pub(super) bus: Option<RunEventBus>,
}

impl Default for LeaseHub {
    fn default() -> Self {
        Self::new()
    }
}

impl LeaseHub {
    #[must_use]
    pub fn new() -> Self {
        Self {
            table: Arc::new(Mutex::new(LeaseTable::default())),
            runs: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            answer_timeout: Duration::from_secs(ANSWER_TIMEOUT_SECS),
            bus: None,
        }
    }

    /// Shorter wait for tests; production keeps the 120 s of PLAN §4.
    #[must_use]
    pub fn with_answer_timeout(mut self, d: Duration) -> Self {
        self.answer_timeout = d;
        self
    }

    /// Fan lease notices onto the host bus, so a dispatched lane whose own
    /// receiver was dropped still reaches the app (R-5).
    #[must_use]
    pub fn with_bus(mut self, bus: RunEventBus) -> Self {
        self.bus = Some(bus);
        self
    }

    /// The shared table itself, for the board, STATUS.md and the deck.
    #[must_use]
    pub fn table(&self) -> Arc<Mutex<LeaseTable>> {
        self.table.clone()
    }

    // ---- registration --------------------------------------------------

    /// Announce a run: its holder identity and the channels a request needs.
    pub async fn register(
        &self,
        run: &str,
        holder: Holder,
        notices: Option<mpsc::UnboundedSender<RunEvent>>,
        approver: Option<Arc<dyn Approver>>,
        store: Option<SessionStore>,
    ) {
        self.runs.lock().await.insert(
            run.to_string(),
            RunEndpoint {
                holder,
                repo: None,
                project: None,
                notices,
                approver,
                store,
            },
        );
    }

    /// Bind the run to a project (journal + `critical:` globs live there).
    pub async fn bind_project(&self, run: &str, workspace: &str, slug: &str) {
        if let Some(e) = self.runs.lock().await.get_mut(run) {
            e.project = Some(ProjectKey {
                workspace: workspace.to_string(),
                slug: slug.to_string(),
            });
        }
    }

    /// Bind the run's worktree to a repo, so `fs.write src/x.rs` becomes the
    /// lease path `<repo>/src/x.rs`. An unbound run is never gated.
    pub async fn bind_repo(&self, run: &str, repo: &str) {
        if let Some(e) = self.runs.lock().await.get_mut(run) {
            e.repo = Some(repo.to_string());
        }
    }

    /// Drop the run and release what it held — a finished lane never keeps a
    /// file hostage (§15.6: a crash waits for the TTL, a clean end does not).
    pub async fn unregister(&self, run: &str) {
        let Some(ep) = self.runs.lock().await.remove(run) else {
            return;
        };
        let mut t = self.table.lock().await;
        let mine: Vec<TaskId> = t
            .leases()
            .filter(|l| same_holder(&l.holder, &ep.holder))
            .map(|l| l.task.clone())
            .collect();
        for task in mine {
            t.release(&task);
        }
    }

    pub async fn holder_of_run(&self, run: &str) -> Option<Holder> {
        self.runs.lock().await.get(run).map(|e| e.holder.clone())
    }

    pub async fn repo_of_run(&self, run: &str) -> Option<String> {
        self.runs.lock().await.get(run).and_then(|e| e.repo.clone())
    }

    pub async fn store_of_run(&self, run: &str) -> Option<SessionStore> {
        self.runs
            .lock()
            .await
            .get(run)
            .and_then(|e| e.store.clone())
    }

    pub async fn project_of_run(&self, run: &str) -> Option<ProjectKey> {
        self.runs
            .lock()
            .await
            .get(run)
            .and_then(|e| e.project.clone())
    }

    // ---- leases ---------------------------------------------------------

    /// Claim `paths` (already `<repo>/<rel>`) for `task`. A held path comes
    /// back naming its holder, never as a silent overwrite.
    pub async fn claim(
        &self,
        run: &str,
        task: &TaskId,
        paths: BTreeSet<String>,
    ) -> Result<serde_json::Value> {
        let holder = self
            .holder_of_run(run)
            .await
            .ok_or_else(|| tool_err("this run has no lease identity"))?;
        let claim = {
            let mut t = self.table.lock().await;
            t.expire(now());
            t.claim(task.clone(), holder, paths.clone())
        };
        match claim {
            Claim::Granted => {
                self.journal(
                    run,
                    JournalKind::Claim,
                    Some(task),
                    &format!("claimed {} path(s): {}", paths.len(), join(&paths)),
                )
                .await;
                Ok(serde_json::json!({
                    "status": "granted",
                    "task": task.as_str(),
                    "paths": paths.iter().collect::<Vec<_>>(),
                }))
            }
            Claim::Held { by } => {
                self.journal(
                    run,
                    JournalKind::Claim,
                    Some(task),
                    &format!("claim refused: {}", held_by(&by)),
                )
                .await;
                Err(tool_err(&format!(
                    "held by {} — ask with lease.request {{path, for_task, reason}}",
                    held_by(&by)
                )))
            }
        }
    }

    pub async fn release(&self, run: &str, task: &TaskId) -> Result<()> {
        let held = {
            let mut t = self.table.lock().await;
            t.release(task)
        };
        let text = match &held {
            Some(l) => format!("released {} path(s)", l.paths.len()),
            None => "released (nothing was held)".to_string(),
        };
        self.journal(run, JournalKind::Release, Some(task), &text)
            .await;
        Ok(())
    }

    /// Keep this run's leases alive. Called on every gated write, so a lane
    /// that is working never expires under its own feet.
    pub async fn heartbeat_run(&self, run: &str) {
        let Some(holder) = self.holder_of_run(run).await else {
            return;
        };
        let mut t = self.table.lock().await;
        let mine: Vec<TaskId> = t
            .leases()
            .filter(|l| same_holder(&l.holder, &holder))
            .map(|l| l.task.clone())
            .collect();
        for task in mine {
            t.heartbeat(&task);
        }
    }

    /// The lease covering `path`, if any (expired ones swept first).
    pub async fn holder_of_path(&self, path: &str) -> Option<Lease> {
        let mut t = self.table.lock().await;
        t.expire(now());
        t.holder_of(path).cloned()
    }

    /// Every live lease, for the board and the deck.
    pub async fn snapshot(&self) -> Vec<Lease> {
        let mut t = self.table.lock().await;
        t.expire(now());
        t.leases().cloned().collect()
    }

    // ---- shared plumbing -------------------------------------------------

    pub(super) async fn run_of_holder(&self, holder: &Holder) -> Option<String> {
        // The lease records the holder's run when it has one; otherwise the
        // live endpoints are matched by identity (a lane that moved machines).
        if let Some(run) = holder.run.clone() {
            if self.runs.lock().await.contains_key(&run) {
                return Some(run);
            }
        }
        self.runs
            .lock()
            .await
            .iter()
            .find(|(_, e)| same_holder(&e.holder, holder))
            .map(|(id, _)| id.clone())
    }

    pub(super) async fn approver_of(&self, run: Option<&str>) -> Option<Arc<dyn Approver>> {
        let run = run?;
        self.runs
            .lock()
            .await
            .get(run)
            .and_then(|e| e.approver.clone())
    }

    pub(super) async fn notify(&self, run: &str, event: RunEvent) {
        let tx = self
            .runs
            .lock()
            .await
            .get(run)
            .and_then(|e| e.notices.clone());
        if let Some(bus) = &self.bus {
            let _ = bus.send((run.to_string(), event.clone()));
        }
        if let Some(tx) = tx {
            if tx.send(event).is_err() {
                tracing::debug!("lease notice dropped: run {run} is no longer listening");
            }
        }
    }

    /// Append one line to the project's journal. A run with no project (a
    /// plain thread) has nowhere to write, and that is fine.
    pub async fn journal(&self, run: &str, kind: JournalKind, task: Option<&TaskId>, text: &str) {
        let Some(key) = self.project_of_run(run).await else {
            return;
        };
        let who = match self.holder_of_run(run).await {
            Some(h) => format!("{} · lane {}", h.user, h.lane),
            None => run.to_string(),
        };
        let line = parzi_core::journal::JournalLine::now(who, kind, task.cloned(), text);
        if let Err(e) = parzi_core::journal::append(&key.workspace, &key.slug, &line) {
            tracing::warn!("journal append failed: {e}");
        }
    }
}
