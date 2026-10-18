//! A session's own copy of a repository: a git worktree on a new branch,
//! made from a branch the person picks, so the session never touches their
//! checkout or another session's files. Worktrees live under
//! `~/.parzi/worktrees/<repo>/`, and go when the last session in one does
//! (only when nothing in it is left uncommitted).

use std::path::{Path, PathBuf};
use std::process::Command;

/// A folder's local branches, and the one it is on.
#[derive(Debug, Default, serde::Serialize)]
pub struct Branches {
    pub current: String,
    pub all: Vec<String>,
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("git did not run: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The local branches of the repository at `dir`, most recently used first.
/// Not a repository: none.
pub fn branches(dir: &Path) -> Branches {
    let current = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let all = git(
        dir,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/heads",
        ],
    )
    .map(|out| {
        out.lines()
            .map(str::to_string)
            .filter(|b| !b.is_empty())
            .collect()
    })
    .unwrap_or_default();
    Branches { current, all }
}

/// Where Parzi keeps its worktrees.
pub fn root() -> Result<PathBuf, String> {
    Ok(parzi_core::paths::parzi_dir()
        .map_err(|e| e.to_string())?
        .join("worktrees"))
}

/// A short name from the first words of `text`: lowercase, dashes.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for word in text.split_whitespace().take(5) {
        let w: String = word
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if w.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(&w);
        if out.len() >= 30 {
            break;
        }
    }
    if out.is_empty() {
        "session".into()
    } else {
        out.chars()
            .take(30)
            .collect::<String>()
            .trim_end_matches('-')
            .to_string()
    }
}

/// A new worktree of the repository at `repo`, on a new branch
/// `parzi/<name>` made from `base`; its folder.
pub fn create(repo: &Path, base: &str, about: &str) -> Result<PathBuf, String> {
    create_in(&root()?, repo, base, about)
}

fn create_in(root: &Path, repo: &Path, base: &str, about: &str) -> Result<PathBuf, String> {
    let top = PathBuf::from(
        git(repo, &["rev-parse", "--show-toplevel"])
            .map_err(|_| format!("{} is not a git repository", repo.display()))?,
    );
    let repo_name = top
        .file_name()
        .map_or_else(|| "repo".into(), |n| n.to_string_lossy().to_string());
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..4];
    let name = format!("{}-{tag}", slug(about));
    let branch = format!("parzi/{name}");
    let dir = root.join(&repo_name).join(&name);
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let dir_s = dir.display().to_string();
    git(&top, &["worktree", "add", "-b", &branch, &dir_s, base])
        .map_err(|e| format!("could not make a worktree from {base}: {e}"))?;
    Ok(dir)
}

/// Removes the worktree at `dir` when it is one of Parzi's and nothing in
/// it is uncommitted; its branch stays. Says what happened, if anything.
pub fn remove(dir: &Path) -> Option<String> {
    remove_in(&root().ok()?, dir)
}

fn remove_in(root: &Path, dir: &Path) -> Option<String> {
    let inside = dir
        .canonicalize()
        .ok()
        .zip(root.canonicalize().ok())
        .is_some_and(|(d, r)| d.starts_with(r));
    if !inside {
        return None;
    }
    let dirty = git(dir, &["status", "--porcelain"]).map_or(true, |s| !s.is_empty());
    if dirty {
        return Some(format!(
            "kept {}: it has uncommitted changes",
            dir.display()
        ));
    }
    let common = git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()?;
    let main = Path::new(&common).parent()?.to_path_buf();
    let dir_s = dir.display().to_string();
    // A process just stopped may still hold the folder a moment.
    let mut tried = git(&main, &["worktree", "remove", &dir_s]);
    if tried.is_err() && dir.exists() {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        tried = git(&main, &["worktree", "remove", "--force", &dir_s]).or_else(|e| {
            std::fs::remove_dir(dir)
                .map(|()| String::new())
                .map_err(|_| e)
        });
        let _ = git(&main, &["worktree", "prune"]);
    }
    match tried {
        Ok(_) => None,
        Err(e) => Some(format!("could not remove {}: {e}", dir.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_short_lowercase_words() {
        assert_eq!(
            slug("Fix the Composer freeze, please!"),
            "fix-the-composer-freeze-please"
        );
        assert_eq!(slug("   "), "session");
        assert!(slug(&"word ".repeat(20)).len() <= 30);
    }

    #[test]
    fn a_worktree_comes_and_goes_and_keeps_your_checkout() {
        let base = std::env::temp_dir().join(format!("parzi-wt-{}", uuid::Uuid::new_v4().simple()));
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let root = base.join("worktrees");
        std::fs::create_dir_all(&root).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec![
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "one",
            ],
        ] {
            git(&repo, &args).unwrap();
        }
        let b = branches(&repo);
        assert_eq!(
            (b.current.as_str(), b.all.clone()),
            ("main", vec!["main".to_string()])
        );
        let wt = create_in(&root, &repo, "main", "Try the slate accent").unwrap();
        assert!(wt.join(".git").exists(), "a checkout of its own");
        assert!(branches(&wt)
            .current
            .starts_with("parzi/try-the-slate-accent-"));
        assert_eq!(
            branches(&repo).current,
            "main",
            "your checkout is untouched"
        );
        assert_eq!(remove_in(&root, &wt), None);
        assert!(!wt.exists(), "gone with its last session");
        assert_eq!(remove_in(&root, &repo), None, "never a folder of yours");
        assert!(repo.exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
