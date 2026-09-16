//! The workspace digest the header agent is briefed with: repo names, each
//! repo's README head and a two-level tree. Deterministic and cached per
//! workspace mtime — the header is the cheap role and must stay cheap
//! (PLAN §15.7).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use parzi_core::workspace::{self, Workspace};

use super::ContextPart;

/// README lines shown per repo.
const README_LINES: usize = 60;
/// Tree depth, and the entry cap that keeps a 10k-file repo from blowing up
/// the header's prompt.
const TREE_DEPTH: usize = 2;
const TREE_CAP: usize = 200;

#[must_use]
pub fn workspace_digest(name: &str) -> Vec<ContextPart> {
    let Ok(ws) = workspace::load(name) else {
        return vec![ContextPart::new(
            &format!("workspace:{name}"),
            "(workspace not found on this machine)",
        )];
    };
    let stamp = digest_stamp(&ws);
    if let Ok(map) = cache().lock() {
        if let Some((cached, parts)) = map.get(&ws.name) {
            if *cached == stamp {
                return parts.clone();
            }
        }
    }
    let parts = build(&ws);
    if let Ok(mut map) = cache().lock() {
        map.insert(ws.name.clone(), (stamp, parts.clone()));
    }
    parts
}

type DigestCache = Mutex<HashMap<String, (u64, Vec<ContextPart>)>>;

fn cache() -> &'static DigestCache {
    static CACHE: OnceLock<DigestCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Newest mtime across `workspace.toml` and each mapped repo's root and
/// README. Cheap (a handful of stats) and enough: a repo that changed under us
/// has a newer root mtime, and a stale README head costs nobody a wrong answer.
fn digest_stamp(ws: &Workspace) -> u64 {
    let mut newest = mtime(&workspace::workspace_file(&ws.name));
    for repo in &ws.repos {
        if let Some(path) = &repo.local_path {
            newest = newest.max(mtime(path));
            newest = newest.max(mtime(&path.join("README.md")));
        }
    }
    newest
}

fn mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

fn build(ws: &Workspace) -> Vec<ContextPart> {
    let mut head = format!("kind: {:?}\nrepos:\n", ws.kind);
    for repo in &ws.repos {
        let mapped = repo.local_path.as_ref().map_or_else(
            || "not mapped on this machine".to_string(),
            |p| p.display().to_string(),
        );
        let remote = if repo.remote.is_empty() {
            "no remote"
        } else {
            repo.remote.as_str()
        };
        head.push_str(&format!("- {} ({remote}) — {mapped}\n", repo.name));
    }
    if !ws.members.is_empty() {
        head.push_str("members:\n");
        for m in &ws.members {
            head.push_str(&format!("- {} ({:?})\n", m.user, m.role));
        }
    }
    let mut parts = vec![ContextPart::new(&format!("workspace:{}", ws.name), head)];
    for repo in &ws.repos {
        let Some(root) = repo.local_path.as_ref().filter(|p| p.is_dir()) else {
            continue;
        };
        if let Some(readme) = read_head(&root.join("README.md"), README_LINES) {
            parts.push(ContextPart::new(
                &format!("repo:{}:readme", repo.name),
                readme,
            ));
        }
        parts.push(ContextPart::new(
            &format!("repo:{}:tree", repo.name),
            tree(root, TREE_DEPTH),
        ));
    }
    parts
}

/// First `n` lines of a file, if it is there and readable.
fn read_head(path: &Path, n: usize) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut out: String = raw.lines().take(n).collect::<Vec<_>>().join("\n");
    if raw.lines().count() > n {
        out.push_str("\n…");
    }
    Some(out)
}

/// Sorted two-level listing with the usual noise skipped.
fn tree(root: &Path, depth: usize) -> String {
    const SKIP: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        "dist",
        "build",
        ".venv",
        "__pycache__",
    ];
    let mut out: Vec<String> = vec![];
    let mut walk: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let mut capped = false;
    while let Some((dir, level)) = walk.pop() {
        if level >= depth || out.len() >= TREE_CAP {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if SKIP.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            if out.len() >= TREE_CAP {
                capped = true;
                break;
            }
            let path = e.path();
            let is_dir = path.is_dir();
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string()
                .replace('\\', "/");
            out.push(if is_dir { format!("{rel}/") } else { rel });
            if is_dir {
                walk.push((path, level + 1));
            }
        }
    }
    out.sort();
    if capped {
        out.push("…".to_string());
    }
    out.join("\n")
}
