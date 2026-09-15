//! Git worktree isolation: executes tasks in isolated, detached worktrees
//! (`~/.parzi/worktrees/<project>/<lane>/<session>`) so agents do not
//! dirty the user's active editor workspace.

use std::path::{Path, PathBuf};

use parzi_core::error::{ParziError, Result};
use parzi_core::paths;

fn git(cwd: &str, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(ParziError::Io)?;
    if !out.status.success() {
        return Err(ParziError::Tool(
            "worktree".into(),
            format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Checks if a directory is a valid git repository.
pub fn is_git_repo(path: &str) -> bool {
    git(path, &["rev-parse", "--is-inside-work-tree"]).is_ok()
}

/// Branch name generated for a session's isolated worktree.
pub fn session_branch(lane: &str, session_id: &str) -> String {
    let clean_lane = if lane.trim().is_empty() {
        "default"
    } else {
        lane.trim()
    };
    format!("parzi/{clean_lane}/{session_id}")
}

/// Create an isolated worktree for a session off `HEAD`.
/// Returns the absolute path to the created worktree directory.
pub fn create_worktree(repo: &str, project: &str, lane: &str, session_id: &str) -> Result<PathBuf> {
    for c in ["..", "/", "\\", " ", "\n", "\r", "\t"] {
        if session_id.contains(c) {
            return Err(ParziError::Tool("worktree".into(), "bad session id".into()));
        }
    }
    let base = paths::worktrees_dir()?;
    let clean_lane = if lane.trim().is_empty() {
        "default"
    } else {
        lane.trim()
    };
    let dest = base.join(project).join(clean_lane).join(session_id);

    if dest.exists() {
        let _ = git(
            repo,
            &["worktree", "remove", "--force", &dest.to_string_lossy()],
        );
        let _ = std::fs::remove_dir_all(&dest);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(ParziError::Io)?;
    }

    let branch = session_branch(lane, session_id);
    // Delete existing branch if left over from a previous killed run
    let _ = git(repo, &["branch", "-D", &branch]);

    let dest_str = dest.to_string_lossy();
    git(repo, &["worktree", "add", "-b", &branch, &dest_str, "HEAD"])?;
    Ok(dest)
}

/// Generate consolidated diff of changes made in the worktree against HEAD.
pub fn worktree_diff(worktree_path: &Path) -> Result<String> {
    let wt = worktree_path.to_string_lossy();
    let diff = git(&wt, &["diff", "--no-color", "HEAD"])?;
    let untracked = git(&wt, &["status", "--porcelain=v1"])?;
    let mut out = String::new();
    if !diff.is_empty() {
        out.push_str(&diff);
        out.push('\n');
    }
    if !untracked.is_empty() {
        out.push_str("\n# Untracked/modified files:\n");
        out.push_str(&untracked);
        out.push('\n');
    }
    Ok(out)
}

/// Merge or apply the session branch back into the main repo working directory.
pub fn apply_worktree(repo: &str, lane: &str, session_id: &str) -> Result<String> {
    let branch = session_branch(lane, session_id);
    // Squash merge so changes arrive as staged edits ready for commit/review
    git(repo, &["merge", "--no-commit", "--squash", &branch])
}

/// Remove a worktree and optionally delete its branch.
pub fn remove_worktree(
    repo: &str,
    worktree_path: &Path,
    delete_branch: bool,
    lane: &str,
    session_id: &str,
) -> Result<()> {
    let wt = worktree_path.to_string_lossy();
    let _ = git(repo, &["worktree", "remove", "--force", &wt]);
    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(worktree_path);
    }
    let _ = git(repo, &["worktree", "prune"]);
    if delete_branch {
        let branch = session_branch(lane, session_id);
        let _ = git(repo, &["branch", "-D", &branch]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_git_repo(dir: &Path) {
        git(&dir.to_string_lossy(), &["init"]).unwrap();
        git(&dir.to_string_lossy(), &["config", "user.name", "Test"]).unwrap();
        git(
            &dir.to_string_lossy(),
            &["config", "user.email", "test@test.com"],
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), "# Test\n").unwrap();
        git(&dir.to_string_lossy(), &["add", "README.md"]).unwrap();
        git(&dir.to_string_lossy(), &["commit", "-m", "initial"]).unwrap();
    }

    #[test]
    fn worktree_create_diff_and_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        init_git_repo(&repo);

        let session_id = "test-session-123";
        let wt = create_worktree(&repo.to_string_lossy(), "proj", "core", session_id).unwrap();
        assert!(wt.exists());
        assert!(wt.join("README.md").exists());

        // Modify file inside worktree
        std::fs::write(wt.join("README.md"), "# Test modified\n").unwrap();
        let diff = worktree_diff(&wt).unwrap();
        assert!(diff.contains("-# Test"));
        assert!(diff.contains("+# Test modified"));

        // Remove worktree
        remove_worktree(&repo.to_string_lossy(), &wt, true, "core", session_id).unwrap();
        assert!(!wt.exists());
    }
}
