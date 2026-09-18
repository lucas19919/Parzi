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

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::tools::ToolDef;
use parzi_core::lease::Answer;
use parzi_core::project::{normalize_path, path_key};

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
    /// PLAN §4, before an agent edits: every worktree-relative path it is
    /// about to write is checked, and the first refusal wins. Reads never
    /// reach this gate.
    pub(crate) async fn lease_gate_paths(&self, paths: &[String]) -> Option<String> {
        let ctx = self.leases.as_ref()?;
        for path in paths {
            if let Some((false, why)) = ctx.hub.gate_write(&ctx.run, path).await {
                return Some(why);
            }
        }
        None
    }

    /// Before an agent's shell command: what it writes is only knowable
    /// afterwards, so back up the files other lanes hold and fingerprint the
    /// worktree now. `None` = this run is not in the lease layer.
    pub(crate) async fn shell_audit_start(&self) -> Option<ShellAudit> {
        let ctx = self.leases.as_ref()?;
        let (foreign, listed) = ctx.hub.foreign_files(&ctx.run, &self.cwd).await;
        let backup = backup_files(&self.cwd, &foreign, listed);
        let before = audit::snapshot(&self.cwd).await?;
        Some(ShellAudit { backup, before })
    }

    /// After it: whatever moved inside a file another lane holds is put
    /// back, and the refusal (naming the holder) comes back for the agent.
    pub(crate) async fn shell_audit_finish(&self, audit: ShellAudit) -> Option<String> {
        let ctx = self.leases.as_ref()?;
        let after = audit::snapshot(&self.cwd).await?;
        let changed = audit::changed(&audit.before, &after);
        if changed.is_empty() {
            return None;
        }
        let (refusal, hits) = ctx.hub.audit_writes(&ctx.run, &changed).await?;
        let left = restore_files(&self.cwd, &audit.backup, &hits);
        if left.is_empty() {
            return Some(refusal);
        }
        Some(format!(
            "{refusal}; could not put back: {} — tell the holder",
            left.join(", ")
        ))
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

/// A shell command's before-picture: held files' bytes and the worktree's
/// fingerprint.
pub(crate) struct ShellAudit {
    backup: Backup,
    before: audit::Snapshot,
}

/// Another lane's files as they were before `shell.exec`, by path key (a
/// lease and the disk may spell a name in different case).
struct Backup {
    /// Each file's bytes, or `None` for a named file that did not exist. A
    /// file that could not be read has no entry.
    files: HashMap<String, Option<Vec<u8>>>,
    /// Every file in the worktree, when it was walked: a changed file that
    /// is not in it was created by the command.
    listed: Option<HashSet<String>>,
}

fn backup_files(cwd: &str, rels: &[String], listed: Option<Vec<String>>) -> Backup {
    let files = rels
        .iter()
        .filter_map(|rel| {
            let bytes = match std::fs::read(std::path::Path::new(cwd).join(rel)) {
                Ok(b) => Some(b),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return None,
            };
            Some((path_key(rel), bytes))
        })
        .collect();
    let listed = listed.map(|all| all.iter().map(|rel| path_key(rel)).collect());
    Backup { files, listed }
}

/// Put each held file back the way it was, or delete it when the command
/// created it. A file with no copy that may have existed before is left as
/// it is. Returns the files that could not be put back.
fn restore_files(cwd: &str, backup: &Backup, hits: &[String]) -> Vec<String> {
    let remove = |path: &std::path::Path| std::fs::remove_file(path).is_ok() || !path.exists();
    let mut left = vec![];
    for rel in hits {
        let path = std::path::Path::new(cwd).join(rel);
        let key = path_key(rel);
        let restored = match backup.files.get(&key) {
            Some(Some(bytes)) => std::fs::write(&path, bytes).is_ok(),
            Some(None) => remove(&path),
            None if backup.listed.as_ref().is_some_and(|l| !l.contains(&key)) => remove(&path),
            None => false,
        };
        if !restored {
            left.push(rel.clone());
        }
    }
    left
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
    use parzi_core::project::PATHS_IGNORE_CASE;

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

    /// The undo never deletes a file it has no copy of unless the walk
    /// before the command proves the file did not exist.
    #[test]
    fn a_file_with_no_copy_is_never_deleted() {
        let dir = std::env::temp_dir().join(format!("parzi-restore-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.to_str().unwrap();
        std::fs::write(dir.join("kept.rs"), "was here").unwrap();
        std::fs::write(dir.join("made.rs"), "new").unwrap();
        // The walk saw kept.rs (unreadable then, or claimed while the command
        // ran: no copy) and not made.rs.
        let backup = Backup {
            files: HashMap::new(),
            listed: Some([path_key("kept.rs")].into_iter().collect()),
        };
        let hits = ["kept.rs".to_string(), "made.rs".to_string()];
        assert_eq!(
            restore_files(cwd, &backup, &hits),
            vec!["kept.rs".to_string()]
        );
        assert!(dir.join("kept.rs").exists(), "it may have existed: left");
        assert!(
            !dir.join("made.rs").exists(),
            "the command made it: removed"
        );

        std::fs::write(dir.join("made.rs"), "new").unwrap();
        let blind = Backup {
            files: HashMap::new(),
            listed: None,
        };
        assert_eq!(restore_files(cwd, &blind, &hits[1..]).len(), 1);
        assert!(dir.join("made.rs").exists(), "no listing, no delete");

        // A copy taken under the lease's spelling restores the disk's.
        std::fs::write(dir.join("Held.rs"), "old").unwrap();
        let copy = backup_files(cwd, &["Held.rs".to_string()], None);
        std::fs::write(dir.join("Held.rs"), "overwritten").unwrap();
        let disk = if PATHS_IGNORE_CASE {
            "held.rs"
        } else {
            "Held.rs"
        };
        assert!(restore_files(cwd, &copy, &[disk.to_string()]).is_empty());
        assert_eq!(std::fs::read_to_string(dir.join("Held.rs")).unwrap(), "old");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
