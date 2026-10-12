//! What a run changed, as git sees it: a snapshot of `git status` and HEAD
//! when the run starts, diffed when it ends. Files already dirty at the
//! start count only when the run changed them again. While another run
//! works in the same repo the two cannot be told apart, so each keeps only
//! the files its own tool calls named.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Git gets this long per snapshot; a bigger repo just goes without.
const BUDGET: Duration = Duration::from_secs(3);
const MAX_ENTRIES: usize = 5_000;
/// Files up to this size are hashed; bigger ones go by size and time.
const HASH_UP_TO: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Print {
    len: u64,
    modified: Option<std::time::SystemTime>,
    hash: Option<u64>,
}

struct Live {
    id: u64,
    root: String,
    overlapped: Arc<AtomicBool>,
}

static LIVE: Mutex<Vec<Live>> = Mutex::new(Vec::new());

/// A run's place in the registry of runs per repo; leaves it when dropped.
struct Slot {
    id: u64,
    overlapped: Arc<AtomicBool>,
}

impl Slot {
    fn join(root: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let overlapped = Arc::new(AtomicBool::new(false));
        let mut live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
        for other in live.iter().filter(|l| l.root == root) {
            other.overlapped.store(true, Ordering::Relaxed);
            overlapped.store(true, Ordering::Relaxed);
        }
        live.push(Live {
            id,
            root: root.to_string(),
            overlapped: overlapped.clone(),
        });
        Self { id, overlapped }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        LIVE.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|l| l.id != self.id);
    }
}

/// The repo as a run found it.
pub(crate) struct GitMark {
    root: PathBuf,
    /// The run's folder inside the repo (`ui/src/`), "" at the top.
    prefix: String,
    head: Option<String>,
    /// Dirty paths at the start; None for one that did not exist then.
    dirty: HashMap<String, Option<Print>>,
    slot: Slot,
}

async fn git(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
    let out = tokio::time::timeout(BUDGET, cmd.output())
        .await
        .ok()?
        .ok()?;
    out.status.success().then_some(out.stdout)
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// Paths `git status --porcelain -z` lists as changed or new.
fn parse_status(raw: &[u8]) -> Vec<String> {
    let mut out = vec![];
    let mut parts = raw.split(|b| *b == 0);
    while let Some(rec) = parts.next() {
        if rec.len() < 4 {
            continue;
        }
        let (code, path) = (&rec[..2], &rec[3..]);
        out.push(String::from_utf8_lossy(path).into_owned());
        // A rename or copy is followed by where it came from.
        if code.contains(&b'R') || code.contains(&b'C') {
            parts.next();
        }
    }
    out
}

fn print(path: &Path) -> Option<Print> {
    let meta = std::fs::metadata(path).ok()?;
    let hash = (meta.is_file() && meta.len() <= HASH_UP_TO)
        .then(|| std::fs::read(path).ok())
        .flatten()
        .map(|b| {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for x in b {
                h ^= u64::from(x);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            h
        });
    Some(Print {
        len: meta.len(),
        modified: meta.modified().ok(),
        hash,
    })
}

async fn prints(root: PathBuf, paths: Vec<String>) -> HashMap<String, Option<Print>> {
    tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|p| {
                let at = root.join(&p);
                (p, print(&at))
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

async fn status(root: &Path) -> Option<Vec<String>> {
    let raw = git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    let mut paths = parse_status(&raw);
    paths.truncate(MAX_ENTRIES);
    Some(paths)
}

impl GitMark {
    /// The snapshot for a run in `cwd`; None outside a repo, without git,
    /// or when git takes longer than its budget.
    pub(crate) async fn take(cwd: &Path) -> Option<Self> {
        if cwd.as_os_str().is_empty() || !cwd.is_dir() {
            return None;
        }
        let root = PathBuf::from(text(git(cwd, &["rev-parse", "--show-toplevel"]).await?));
        let prefix = text(
            git(cwd, &["rev-parse", "--show-prefix"])
                .await
                .unwrap_or_default(),
        );
        let head = git(&root, &["rev-parse", "--verify", "-q", "HEAD"])
            .await
            .map(text)
            .filter(|h| !h.is_empty());
        let paths = status(&root).await?;
        let dirty = prints(root.clone(), paths).await;
        let key = root.to_string_lossy().replace('\\', "/").to_lowercase();
        Some(Self {
            slot: Slot::join(&key),
            root,
            prefix,
            head,
            dirty,
        })
    }

    /// Whether another run worked in the same repo meanwhile.
    pub(crate) fn overlapped(&self) -> bool {
        self.slot.overlapped.load(Ordering::Relaxed)
    }

    /// Files the run changed, relative to its folder: what it committed and
    /// what is dirty now that was not (or was different) at the start. With
    /// an overlapping run, only those `named` by the run's own tool calls.
    pub(crate) async fn finish(self, named: &[String]) -> Vec<String> {
        let mut changed: BTreeSet<String> = BTreeSet::new();
        let head = git(&self.root, &["rev-parse", "--verify", "-q", "HEAD"])
            .await
            .map(text)
            .filter(|h| !h.is_empty());
        if let (Some(old), Some(new)) = (&self.head, &head) {
            if old != new {
                if let Some(raw) = git(
                    &self.root,
                    &["diff", "--name-only", "-z", &format!("{old}..{new}")],
                )
                .await
                {
                    changed.extend(
                        raw.split(|b| *b == 0)
                            .filter(|p| !p.is_empty())
                            .map(|p| String::from_utf8_lossy(p).into_owned()),
                    );
                }
            }
        }
        if let Some(now) = status(&self.root).await {
            let fresh = prints(self.root.clone(), now).await;
            for (p, print) in fresh {
                if self.dirty.get(&p) != Some(&print) {
                    changed.insert(p);
                }
            }
        }
        let overlapped = self.overlapped();
        changed
            .into_iter()
            .map(|p| from_root(&self.prefix, &p))
            .filter(|p| !overlapped || named.iter().any(|n| same_path(n, p)))
            .collect()
    }
}

/// A repo-root path as seen from the run's folder (`prefix` inside it).
fn from_root(prefix: &str, path: &str) -> String {
    if let Some(rest) = path.strip_prefix(prefix) {
        return rest.to_string();
    }
    let depth = prefix.split('/').filter(|s| !s.is_empty()).count();
    format!("{}{path}", "../".repeat(depth))
}

/// Paths compare with forward slashes, and without case on Windows.
pub(crate) fn same_path(a: &str, b: &str) -> bool {
    let n = |s: &str| {
        let s = s.replace('\\', "/");
        let s = s.trim_start_matches("./").to_string();
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    n(a) == n(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        run(d.path(), &["init", "-q"]);
        run(d.path(), &["config", "user.email", "t@example.com"]);
        run(d.path(), &["config", "user.name", "t"]);
        run(d.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(d.path().join("kept.txt"), "one").unwrap();
        std::fs::write(d.path().join("dirty.txt"), "one").unwrap();
        run(d.path(), &["add", "."]);
        run(d.path(), &["commit", "-q", "-m", "start"]);
        d
    }

    #[test]
    fn status_lists_new_changed_and_renamed_files() {
        let raw = b" M a.rs\0?? new dir/b.rs\0R  to.rs\0from.rs\0";
        assert_eq!(parse_status(raw), ["a.rs", "new dir/b.rs", "to.rs"]);
        assert_eq!(from_root("ui/", "ui/src/a.ts"), "src/a.ts");
        assert_eq!(from_root("ui/src/", "README.md"), "../../README.md");
        assert!(same_path("./src\\a.rs", "src/a.rs"));
    }

    #[tokio::test]
    async fn new_and_changed_files_count_and_old_dirt_does_not() {
        let d = repo();
        std::fs::write(d.path().join("dirty.txt"), "dirty before").unwrap();
        std::fs::write(d.path().join("loose.txt"), "untracked before").unwrap();
        let mark = GitMark::take(d.path()).await.expect("a repo");
        std::fs::write(d.path().join("kept.txt"), "two").unwrap();
        std::fs::create_dir_all(d.path().join("src")).unwrap();
        std::fs::write(d.path().join("src").join("new.rs"), "fn x() {}").unwrap();
        let files = mark.finish(&[]).await;
        assert_eq!(
            files,
            ["kept.txt", "src/new.rs"],
            "the run's own changes only"
        );

        let mark = GitMark::take(d.path()).await.unwrap();
        std::fs::write(d.path().join("dirty.txt"), "dirty again").unwrap();
        assert_eq!(
            mark.finish(&[]).await,
            ["dirty.txt"],
            "changed again counts"
        );
    }

    #[tokio::test]
    async fn a_commit_during_the_run_still_counts() {
        let d = repo();
        let mark = GitMark::take(d.path()).await.unwrap();
        std::fs::write(d.path().join("kept.txt"), "committed").unwrap();
        run(d.path(), &["commit", "-q", "-am", "mid-run"]);
        assert_eq!(mark.finish(&[]).await, ["kept.txt"]);
    }

    #[tokio::test]
    async fn a_folder_outside_git_has_no_mark() {
        let d = tempfile::tempdir().unwrap();
        // Inside a temp folder that a parent repo might hold, ask git first.
        let inside = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(d.path())
            .output()
            .is_ok_and(|o| o.status.success());
        if !inside {
            assert!(GitMark::take(d.path()).await.is_none());
        }
        assert!(GitMark::take(Path::new("")).await.is_none());
    }

    #[tokio::test]
    async fn overlapping_runs_keep_only_what_they_named() {
        let d = repo();
        let a = GitMark::take(d.path()).await.unwrap();
        let b = GitMark::take(d.path()).await.unwrap();
        assert!(a.overlapped() && b.overlapped());
        std::fs::write(d.path().join("kept.txt"), "a wrote this").unwrap();
        std::fs::write(d.path().join("b.txt"), "b wrote this").unwrap();
        assert_eq!(a.finish(&["kept.txt".into()]).await, ["kept.txt"]);
        assert_eq!(b.finish(&["./b.txt".into()]).await, ["b.txt"]);
        let alone = GitMark::take(d.path()).await.unwrap();
        assert!(!alone.overlapped(), "finished runs leave the registry");
    }
}
