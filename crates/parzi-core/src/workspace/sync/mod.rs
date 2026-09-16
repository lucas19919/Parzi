//! The round-1 syncer: the workspace repo itself (hub PLAN.md §1.1).
//!
//! A workspace is a small git repository at `~/.parzi/workspaces/<name>`.
//! Sharing a project with a teammate is a commit and a push; there is no
//! daemon in this round. Everything goes through the `git` CLI, like
//! `parzi-runtime::git_worktree` does — no libgit2, and no credential
//! handling of our own (`GIT_TERMINAL_PROMPT=0`, so a missing credential
//! fails fast instead of hanging a background timer).
//!
//! Conflict policy (§15.2): PLAN.md and PROJECT.md are never auto-merged.
//! A conflicting pull leaves the merge in progress with the markers in the
//! file and reports the paths; `commit_and_push` refuses to commit while a
//! marker is still there, so a half-resolved plan never reaches a teammate.
//! A human or the orchestrator resolves the file and the next commit closes
//! the merge.
//!
//! This is the one place in `parzi-core` that reaches the network (`fetch`,
//! `push`), and it does so by spawning git, never by opening a socket.
//! `git.rs` next door holds the plumbing; this file is the policy.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::Workspace;
use crate::error::{ParziError, Result};

mod git;

use git::{
    current_branch, ensure_identity, err, git, has_staged_changes, marker_paths, merge_in_progress,
    require_repo, run_git, unmerged_paths,
};
pub use git::{is_repo, remote_url};

/// What one sync did. Facts about this call, not stored state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    /// A commit was created in the workspace repo.
    pub committed: bool,
    /// The commit(s) reached `origin`.
    pub pushed: bool,
    /// Something new arrived from `origin`.
    pub pulled: bool,
    /// Repo-relative paths left for a human: carrying conflict markers, or
    /// unmerged and unreadable. Non-empty means nothing was committed.
    pub conflicts: Vec<String>,
}

impl SyncReport {
    fn absorb(&mut self, other: SyncReport) {
        self.committed |= other.committed;
        self.pushed |= other.pushed;
        self.pulled |= other.pulled;
        for c in other.conflicts {
            if !self.conflicts.contains(&c) {
                self.conflicts.push(c);
            }
        }
    }
}

/// Everything a human still has to look at: a file carrying conflict markers,
/// or an unmerged path git could not even hand over as text (binary, or gone
/// on one side). An unmerged text file somebody has already cleaned counts as
/// resolved — the next `add -A` closes its stage and the merge commit lands.
pub fn conflicts_at(dir: &Path) -> Result<Vec<String>> {
    let mut all = marker_paths(dir)?;
    for path in unmerged_paths(dir)? {
        let readable = std::fs::read_to_string(dir.join(&path)).is_ok();
        if !readable && !all.contains(&path) {
            all.push(path);
        }
    }
    all.sort();
    Ok(all)
}

fn dir_of(ws: &Workspace) -> PathBuf {
    super::dir(&ws.name)
}

/// Create the workspace repo and commit `workspace.toml`. Idempotent: an
/// existing repo keeps its history and `remote` is (re)pointed at `origin`.
/// Never pushes — the caller decides when the first push happens.
pub fn init(ws: &Workspace, remote: Option<String>) -> Result<SyncReport> {
    let dir = dir_of(ws);
    init_at(&dir, remote.as_deref())
}

/// Directory-level `init`, so the wizard and the tests can work on a path.
pub fn init_at(dir: &Path, remote: Option<&str>) -> Result<SyncReport> {
    std::fs::create_dir_all(dir).map_err(ParziError::Io)?;
    if !is_repo(dir) {
        git(dir, &["init"])?;
        // `init -b main` needs git >= 2.28; this works on every version and
        // only bites while the repo has no commits.
        git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"])?;
    }
    ensure_identity(dir)?;
    if let Some(url) = remote.map(str::trim).filter(|u| !u.is_empty()) {
        if remote_url(dir).is_some() {
            git(dir, &["remote", "set-url", "origin", url])?;
        } else {
            git(dir, &["remote", "add", "origin", url])?;
        }
    }
    let mut report = SyncReport::default();
    git(dir, &["add", "-A", "--", "."])?;
    if has_staged_changes(dir)? {
        git(dir, &["commit", "-m", "parzi: workspace created"])?;
        report.committed = true;
    }
    Ok(report)
}

/// A workspace directory that was written before anyone called [`init`] is
/// still a pile of files worth sharing, so the workspace-level entry points
/// below initialise it on first use instead of failing. Already a repo: does
/// nothing. The directory-level `*_at` functions stay strict.
fn ensure_at(dir: &Path) -> Result<SyncReport> {
    if is_repo(dir) {
        return Ok(SyncReport::default());
    }
    init_at(dir, None)
}

/// Fetch and merge `origin`. A conflict is reported, never resolved: the
/// merge stays in progress with the markers in the files.
pub fn pull(ws: &Workspace) -> Result<SyncReport> {
    let dir = dir_of(ws);
    let mut report = ensure_at(&dir)?;
    report.absorb(pull_at(&dir)?);
    note_conflicts(&ws.name, &report);
    Ok(report)
}

/// Directory-level `pull`.
pub fn pull_at(dir: &Path) -> Result<SyncReport> {
    require_repo(dir)?;
    let mut report = SyncReport::default();
    if remote_url(dir).is_none() {
        // Solo workspace: nothing to pull, and that is not an error.
        return Ok(report);
    }
    let open = conflicts_at(dir)?;
    if !open.is_empty() {
        // A merge is still open; pulling again would only pile on.
        report.conflicts = open;
        return Ok(report);
    }
    if merge_in_progress(dir) {
        // Resolved on disk but not yet committed. Git refuses a second merge
        // on top of that, and rightly: the commit that closes this one comes
        // first, so leave the round to `commit_and_push`.
        return Ok(report);
    }
    git(dir, &["fetch", "origin", "--prune"])?;
    let branch = current_branch(dir)?;
    let upstream = format!("origin/{branch}");
    if !run_git(dir, &["rev-parse", "--verify", "--quiet", &upstream])?.ok {
        // Never pushed: there is no remote branch yet.
        return Ok(report);
    }
    if git(dir, &["rev-list", "--count", &format!("HEAD..{upstream}")])? == "0" {
        return Ok(report);
    }
    // A merge commit needs an identity just like a normal one.
    ensure_identity(dir)?;
    let merge = run_git(dir, &["merge", "--no-edit", &upstream])?;
    if merge.ok {
        report.pulled = true;
        return Ok(report);
    }
    let conflicts = conflicts_at(dir)?;
    if conflicts.is_empty() {
        return Err(err(format!(
            "git merge {upstream} failed: {}",
            if merge.stderr.is_empty() {
                merge.stdout
            } else {
                merge.stderr
            }
        )));
    }
    report.conflicts = conflicts;
    Ok(report)
}

/// Stage everything inside the workspace dir, commit when there is something
/// to commit, push when an `origin` exists. A solo workspace without a remote
/// commits locally and reports `pushed: false` — not an error.
pub fn commit_and_push(ws: &Workspace, message: &str) -> Result<SyncReport> {
    let dir = dir_of(ws);
    let mut report = ensure_at(&dir)?;
    report.absorb(commit_and_push_at(&dir, message)?);
    note_conflicts(&ws.name, &report);
    Ok(report)
}

/// Directory-level `commit_and_push`.
pub fn commit_and_push_at(dir: &Path, message: &str) -> Result<SyncReport> {
    require_repo(dir)?;
    let mut report = commit_at(dir, message)?;
    if !report.conflicts.is_empty() {
        return Ok(report);
    }
    report.absorb(push_at(dir)?);
    Ok(report)
}

/// Stage everything inside the workspace dir and commit it, unless a file is
/// still waiting for a human — a half-resolved plan must not become a commit.
/// Staging is also what closes an open merge, so a resolved conflict lands
/// here as the merge commit.
fn commit_at(dir: &Path, message: &str) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    let conflicts = conflicts_at(dir)?;
    if !conflicts.is_empty() {
        report.conflicts = conflicts;
        return Ok(report);
    }
    ensure_identity(dir)?;
    git(dir, &["add", "-A", "--", "."])?;
    if has_staged_changes(dir)? || merge_in_progress(dir) {
        let msg = message.trim();
        let msg = if msg.is_empty() { "parzi: sync" } else { msg };
        git(dir, &["commit", "-m", msg])?;
        report.committed = true;
    }
    Ok(report)
}

/// Hand the branch over to `origin`. No remote is not an error (§1.1: a solo
/// workspace is a local repo). A rejection means a teammate pushed first, so
/// pull once and try again; a conflict from that pull stops the push.
fn push_at(dir: &Path) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    if remote_url(dir).is_none() {
        return Ok(report);
    }
    let branch = current_branch(dir)?;
    let refspec = format!("HEAD:refs/heads/{branch}");
    let push = run_git(dir, &["push", "origin", &refspec])?;
    if push.ok {
        report.pushed = true;
        return Ok(report);
    }
    report.absorb(pull_at(dir)?);
    if !report.conflicts.is_empty() {
        return Ok(report);
    }
    // The merge commit the pull just made still has to go out.
    let retry = run_git(dir, &["push", "origin", &refspec])?;
    if !retry.ok {
        return Err(err(format!(
            "git push origin {refspec} failed: {}",
            if retry.stderr.is_empty() {
                retry.stdout
            } else {
                retry.stderr
            }
        )));
    }
    report.pushed = true;
    Ok(report)
}

/// One round: commit what we wrote, take what teammates wrote, hand ours
/// over. Local work is committed *before* the pull because git refuses to
/// merge over uncommitted changes. This is what the sync timer and the
/// `workspace_sync` command call.
pub fn sync_now(ws: &Workspace, message: &str) -> Result<SyncReport> {
    let dir = dir_of(ws);
    let mut report = ensure_at(&dir)?;
    report.absorb(sync_now_at(&dir, message)?);
    note_conflicts(&ws.name, &report);
    Ok(report)
}

/// Directory-level `sync_now`.
pub fn sync_now_at(dir: &Path, message: &str) -> Result<SyncReport> {
    require_repo(dir)?;
    let mut report = commit_at(dir, message)?;
    if !report.conflicts.is_empty() {
        return Ok(report);
    }
    report.absorb(pull_at(dir)?);
    if !report.conflicts.is_empty() {
        return Ok(report);
    }
    report.absorb(push_at(dir)?);
    Ok(report)
}

/// A conflict is an event a person has to see, so it lands in the journal of
/// the project whose file conflicted (`projects/<slug>/...`). Paths outside a
/// project have no journal to go to and are only in the report.
///
/// The same conflict is reported on every sync until somebody resolves it, so
/// the note is written only when it is not already the project's last word —
/// the Activity tab says it once, not once a minute.
fn note_conflicts(workspace: &str, report: &SyncReport) {
    for path in &report.conflicts {
        let Some(slug) = project_slug(path) else {
            continue;
        };
        let text = format!("sync conflict in {path} — resolve it, nothing was committed");
        if already_noted(workspace, &slug, &text) {
            continue;
        }
        let line = crate::journal::JournalLine::now("sync", crate::journal::Kind::Note, None, text);
        if let Err(e) = crate::journal::append(workspace, &slug, &line) {
            tracing::warn!("workspace sync: cannot journal conflict in {path}: {e}");
        }
    }
}

/// True when this exact note is already the newest line of the project's
/// journal — anything else that happened since makes it news again.
fn already_noted(workspace: &str, slug: &str, text: &str) -> bool {
    crate::journal::read(workspace, slug)
        .last()
        .is_some_and(|l| l.kind == crate::journal::Kind::Note && l.text == text)
}

/// `projects/<slug>/PLAN.md` -> `slug`.
fn project_slug(repo_relative: &str) -> Option<String> {
    let mut parts = repo_relative.split('/');
    if parts.next()? != "projects" {
        return None;
    }
    parts.next().filter(|s| !s.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_comes_from_the_project_path_only() {
        assert_eq!(
            project_slug("projects/checkout-flow/PLAN.md").as_deref(),
            Some("checkout-flow")
        );
        assert_eq!(project_slug("workspace.toml"), None);
    }
}
