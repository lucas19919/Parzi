//! What each role is allowed to see (PLAN §5). Files in, text out: no role
//! ever receives another session's transcript, and nothing here costs a model
//! call to produce.

use std::path::PathBuf;

use parzi_core::capsule;
use parzi_core::journal;
use parzi_core::project;

use super::{digest::workspace_digest, ContextPart, RoleCtx};
use crate::project_flow::files;

/// Journal lines the header is briefed with.
const JOURNAL_TAIL: usize = 40;
/// Bytes kept per whole-file part (PROJECT.md, PLAN.md, KNOWLEDGE.md).
const FILE_CAP: usize = 24_000;

/// Header: the workspace digest, PROJECT.md, KNOWLEDGE.md, STATUS.md, the
/// capsules and the journal tail. All of it computed, none of it generated.
pub fn header(ctx: &RoleCtx) -> Vec<ContextPart> {
    let mut parts = workspace_digest(&ctx.workspace);
    parts.extend(file_part("project", project_file(ctx, "PROJECT.md")));
    parts.extend(file_part("knowledge", project_file(ctx, "KNOWLEDGE.md")));
    parts.push(ContextPart::new("status", status_text(ctx)));
    parts.extend(capsules(ctx));
    parts.extend(journal_tail(ctx, JOURNAL_TAIL));
    parts
}

/// Orchestrator: PROJECT.md, every draft, the current PLAN.md, the capsules
/// and STATUS.md. The draft it is auditing is named in its dispatch message.
pub fn orchestrator(ctx: &RoleCtx) -> Vec<ContextPart> {
    let mut parts = vec![];
    parts.extend(file_part("project", project_file(ctx, "PROJECT.md")));
    for draft in files::drafts(&ctx.workspace, &ctx.slug).unwrap_or_default() {
        parts.push(ContextPart::new(
            &format!("draft:{}", draft.name),
            draft.body,
        ));
    }
    parts.extend(file_part("plan", project_file(ctx, "PLAN.md")));
    parts.extend(file_part("knowledge", project_file(ctx, "KNOWLEDGE.md")));
    parts.extend(capsules(ctx));
    parts.push(ContextPart::new("status", status_text(ctx)));
    parts
}

/// Coder: the lease lane's micro-context for exactly one task — the task, its
/// acceptance lines, the files in scope, the signatures around them, the
/// capsules it comes after, KNOWLEDGE.md. Nothing about the project at large.
pub fn coder(ctx: &RoleCtx) -> Vec<ContextPart> {
    let Some(id) = ctx.task.as_deref().map(parzi_core::plan::TaskId::from) else {
        return vec![ContextPart::new(
            "task",
            "(no task was dispatched with this run)",
        )];
    };
    let (Ok(project), Ok(plan)) = (
        project::load(&ctx.workspace, &ctx.slug),
        files::plan(&ctx.workspace, &ctx.slug),
    ) else {
        return vec![];
    };
    let Some(task) = plan.task(&id).cloned() else {
        return vec![ContextPart::new(
            "task",
            format!("(task {} is not in PLAN.md)", id.as_str()),
        )];
    };
    let after = capsule::for_after(&ctx.workspace, &ctx.slug, &task.after);
    crate::context_micro::micro_context(&project, &plan, &task, &after)
}

// ------------------------------------------------------------ file pieces

fn project_file(ctx: &RoleCtx, name: &str) -> PathBuf {
    project::dir(&ctx.workspace, &ctx.slug).join(name)
}

/// A whole file as one part, or nothing at all when it is not there yet.
fn file_part(label: &str, path: PathBuf) -> Option<ContextPart> {
    let raw = std::fs::read_to_string(&path).ok()?;
    Some(ContextPart::new(label, cap(&raw, FILE_CAP)))
}

fn cap(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let cut: String = s.chars().take(n).collect();
    format!("{cut}\n…(truncated)")
}

/// STATUS.md as `parzi_core::status` renders it — the same text the deck and
/// the `project.status` tool show (§15.7).
fn status_text(ctx: &RoleCtx) -> String {
    crate::project_flow::status(&ctx.workspace, &ctx.slug)
        .unwrap_or_else(|e| format!("(status unavailable: {e})"))
}

/// Every capsule the project has, each as the ~25-line block §5 asks for.
fn capsules(ctx: &RoleCtx) -> Option<ContextPart> {
    let mut body = String::new();
    for id in files::capsule_ids(&ctx.workspace, &ctx.slug) {
        if let Ok(c) = files::capsule(&ctx.workspace, &ctx.slug, &id) {
            body.push_str(&c.render());
            body.push('\n');
        }
    }
    (!body.is_empty()).then(|| ContextPart::new("capsules", body))
}

fn journal_tail(ctx: &RoleCtx, n: usize) -> Option<ContextPart> {
    let lines = journal::tail(&ctx.workspace, &ctx.slug, n);
    if lines.is_empty() {
        return None;
    }
    let body = lines
        .iter()
        .map(|l| {
            format!(
                "{} {} {} {}",
                l.at.format("%Y-%m-%d %H:%M:%SZ"),
                l.who,
                l.kind.as_str(),
                l.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(ContextPart::new("activity", body))
}
