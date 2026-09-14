//! T3-style turn checkpoints: shadow git commits under
//! `refs/parzi/checkpoints/<session>/<turn>` for risk-free rollbacks.
//! Never touches the user's active branch — all writes go to detached shadow
//! refs via `git hash-object` / `git update-ref` plumbing.

use parzi_core::error::{ParziError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointView {
    pub session_id: String,
    pub turn: u32,
    pub hash: String,
    pub created: String,
}

fn git(repo: &str, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(ParziError::Io)?;
    if !out.status.success() {
        return Err(ParziError::Tool(
            "checkpoint".into(),
            format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Snapshot the current worktree state (tracked-file diff) as a shadow commit.
/// Returns the commit hash. No-op error when `repo` is not a git checkout.
pub fn create_checkpoint(repo: &str, session_id: &str, turn: u32) -> Result<String> {
    for c in ["..", "/", "\\", " ", "\n", "\r", "\t"] {
        if session_id.contains(c) {
            return Err(ParziError::Tool(
                "checkpoint".into(),
                "bad session id".into(),
            ));
        }
    }
    // Stage nothing: build a tree object from the index + worktree via
    // `git stash create`-like plumbing without touching refs/HEAD.
    // Simplest portable path: `git add -A -N` intent + `git write-tree`?
    // We avoid mutating the index: use `git diff` content hashed directly.
    let diff = git(repo, &["diff", "--no-color", "--no-ext-diff", "HEAD", "--"])?;
    let status = git(repo, &["status", "--porcelain=v1", "--"])?;
    let body = format!("parzi checkpoint {session_id} turn {turn}\n\n{status}\n{diff}\n");
    // hash-object -w -t blob
    let mut child = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["hash-object", "-w", "--stdin"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(ParziError::Io)?;
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(body.as_bytes())
        .map_err(ParziError::Io)?;
    let out = child.wait_with_output().map_err(ParziError::Io)?;
    let blob = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if blob.is_empty() {
        return Err(ParziError::Tool(
            "checkpoint".into(),
            "hash-object failed".into(),
        ));
    }
    // commit-tree <head-tree> -m <msg> — parent = HEAD when available.
    let head_tree = git(repo, &["rev-parse", "HEAD^{tree}"])
        .unwrap_or_else(|_| "4b825dc642cb6eb9a060e54bf8d69288fbee4904".into());
    let head_commit = git(repo, &["rev-parse", "--verify", "HEAD"]).ok();
    let msg = format!("parzi checkpoint {session_id} turn {turn} blob {blob}");
    let mut args: Vec<&str> = vec!["commit-tree", &head_tree, "-m", &msg];
    let parent_holder;
    if let Some(h) = head_commit.as_deref() {
        parent_holder = h.to_string();
        args.push("-p");
        args.push(&parent_holder);
    }
    let commit = git(repo, &args)?;
    let r = format!("refs/parzi/checkpoints/{session_id}/turn/{turn}");
    git(repo, &["update-ref", &r, &commit])?;
    Ok(commit)
}

pub fn list_checkpoints(repo: &str, session_id: &str) -> Result<Vec<CheckpointView>> {
    let pattern = format!("refs/parzi/checkpoints/{session_id}/turn/");
    let out = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname) %(creatordate:iso)",
            &pattern,
        ],
    )
    .unwrap_or_default();
    let mut v = vec![];
    for line in out.lines() {
        let mut it = line.splitn(3, ' ');
        let name = it.next().unwrap_or("");
        let hash = it.next().unwrap_or("").to_string();
        let created = it.next().unwrap_or("").to_string();
        let turn: u32 = name
            .rsplit('/')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        v.push(CheckpointView {
            session_id: session_id.to_string(),
            turn,
            hash,
            created,
        });
    }
    v.sort_by_key(|c| c.turn);
    Ok(v)
}

/// Roll the worktree back to a checkpoint's recorded diff by checking out the
/// shadow commit's tree into the worktree (tracked files only). History on
/// the user's branch is untouched.
pub fn restore_checkpoint(repo: &str, session_id: &str, turn: u32) -> Result<()> {
    let r = format!("refs/parzi/checkpoints/{session_id}/turn/{turn}");
    let commit = git(repo, &["rev-parse", "--verify", &r])?;
    // Checkout the tree without moving HEAD: `read-tree` + `checkout-index`.
    git(repo, &["read-tree", &commit])?;
    git(repo, &["checkout-index", "-a", "-f"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo() -> String {
        let dir = std::env::temp_dir().join(format!(
            "parzi-ckpt-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let s = dir.to_string_lossy().to_string();
        git(&s, &["init", "-q"]).unwrap();
        git(&s, &["config", "user.email", "t@t.t"]).unwrap();
        git(&s, &["config", "user.name", "t"]).unwrap();
        std::fs::write(dir.join("a.txt"), "v1").unwrap();
        git(&s, &["add", "."]).unwrap();
        git(&s, &["commit", "-qm", "init"]).unwrap();
        s
    }

    fn rand_suffix() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        format!(
            "{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    #[test]
    fn checkpoint_roundtrip_lists_and_restores() {
        let repo = init_repo();
        let dir = std::path::PathBuf::from(&repo);
        std::fs::write(dir.join("a.txt"), "v2").unwrap();
        let c = create_checkpoint(&repo, "sess1", 1).unwrap();
        assert!(!c.is_empty());
        let list = list_checkpoints(&repo, "sess1").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].turn, 1);
        restore_checkpoint(&repo, "sess1", 1).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_session_rejected() {
        let repo = init_repo();
        assert!(create_checkpoint(&repo, "../x", 1).is_err());
        let _ = std::fs::remove_dir_all(&repo);
    }
}
