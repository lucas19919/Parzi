//! `lease.*` tools and the write-time enforcement of PLAN §4.
//!
//! A lane checks out a task by claiming the files it needs; a write into
//! somebody else's claim is a tool error naming the holder; a write outside
//! the lane's own scope is allowed and noticed. Asking is a first-class move:
//! `lease.request` reaches the holder's session, `lease.grant`/`lease.deny`
//! answer it, and a `critical:` path turns the request into a human approval
//! card instead.

pub mod audit;
pub mod enforce;
pub mod hub;
pub mod naming;
pub mod requests;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use parzi_core::lease::Answer;
use parzi_core::project::normalize_path;
use parzi_providers::ToolDef;

use crate::tools::ToolExecutor;

/// H-5: a lease request travels as one of these. The type is `inter`'s; it is
/// re-exported here because the lease tools are its only sender today.
pub use crate::inter::{InterKind, InterSessionMessage};
pub use hub::{LeaseHub, ProjectKey, ANSWER_TIMEOUT_SECS};
pub use naming::{held_by, task_id, task_str};
pub use requests::Outcome;

/// What one run needs to take part in the lease layer: the shared hub and its
/// own run id. Everything else (holder, project, repo, approver) is registered
/// on the hub, so a lane that moves machines changes one record, not a graph.
#[derive(Clone)]
pub struct LeaseCtx {
    pub hub: Arc<LeaseHub>,
    pub run: String,
}

impl LeaseCtx {
    pub fn new(hub: Arc<LeaseHub>, run: &str) -> Self {
        Self {
            hub,
            run: run.to_string(),
        }
    }
}

pub fn is_lease_tool(name: &str) -> bool {
    matches!(
        name,
        "lease.claim" | "lease.release" | "lease.request" | "lease.grant" | "lease.deny"
    )
}

pub fn lease_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "lease.claim".into(),
            description: "Check out a task: claim the repo-relative files (or globs) it needs. Granted only when no live lease overlaps; otherwise the holder is named and you ask with lease.request.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string", "description": "task id, e.g. TSK-7"},
                    "paths": {"type": "array", "items": {"type": "string"}},
                    "repo": {"type": "string", "description": "repo name; defaults to this lane's repo"},
                },
                "required": ["task", "paths"],
            }),
        },
        ToolDef {
            name: "lease.release".into(),
            description: "Release the files claimed for a task. Call it when the task is done or abandoned.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"task": {"type": "string"}},
                "required": ["task"],
            }),
        },
        ToolDef {
            name: "lease.request".into(),
            description: "Ask the current holder for one file you need. Blocks until the holder answers; no answer is a denial. A file marked critical in PROJECT.md goes to a human instead.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "for_task": {"type": "string"},
                    "reason": {"type": "string"},
                    "repo": {"type": "string"},
                },
                "required": ["path", "for_task", "reason"],
            }),
        },
        ToolDef {
            name: "lease.grant".into(),
            description: "Answer a delivered lease request by handing the file over.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"request_id": {"type": "string"}},
                "required": ["request_id"],
            }),
        },
        ToolDef {
            name: "lease.deny".into(),
            description: "Answer a delivered lease request with a refusal and its reason.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "request_id": {"type": "string"},
                    "reason": {"type": "string"},
                },
                "required": ["request_id", "reason"],
            }),
        },
    ]
}

impl ToolExecutor {
    /// PLAN §4 enforcement, called before every tool runs. `Some((false, msg))`
    /// refuses the call; `None` lets it through. Every path a tool is about to
    /// write is checked — `tools::write_path_args` is the list, so a new write
    /// tool is gated the day it is added — and reads are never blocked.
    pub(crate) async fn lease_gate(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Option<(bool, String)> {
        let ctx = self.leases.as_ref()?;
        for key in crate::tools::write_path_args(name) {
            let Some(path) = args.get(*key).and_then(|v| v.as_str()) else {
                continue;
            };
            if let Some(refusal) = ctx.hub.gate_write(&ctx.run, path).await {
                return Some(refusal);
            }
        }
        None
    }

    /// `shell.exec`'s half of the gate: the command owns a whole worktree, so
    /// what it wrote is only knowable afterwards. Fingerprint before, run,
    /// fingerprint after, and hold what moved to the same rule (§4).
    pub(crate) async fn lease_audit_shell<F>(&self, run_it: F) -> (bool, String)
    where
        F: std::future::Future<Output = (bool, String)>,
    {
        let Some(ctx) = self.leases.as_ref() else {
            return run_it.await;
        };
        let backup = backup_files(&self.cwd, &ctx.hub.foreign_files(&ctx.run, &self.cwd).await);
        let before = audit::snapshot(&self.cwd).await;
        let out = run_it.await;
        let Some(before) = before else { return out };
        let Some(after) = audit::snapshot(&self.cwd).await else {
            return out;
        };
        let changed = audit::changed(&before, &after);
        if changed.is_empty() {
            return out;
        }
        match ctx.hub.audit_writes(&ctx.run, &changed).await {
            Some((refusal, hits)) => {
                restore_files(&self.cwd, &backup, &hits);
                (false, format!("{refusal}\n\n{}", out.1))
            }
            None => out,
        }
    }

    /// Execute one `lease.*` tool. Every action is journalled by the hub.
    pub(crate) async fn execute_lease_tool(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> (bool, String) {
        let Some(ctx) = self.leases.as_ref() else {
            return (false, "lease tools need a project lane".into());
        };
        let str_arg = |k: &str| {
            args.get(k)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        match name {
            "lease.claim" => {
                let Some(task) = str_arg("task") else {
                    return (false, "lease.claim needs `task`".into());
                };
                let repo = match self.lease_repo(ctx, str_arg("repo")).await {
                    Ok(r) => r,
                    Err(e) => return (false, e),
                };
                let paths = qualify(args.get("paths"), &repo);
                if paths.is_empty() {
                    return (false, "lease.claim needs at least one path".into());
                }
                match ctx.hub.claim(&ctx.run, &task_id(&task), paths).await {
                    Ok(v) => (true, v.to_string()),
                    Err(e) => (false, e.to_string()),
                }
            }
            "lease.release" => {
                let Some(task) = str_arg("task") else {
                    return (false, "lease.release needs `task`".into());
                };
                match ctx.hub.release(&ctx.run, &task_id(&task)).await {
                    Ok(()) => (true, format!("released {task}")),
                    Err(e) => (false, e.to_string()),
                }
            }
            "lease.request" => {
                let (Some(path), Some(for_task)) = (str_arg("path"), str_arg("for_task")) else {
                    return (false, "lease.request needs `path` + `for_task`".into());
                };
                let reason = str_arg("reason").unwrap_or_else(|| "no reason given".into());
                let repo = match self.lease_repo(ctx, str_arg("repo")).await {
                    Ok(r) => r,
                    Err(e) => return (false, e),
                };
                let key = format!("{repo}/{}", normalize_path(&path));
                match ctx
                    .hub
                    .request(&ctx.run, &key, &task_id(&for_task), &reason)
                    .await
                {
                    Ok(Outcome::Granted) => (true, format!("granted: {key} is yours")),
                    Ok(Outcome::Denied(why)) => (false, format!("denied: {why}")),
                    Err(e) => (false, e.to_string()),
                }
            }
            "lease.grant" | "lease.deny" => {
                let Some(id) = str_arg("request_id") else {
                    return (false, format!("{name} needs `request_id`"));
                };
                let answer = if name == "lease.grant" {
                    Answer::Grant
                } else {
                    Answer::Deny {
                        reason: str_arg("reason").unwrap_or_else(|| "no reason given".into()),
                    }
                };
                match ctx.hub.answer(&ctx.run, &id, answer).await {
                    Ok(text) => (true, text),
                    Err(e) => (false, e.to_string()),
                }
            }
            _ => (false, format!("unknown lease tool `{name}`")),
        }
    }

    /// The repo a lease path belongs to: the argument, else the lane's bound
    /// repo. Without either there is no `<repo>/` prefix to build, and a
    /// lease on a bare relative path would collide across repos.
    async fn lease_repo(
        &self,
        ctx: &LeaseCtx,
        arg: Option<String>,
    ) -> std::result::Result<String, String> {
        if let Some(r) = arg {
            return Ok(r);
        }
        ctx.hub
            .repo_of_run(&ctx.run)
            .await
            .ok_or_else(|| "this lane has no repo bound; pass `repo`".to_string())
    }
}

/// Bytes of each foreign-held file, taken before `shell.exec`. `None` means
/// the path did not exist, so a violation that created it is undone by delete.
fn backup_files(cwd: &str, rels: &[String]) -> HashMap<String, Option<Vec<u8>>> {
    rels.iter()
        .map(|rel| {
            (
                rel.clone(),
                std::fs::read(std::path::Path::new(cwd).join(rel)).ok(),
            )
        })
        .collect()
}

/// Put a held file back the way it was, or delete it if the command created it.
fn restore_files(cwd: &str, backup: &HashMap<String, Option<Vec<u8>>>, hits: &[String]) {
    for rel in hits {
        let path = std::path::Path::new(cwd).join(rel);
        match backup.get(rel) {
            Some(Some(bytes)) => {
                let _ = std::fs::write(&path, bytes);
            }
            _ => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// `["src/a.rs"]` + repo `api` → `{"api/src/a.rs"}`. Paths that already carry
/// the repo prefix are left alone, so a plan's `[scope:api/src/**]` works too.
fn qualify(paths: Option<&serde_json::Value>, repo: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(list) = paths.and_then(|v| v.as_array()) else {
        return out;
    };
    for p in list.iter().filter_map(|v| v.as_str()) {
        let n = normalize_path(p);
        if n.is_empty() {
            continue;
        }
        if n == repo || n.starts_with(&format!("{repo}/")) {
            out.insert(n);
        } else {
            out.insert(format!("{repo}/{n}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualify_prefixes_once() {
        let v = serde_json::json!(["src/a.rs", "api/src/b.rs", r"src\c.rs", ""]);
        let out = qualify(Some(&v), "api");
        assert!(out.contains("api/src/a.rs"));
        assert!(out.contains("api/src/b.rs"));
        assert!(out.contains("api/src/c.rs"));
        assert_eq!(out.len(), 3, "empty paths dropped: {out:?}");
    }

    #[test]
    fn lease_tool_names_are_the_contract() {
        for n in [
            "lease.claim",
            "lease.release",
            "lease.request",
            "lease.grant",
            "lease.deny",
        ] {
            assert!(is_lease_tool(n), "missing {n}");
            assert!(lease_defs().iter().any(|d| d.name == n), "no def for {n}");
        }
        assert!(!is_lease_tool("fs.write"));
    }
}
