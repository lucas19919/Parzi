//! The git plumbing under the workspace syncer: spawning `git`, reading its
//! exit codes, and answering the two questions the policy in `mod.rs` asks —
//! "is anything unmerged?" and "does any file still carry conflict markers?".
//!
//! Nothing here decides anything. No libgit2 (the repo shells out to the git
//! CLI everywhere, like `parzi-runtime::git_worktree`), and no credential
//! handling of our own: `GIT_TERMINAL_PROMPT=0` makes a missing credential
//! fail fast instead of hanging a background timer on a prompt nobody sees.

use std::path::Path;

use crate::error::{ParziError, Result};

/// Files bigger than this are never scanned for conflict markers.
const MARKER_SCAN_LIMIT: u64 = 1 << 20;

pub(super) fn err(msg: impl Into<String>) -> ParziError {
    ParziError::Tool("workspace.sync".into(), msg.into())
}

pub(super) struct GitOut {
    pub(super) ok: bool,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

/// Run git in `dir`. `Err` only when git itself could not be started; a
/// non-zero exit is data the caller decides about.
pub(super) fn run_git(dir: &Path, args: &[&str]) -> Result<GitOut> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(ParziError::Io)?;
    Ok(GitOut {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).trim_end().to_string(),
    })
}

/// Run git in `dir` and turn a non-zero exit into an error naming the command.
pub(super) fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = run_git(dir, args)?;
    if !out.ok {
        return Err(err(format!(
            "git {} failed: {}",
            args.join(" "),
            if out.stderr.is_empty() {
                out.stdout.as_str()
            } else {
                out.stderr.as_str()
            }
        )));
    }
    Ok(out.stdout)
}

/// True when `dir` is inside a git repository.
pub fn is_repo(dir: &Path) -> bool {
    run_git(dir, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|o| o.ok)
}

/// The `origin` URL, when one is configured.
pub fn remote_url(dir: &Path) -> Option<String> {
    run_git(dir, &["remote", "get-url", "origin"])
        .ok()
        .filter(|o| o.ok && !o.stdout.is_empty())
        .map(|o| o.stdout)
}

pub(super) fn require_repo(dir: &Path) -> Result<()> {
    if !is_repo(dir) {
        return Err(err(format!(
            "{} is not a git repository — create the workspace first",
            dir.display()
        )));
    }
    Ok(())
}

/// The checked-out branch. `symbolic-ref` and not `rev-parse` on purpose: it
/// still answers on an unborn branch (a repo whose first commit is the one we
/// are about to make), and it is empty exactly when HEAD is detached.
pub(super) fn current_branch(dir: &Path) -> Result<String> {
    let out = run_git(dir, &["symbolic-ref", "--short", "--quiet", "HEAD"])?;
    if !out.ok || out.stdout.is_empty() {
        return Err(err("workspace repo is on a detached HEAD"));
    }
    Ok(out.stdout)
}

/// The machine may have no global git identity (a fresh laptop). Set a local
/// one rather than let the first commit fail.
pub(super) fn ensure_identity(dir: &Path) -> Result<()> {
    for (key, value) in [("user.name", "Parzi"), ("user.email", "parzi@localhost")] {
        let set = run_git(dir, &["config", "--get", key])?;
        if !set.ok || set.stdout.is_empty() {
            git(dir, &["config", key, value])?;
        }
    }
    Ok(())
}

/// Paths git itself calls unmerged.
pub(super) fn unmerged_paths(dir: &Path) -> Result<Vec<String>> {
    let out = run_git(dir, &["diff", "--name-only", "--diff-filter=U"])?;
    if !out.ok {
        return Ok(Vec::new());
    }
    Ok(out
        .stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// Changed files that still carry conflict markers — a merge somebody
/// resolved half-way, or markers a model wrote into a plan.
pub(super) fn marker_paths(dir: &Path) -> Result<Vec<String>> {
    let out = run_git(dir, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    if !out.ok {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    for line in out.stdout.lines() {
        let Some(rel) = status_path(line) else {
            continue;
        };
        let path = dir.join(&rel);
        let scannable =
            std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MARKER_SCAN_LIMIT);
        if !scannable {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if has_markers(&text) && !hits.contains(&rel) {
            hits.push(rel);
        }
    }
    Ok(hits)
}

/// `XY path`, `XY old -> new`, or `XY "quoted path"`.
fn status_path(line: &str) -> Option<String> {
    if line.len() < 4 {
        return None;
    }
    let rest = line[3..].trim();
    let path = rest.rsplit(" -> ").next().unwrap_or(rest);
    let path = path.trim_matches('"');
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

fn has_markers(text: &str) -> bool {
    let mut ours = false;
    let mut theirs = false;
    for line in text.lines() {
        ours |= line.starts_with("<<<<<<< ");
        theirs |= line.starts_with(">>>>>>> ");
    }
    ours && theirs
}

pub(super) fn merge_in_progress(dir: &Path) -> bool {
    run_git(dir, &["rev-parse", "--verify", "--quiet", "MERGE_HEAD"]).is_ok_and(|o| o.ok)
}

/// exit 1 = there are staged differences, exit 0 = none.
pub(super) fn has_staged_changes(dir: &Path) -> Result<bool> {
    // exit 1 = there are staged differences, exit 0 = none.
    Ok(!run_git(dir, &["diff", "--cached", "--quiet"])?.ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_need_both_sides() {
        assert!(has_markers(
            "a\n<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n"
        ));
        assert!(!has_markers("a\n<<<<<<< ours\nx\n"));
        assert!(!has_markers("plain text\n"));
    }

    #[test]
    fn status_paths_survive_renames_and_quotes() {
        assert_eq!(status_path(" M PLAN.md").as_deref(), Some("PLAN.md"));
        assert_eq!(
            status_path("R  old.md -> projects/a/PLAN.md").as_deref(),
            Some("projects/a/PLAN.md")
        );
        assert_eq!(
            status_path("?? \"projects/a b/PROJECT.md\"").as_deref(),
            Some("projects/a b/PROJECT.md")
        );
        assert_eq!(status_path("").as_deref(), None);
    }
}
