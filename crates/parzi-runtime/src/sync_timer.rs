//! Keeps open workspaces in step with their git remote (hub PLAN.md §1.1).
//!
//! Two rhythms, both cheap: a **pull** every 60 s while a workspace is open,
//! so a teammate's plan arrives without anyone pressing anything, and a
//! **commit + push** right after a write inside the workspace (a draft, a
//! PLAN.md, a journal line). Writers announce themselves with [`SyncTimer::touch`];
//! anything that writes without announcing is still caught by an mtime poll
//! on the same 10 s tick, so no change is lost — it is only later.
//!
//! Git blocks, so every call goes through `spawn_blocking`. Conflicts are
//! never resolved here: the report carries them and the deck shows them.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use parzi_core::workspace::{self, sync::SyncReport};

/// How often an open workspace takes what teammates wrote.
pub const PULL_EVERY: Duration = Duration::from_secs(60);
/// How often the timer looks at its workspaces at all.
pub const TICK: Duration = Duration::from_secs(10);
/// Directory entries one mtime poll will look at. A workspace is plans and
/// capsules, not a code tree; the cap keeps a stray checkout from costing.
const POLL_ENTRY_CAP: usize = 4000;

/// What one workspace needs on this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Work {
    /// Nothing to do.
    Idle,
    /// Local writes to hand over.
    Push,
    /// Pull first, then hand over.
    Full,
}

#[derive(Debug, Default)]
struct State {
    open: bool,
    /// A write happened since the last successful push.
    dirty: bool,
    last_pull: Option<Instant>,
    /// Set after a failed sync: nothing is tried again before this, so an
    /// offline machine does not push once a tick.
    retry_after: Option<Instant>,
    seen_mtime: Option<SystemTime>,
    last_report: Option<SyncReport>,
    last_error: Option<String>,
}

impl State {
    fn work(&self, now: Instant) -> Work {
        if !self.open {
            return Work::Idle;
        }
        if self.retry_after.is_some_and(|t| now < t) {
            return Work::Idle;
        }
        let due = self
            .last_pull
            .is_none_or(|t| now.duration_since(t) >= PULL_EVERY);
        match (due, self.dirty) {
            (true, _) => Work::Full,
            (false, true) => Work::Push,
            (false, false) => Work::Idle,
        }
    }
}

#[derive(Default)]
struct Inner {
    states: Mutex<BTreeMap<String, State>>,
}

impl Inner {
    fn with<R>(&self, workspace: &str, f: impl FnOnce(&mut State) -> R) -> R {
        let mut states = self.states.lock().unwrap_or_else(|e| e.into_inner());
        f(states.entry(workspace.to_string()).or_default())
    }
}

/// The running timer. Dropping it stops the task; the workspaces stay on disk.
pub struct SyncTimer {
    inner: Arc<Inner>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for SyncTimer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl SyncTimer {
    /// Start the ticker. Nothing happens until a workspace is opened.
    pub fn start() -> Self {
        let inner = Arc::new(Inner::default());
        let task = tokio::spawn(run(inner.clone()));
        Self { inner, task }
    }

    /// A workspace is on screen: pull it on the timer from now on.
    pub fn open(&self, workspace: &str) {
        self.inner.with(workspace, |s| {
            s.open = true;
            // First tick pulls, so an opened workspace is current at once.
            s.last_pull = None;
        });
    }

    /// Nobody is looking at this workspace any more.
    pub fn close(&self, workspace: &str) {
        self.inner.with(workspace, |s| s.open = false);
    }

    /// Something inside the workspace was written — commit and push it on the
    /// next tick. Called by every writer that journals (drafts, PLAN.md,
    /// approvals, capsules).
    pub fn touch(&self, workspace: &str) {
        self.inner.with(workspace, |s| s.dirty = true);
    }

    /// The last sync of this workspace, for the deck's status line.
    pub fn report(&self, workspace: &str) -> Option<SyncReport> {
        self.inner.with(workspace, |s| s.last_report.clone())
    }

    /// The last failure (offline, no credential, detached HEAD), if any.
    pub fn last_error(&self, workspace: &str) -> Option<String> {
        self.inner.with(workspace, |s| s.last_error.clone())
    }

    /// Sync one workspace now and wait for it, then fold the result into the
    /// timer's state so the next tick does not repeat the work.
    pub async fn sync_now(&self, workspace: &str) -> parzi_core::error::Result<SyncReport> {
        let result = run_sync(workspace.to_string(), Work::Full).await;
        record(&self.inner, workspace, Work::Full, &result);
        result
    }
}

/// The host's one timer, started the first time anybody asks for it. A host
/// that never touches a workspace never spawns the task. Must be called from
/// inside the tokio runtime (every Tauri command is).
///
/// # Panics
/// If no tokio runtime is running.
pub fn global() -> &'static SyncTimer {
    static TIMER: std::sync::OnceLock<SyncTimer> = std::sync::OnceLock::new();
    TIMER.get_or_init(SyncTimer::start)
}

/// One full sync of one workspace now — what the `workspace_sync` command
/// forwards to. Asking for a sync also says "I am working in this workspace",
/// so the 60 s pull keeps it in step afterwards without a second control;
/// [`SyncTimer::close`] stops that again.
pub async fn sync_workspace(workspace: &str) -> parzi_core::error::Result<SyncReport> {
    let timer = global();
    timer.open(workspace);
    timer.sync_now(workspace).await
}

/// The tick loop. One workspace at a time: git is disk- and network-bound and
/// a workspace is small, so there is nothing to win by fanning out.
async fn run(inner: Arc<Inner>) {
    loop {
        tokio::time::sleep(TICK).await;
        let names: Vec<String> = {
            let states = inner.states.lock().unwrap_or_else(|e| e.into_inner());
            states
                .iter()
                .filter(|(_, s)| s.open)
                .map(|(n, _)| n.clone())
                .collect()
        };
        for name in names {
            poll_mtime(&inner, &name).await;
            let work = inner.with(&name, |s| s.work(Instant::now()));
            if work == Work::Idle {
                continue;
            }
            let result = run_sync(name.clone(), work).await;
            record(&inner, &name, work, &result);
        }
    }
}

/// Fold one sync's outcome back into the state: a sync that ran at all clears
/// the pending write, a sync that failed sets a minute of quiet.
fn record(
    inner: &Arc<Inner>,
    workspace: &str,
    work: Work,
    result: &parzi_core::error::Result<SyncReport>,
) {
    inner.with(workspace, |s| {
        // A push-only tick must not postpone the minute pull, or a workspace
        // that is written every half minute would never take anything in.
        if work == Work::Full {
            s.last_pull = Some(Instant::now());
        }
        match result {
            Ok(report) => {
                // Even a conflict clears `dirty`: nothing can be handed over
                // until a person edits the file, and that edit re-arms this
                // through `touch` or the mtime poll.
                s.dirty = false;
                s.retry_after = None;
                s.last_report = Some(report.clone());
                s.last_error = None;
            }
            Err(e) => {
                // Offline, no credential, detached HEAD: keep the work and
                // come back in a minute, not on every tick.
                s.retry_after = Some(Instant::now() + PULL_EVERY);
                s.last_error = Some(e.to_string());
            }
        }
    });
}

async fn run_sync(workspace: String, work: Work) -> parzi_core::error::Result<SyncReport> {
    let message = format!("parzi: {workspace} sync");
    tokio::task::spawn_blocking(move || {
        let ws = workspace::load(&workspace)?;
        match work {
            Work::Idle => Ok(SyncReport::default()),
            Work::Push => workspace::sync::commit_and_push(&ws, &message),
            Work::Full => workspace::sync::sync_now(&ws, &message),
        }
    })
    .await
    .map_err(|e| {
        parzi_core::error::ParziError::Tool("workspace.sync".into(), format!("sync task: {e}"))
    })?
}

/// The safety net under `touch`: any write inside the workspace shows up as a
/// newer mtime within one tick.
async fn poll_mtime(inner: &Arc<Inner>, workspace: &str) {
    let dir = workspace::dir(workspace);
    let newest = tokio::task::spawn_blocking(move || newest_mtime(&dir))
        .await
        .unwrap_or(None);
    let Some(newest) = newest else { return };
    inner.with(workspace, |s| {
        if s.seen_mtime.is_some_and(|seen| newest > seen) {
            s.dirty = true;
        }
        if s.seen_mtime.is_none_or(|seen| newest > seen) {
            s.seen_mtime = Some(newest);
        }
    });
}

/// Newest mtime under `dir`, skipping `.git` (git's own writes are not the
/// user's) and stopping at [`POLL_ENTRY_CAP`] entries.
fn newest_mtime(dir: &Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0usize;
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > POLL_ENTRY_CAP {
                return newest;
            }
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
                continue;
            }
            if let Ok(modified) = meta.modified() {
                if newest.is_none_or(|n| modified > n) {
                    newest = Some(modified);
                }
            }
        }
    }
    newest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_workspaces_are_never_worked() {
        let s = State::default();
        assert_eq!(s.work(Instant::now()), Work::Idle);
    }

    #[test]
    fn a_write_pushes_between_pulls() {
        let now = Instant::now();
        let mut s = State {
            open: true,
            last_pull: Some(now),
            ..State::default()
        };
        assert_eq!(s.work(now), Work::Idle);
        s.dirty = true;
        assert_eq!(s.work(now), Work::Push);
    }

    #[test]
    fn a_failed_sync_waits_before_it_tries_again() {
        let now = Instant::now();
        let s = State {
            open: true,
            dirty: true,
            retry_after: Some(now + PULL_EVERY),
            ..State::default()
        };
        assert_eq!(s.work(now), Work::Idle, "offline: not once a tick");
        assert_eq!(s.work(now + PULL_EVERY), Work::Full, "and then again");
    }

    #[test]
    fn an_open_workspace_pulls_on_the_minute() {
        let now = Instant::now();
        let s = State {
            open: true,
            last_pull: Some(now - PULL_EVERY),
            ..State::default()
        };
        assert_eq!(s.work(now), Work::Full);
        let fresh = State {
            open: true,
            ..State::default()
        };
        assert_eq!(fresh.work(now), Work::Full, "first tick after open pulls");
    }

    #[test]
    fn newest_mtime_ignores_the_git_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join(".git/HEAD"), "ref: x\n").unwrap();
        assert_eq!(newest_mtime(tmp.path()), None, "only .git exists");
        std::fs::create_dir_all(tmp.path().join("projects/a")).unwrap();
        std::fs::write(tmp.path().join("projects/a/PLAN.md"), "parzi: 1\n").unwrap();
        assert!(newest_mtime(tmp.path()).is_some());
    }
}
