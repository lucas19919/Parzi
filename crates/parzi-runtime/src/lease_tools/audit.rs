//! What a command changed (PLAN §4). `shell.exec` cannot say in advance which
//! files it will write, so the lease gate looks afterwards: the worktree is
//! fingerprinted before and after the command, and every path that moved is
//! checked against the lease table exactly as an `fs.write` would have been.
//!
//! `git status --porcelain` is the fingerprint wherever the worktree is a git
//! checkout — which is what a dispatched lane gets (§4, "where lanes run") —
//! and a repo-less directory falls back to a bounded walk of mtimes and sizes,
//! so a lane working outside git is not silently ungated.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

/// Give up on a non-git worktree past this many files: the walk would then
/// cost more than the audit is worth, and a half-taken snapshot would report
/// changes that never happened.
const WALK_LIMIT: usize = 20_000;

/// `git status` on a huge tree is still fast, but a hung git must never hang
/// a lane's tool call.
const GIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Worktree-relative path (always `/`-separated) → fingerprint.
pub(crate) type Snapshot = BTreeMap<String, String>;

/// Fingerprint the worktree. `None` means no honest snapshot could be taken —
/// the caller then skips the audit rather than inventing a violation.
pub(crate) async fn snapshot(cwd: &str) -> Option<Snapshot> {
    if cwd.trim().is_empty() {
        return None;
    }
    if let Some(snap) = git_status(cwd).await {
        return Some(snap);
    }
    let dir = cwd.to_string();
    tokio::task::spawn_blocking(move || walk(&dir)).await.ok()?
}

/// Every file in the worktree, worktree-relative: where another lane holds
/// a folder or a glob, its files are found here before `shell.exec` runs,
/// and the listing proves which files existed. `None` past the walk limit.
pub(crate) fn files(cwd: &str) -> Option<Vec<String>> {
    walk(cwd).map(|snap| snap.into_keys().collect())
}

/// Every path that appeared, vanished, or has a different fingerprint.
pub(crate) fn changed(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut out: Vec<String> = after
        .iter()
        .filter(|(path, stamp)| before.get(*path) != Some(*stamp))
        .map(|(path, _)| path.clone())
        .collect();
    out.extend(
        before
            .keys()
            .filter(|path| !after.contains_key(*path))
            .cloned(),
    );
    out.sort();
    out.dedup();
    out
}

/// The dirty set as git sees it, each entry stamped with the file's own
/// mtime and size — a file that was already modified and is modified again
/// keeps its status code, so the status alone would miss the second write.
async fn git_status(cwd: &str) -> Option<Snapshot> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["status", "--porcelain", "-z", "--untracked-files=all"])
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(GIT_TIMEOUT, cmd.output())
        .await
        .ok()?
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let root = Path::new(cwd);
    let mut snap = Snapshot::new();
    for entry in String::from_utf8_lossy(&out.stdout).split('\0') {
        let (code, path) = split_entry(entry);
        if path.is_empty() {
            continue;
        }
        snap.insert(
            path.to_string(),
            format!("{code}:{}", stamp(&root.join(path))),
        );
    }
    Some(snap)
}

/// `" M src/routes.rs"` → `(" M", "src/routes.rs")`. A rename's second `-z`
/// record is the old path with no status code of its own.
fn split_entry(entry: &str) -> (&str, &str) {
    match entry.len() > 3 && entry.as_bytes()[2] == b' ' {
        true => (&entry[..2], entry[3..].trim_start()),
        false => ("->", entry),
    }
}

/// Fallback for a worktree that is not a git checkout: every file under `cwd`
/// with its mtime and size. `.git` is never walked.
fn walk(cwd: &str) -> Option<Snapshot> {
    let root = Path::new(cwd);
    let mut snap = Snapshot::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            if e.file_name() == std::ffi::OsStr::new(".git") {
                continue;
            }
            let path = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(_) => {
                    if snap.len() >= WALK_LIMIT {
                        return None;
                    }
                    if let Some(rel) = relative(root, &path) {
                        snap.insert(rel, stamp(&path));
                    }
                }
                Err(_) => continue,
            }
        }
    }
    Some(snap)
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    Some(
        path.strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/"),
    )
}

/// mtime + size. A file that cannot be read is stamped `gone`, which differs
/// from any real stamp, so its disappearance still counts as a change.
fn stamp(path: &Path) -> String {
    let Ok(meta) = std::fs::metadata(path) else {
        return "gone".to_string();
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{mtime}:{}", meta.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(pairs: &[(&str, &str)]) -> Snapshot {
        pairs
            .iter()
            .map(|(p, s)| ((*p).to_string(), (*s).to_string()))
            .collect()
    }

    #[test]
    fn a_new_a_touched_and_a_deleted_file_all_count_as_changed() {
        let before = snap(&[("src/a.rs", "1:10"), ("src/b.rs", "1:10")]);
        let after = snap(&[("src/a.rs", "2:12"), ("src/c.rs", "3:1")]);
        assert_eq!(
            changed(&before, &after),
            vec![
                "src/a.rs".to_string(),
                "src/b.rs".to_string(),
                "src/c.rs".to_string()
            ]
        );
        assert!(changed(&before, &before).is_empty(), "a quiet command");
    }

    #[test]
    fn a_rewritten_file_with_the_same_status_still_shows_up() {
        // Both snapshots say "modified"; only the stamp tells them apart.
        let before = snap(&[("src/a.rs", " M:100:10")]);
        let after = snap(&[("src/a.rs", " M:200:10")]);
        assert_eq!(changed(&before, &after), vec!["src/a.rs".to_string()]);
    }

    #[test]
    fn porcelain_entries_split_into_status_and_path() {
        assert_eq!(split_entry(" M src/routes.rs"), (" M", "src/routes.rs"));
        assert_eq!(split_entry("?? new.rs"), ("??", "new.rs"));
        // The old path of a rename arrives on its own, with no code.
        assert_eq!(split_entry("old.rs"), ("->", "old.rs"));
        assert_eq!(split_entry(""), ("->", ""));
    }
}
