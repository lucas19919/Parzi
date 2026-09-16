//! Micro-context for a coder lane (PLAN §5): the task and its acceptance
//! lines, the files in scope, the public signatures around them, the capsules
//! of the tasks it comes after, and the project's KNOWLEDGE.md. Nothing else —
//! no other session's transcript, ever.
//!
//! Every part is capped, because the point of a micro-context is that a coder
//! model reads it whole.

use std::path::{Path, PathBuf};

use parzi_core::capsule::HandoffCapsule;
use parzi_core::plan::{Plan, Task};
use parzi_core::project::{glob_match, normalize_path, Project};

use crate::lease_tools::task_str;

/// One titled block of context — the roles lane's type, because a coder run
/// is assembled from these and there must be exactly one of them.
pub use crate::roles::ContextPart;

/// Files listed from the worktree for one scope.
const MAX_SCOPE_FILES: usize = 200;
/// Signature lines across all scope files.
const MAX_SIGNATURE_LINES: usize = 200;
/// Lines kept per handoff capsule.
const MAX_CAPSULE_LINES: usize = 25;
/// Directory entries walked before giving up on a huge repo.
const MAX_WALK_ENTRIES: usize = 20_000;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "__pycache__",
];

/// The whole context a coder gets for one task.
pub fn micro_context(
    project: &Project,
    plan: &Plan,
    task: &Task,
    capsules: &[HandoffCapsule],
) -> Vec<ContextPart> {
    let mut parts = vec![ContextPart::new("task", task_block(project, plan, task))];
    let files = scope_files(project, task);
    if !files.is_empty() {
        let listed: Vec<String> = files
            .iter()
            .map(|(rel, _)| format!("- {rel}"))
            .take(MAX_SCOPE_FILES)
            .collect();
        parts.push(ContextPart::new(
            "scope",
            format!(
                "Files in scope ({} shown of {}):\n{}",
                listed.len(),
                files.len(),
                listed.join("\n")
            ),
        ));
        let sigs = signatures(&files);
        if !sigs.is_empty() {
            parts.push(ContextPart::new(
                "signatures",
                format!("Public surface around the scope:\n{}", sigs.join("\n")),
            ));
        }
    }
    for c in capsules.iter().filter(|c| task.after.contains(&c.task)) {
        parts.push(ContextPart::new(
            &format!("capsule:{}", task_str(&c.task)),
            render_capsule(c),
        ));
    }
    if let Some(k) = knowledge(project) {
        parts.push(ContextPart::new("knowledge", k));
    }
    parts
}

/// The task as the plan states it, plus its acceptance lines and the sprint
/// it belongs to — a coder should never have to guess what "done" means.
fn task_block(project: &Project, plan: &Plan, task: &Task) -> String {
    let mut s = format!(
        "Project {} ({}): {}\nTask {} — {}\nRepo: {}\nScope: {}\n",
        project.slug,
        project.workspace,
        project.title,
        task_str(&task.id),
        task.title,
        task.repo,
        task.scope.join(", ")
    );
    for sprint in &plan.sprints {
        if sprint
            .lanes
            .iter()
            .any(|l| l.tasks.iter().any(|t| t.id == task.id))
        {
            s.push_str(&format!(
                "Sprint: {} (target {})\n",
                sprint.title, sprint.target
            ));
            break;
        }
    }
    if !task.after.is_empty() {
        s.push_str(&format!(
            "After: {}\n",
            task.after
                .iter()
                .map(task_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if task.critical {
        s.push_str("Critical: file transfers here need a human.\n");
    }
    if !task.acceptance.is_empty() {
        s.push_str("Acceptance:\n");
        for a in &task.acceptance {
            s.push_str(&format!("- {a}\n"));
        }
    }
    if !project.constraints.is_empty() {
        s.push_str("Constraints:\n");
        for c in &project.constraints {
            s.push_str(&format!("- {c}\n"));
        }
    }
    s
}

/// `(repo-relative path, absolute path)` for everything the scope globs cover
/// in the task's repo worktree. An unmapped repo yields nothing rather than
/// guessing a path.
fn scope_files(project: &Project, task: &Task) -> Vec<(String, PathBuf)> {
    let Some(root) = repo_root(project, &task.repo) else {
        return vec![];
    };
    let mut out = vec![];
    let mut budget = MAX_WALK_ENTRIES;
    walk(&root, &root, &mut budget, &mut |rel, abs| {
        if task.scope.iter().any(|g| glob_match(g, rel)) {
            out.push((rel.to_string(), abs.to_path_buf()));
        }
        out.len() < MAX_SCOPE_FILES * 4
    });
    out.sort();
    out
}

/// Local worktree of `repo` from the workspace's repo map (PLAN §1.1: each
/// machine maps remote → local path for itself).
fn repo_root(project: &Project, repo: &str) -> Option<PathBuf> {
    let ws = parzi_core::workspace::load(&project.workspace).ok()?;
    ws.repos
        .iter()
        .find(|r| r.name == repo)
        .and_then(|r| r.local_path.clone())
        .filter(|p| p.is_dir())
}

/// Depth-first walk with a hard entry budget. `visit` returns false to stop.
fn walk(
    root: &Path,
    dir: &Path,
    budget: &mut usize,
    visit: &mut impl FnMut(&str, &Path) -> bool,
) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return true;
    };
    for entry in rd.flatten() {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            if !walk(root, &path, budget, visit) {
                return false;
            }
            continue;
        }
        let rel = normalize_path(&path.strip_prefix(root).unwrap_or(&path).to_string_lossy());
        if !visit(&rel, &path) {
            return false;
        }
    }
    true
}

/// First pass at "public surface": the declaration lines a neighbour needs to
/// call into this scope. Grep, not a parser — cheap, and wrong only by being
/// generous.
fn signatures(files: &[(String, PathBuf)]) -> Vec<String> {
    let mut out = vec![];
    for (rel, abs) in files {
        if out.len() >= MAX_SIGNATURE_LINES {
            break;
        }
        let Ok(text) = std::fs::read_to_string(abs) else {
            continue;
        };
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("pub fn ")
                || t.starts_with("pub struct ")
                || t.starts_with("pub enum ")
                || t.starts_with("pub trait ")
                || t.starts_with("export function ")
                || t.starts_with("export const ")
                || t.starts_with("export class ")
            {
                out.push(format!("{rel}: {}", cut(t, 160)));
                if out.len() >= MAX_SIGNATURE_LINES {
                    break;
                }
            }
        }
    }
    out
}

/// ~25 lines: what the previous task left behind and how it was proved.
fn render_capsule(c: &HandoffCapsule) -> String {
    let mut lines = vec![
        format!("{} — {}", task_str(&c.task), cut(&c.summary, 400)),
        format!("verified by: {}", cut(&c.verification, 200)),
    ];
    if !c.touched_files.is_empty() {
        lines.push(format!("touched: {}", c.touched_files.join(", ")));
    }
    if !c.exported_symbols.is_empty() {
        lines.push(format!("exports: {}", c.exported_symbols.join(", ")));
    }
    for i in &c.invariants {
        lines.push(format!("invariant: {i}"));
    }
    for g in &c.gotchas {
        lines.push(format!("gotcha: {g}"));
    }
    lines.truncate(MAX_CAPSULE_LINES);
    lines.join("\n")
}

/// KNOWLEDGE.md of the project, tail-capped like the rest.
fn knowledge(project: &Project) -> Option<String> {
    let dir = parzi_core::project::dir(&project.workspace, &project.slug);
    let raw = std::fs::read_to_string(dir.join("KNOWLEDGE.md")).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let lines: Vec<&str> = raw.lines().collect();
    let start = lines.len().saturating_sub(120);
    Some(lines[start..].join("\n"))
}

fn cut(s: &str, n: usize) -> String {
    let one = s.lines().next().unwrap_or("").trim();
    let c: String = one.chars().take(n).collect();
    if one.chars().count() > n {
        format!("{c}…")
    } else {
        c
    }
}
