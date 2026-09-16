//! `board.*` tools: what is live, handing a task over, and saying it is stuck.
//!
//! The board is the live overlay on PLAN.md (PLAN §6): the plan gives the
//! tasks, the lease table gives the holders. A task ends with a handoff
//! capsule — validated like a widget, written to `capsules/<TSK>.json` *and*
//! appended to the session as an artifact, so it survives on both truths.

use parzi_core::error::{ParziError, Result};
use parzi_core::journal::JournalKind;
use parzi_core::store::Event;
use parzi_providers::ToolDef;

use crate::lease_tools::{held_by, task_id, task_str, LeaseCtx};
use crate::tools::ToolExecutor;

pub fn is_board_tool(name: &str) -> bool {
    matches!(name, "board.list" | "board.handoff" | "board.block")
}

pub fn board_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "board.list".into(),
            description: "The live board: this project's sprints, lanes and tasks with who holds what right now. Reads state, costs no model.".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
        },
        ToolDef {
            name: "board.handoff".into(),
            description: "End a task with its handoff capsule: summary, touched files, exported symbols, verification (required — the command that proves it), invariants, gotchas. Releases the task's files.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string"},
                    "capsule": {
                        "type": "object",
                        "properties": {
                            "task": {"type": "string"},
                            "summary": {"type": "string"},
                            "touched_files": {"type": "array", "items": {"type": "string"}},
                            "exported_symbols": {"type": "array", "items": {"type": "string"}},
                            "verification": {"type": "string"},
                            "invariants": {"type": "array", "items": {"type": "string"}},
                            "gotchas": {"type": "array", "items": {"type": "string"}},
                        },
                        "required": ["summary", "verification"],
                    },
                },
                "required": ["task", "capsule"],
            }),
        },
        ToolDef {
            name: "board.block".into(),
            description: "Mark this task blocked with the reason. The orchestrator sees it and re-plans; the lane keeps its files unless it also releases them.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {"type": "string"},
                    "reason": {"type": "string"},
                },
                "required": ["task", "reason"],
            }),
        },
    ]
}

impl ToolExecutor {
    pub(crate) async fn execute_board_tool(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> (bool, String) {
        let Some(ctx) = self.leases.as_ref() else {
            return (false, "board tools need a project lane".into());
        };
        let str_arg = |k: &str| {
            args.get(k)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        match name {
            "board.list" => match board_json(ctx).await {
                Ok(v) => (true, v.to_string()),
                Err(e) => (false, e.to_string()),
            },
            "board.handoff" => {
                let Some(task) = str_arg("task") else {
                    return (false, "board.handoff needs `task`".into());
                };
                let Some(capsule) = args.get("capsule") else {
                    return (false, "board.handoff needs `capsule`".into());
                };
                match handoff(ctx, &task, capsule).await {
                    Ok(text) => (true, text),
                    Err(e) => (false, e.to_string()),
                }
            }
            "board.block" => {
                let (Some(task), Some(reason)) = (str_arg("task"), str_arg("reason")) else {
                    return (false, "board.block needs `task` + `reason`".into());
                };
                ctx.hub
                    .journal(
                        &ctx.run,
                        JournalKind::Block,
                        Some(&task_id(&task)),
                        &format!("blocked: {reason}"),
                    )
                    .await;
                (true, format!("{task} marked blocked: {reason}"))
            }
            _ => (false, format!("unknown board tool `{name}`")),
        }
    }
}

/// Plan tasks joined with live leases. Missing PLAN.md is not an error: a
/// project that has not been planned yet still has a board of leases.
async fn board_json(ctx: &LeaseCtx) -> Result<serde_json::Value> {
    let leases = ctx.hub.snapshot().await;
    let holders: Vec<serde_json::Value> = leases
        .iter()
        .map(|l| {
            serde_json::json!({
                "task": task_str(&l.task),
                "lane": l.holder.lane,
                "user": l.holder.user,
                "machine": l.holder.machine,
                "run": l.holder.run,
                "paths": l.paths.iter().collect::<Vec<_>>(),
                "since": l.granted_at.to_rfc3339(),
            })
        })
        .collect();
    let mut sprints = vec![];
    if let Some(key) = ctx.hub.project_of_run(&ctx.run).await {
        let dir = parzi_core::project::dir(&key.workspace, &key.slug);
        if let Ok(raw) = std::fs::read_to_string(dir.join("PLAN.md")) {
            let plan = parzi_core::plan::parse_plan_v1(&raw)?;
            for s in &plan.sprints {
                let lanes: Vec<serde_json::Value> = s
                    .lanes
                    .iter()
                    .map(|l| {
                        let tasks: Vec<serde_json::Value> = l
                            .tasks
                            .iter()
                            .map(|t| {
                                let holder = leases
                                    .iter()
                                    .find(|x| x.task == t.id)
                                    .map(|x| held_by(x))
                                    .unwrap_or_default();
                                serde_json::json!({
                                    "id": task_str(&t.id),
                                    "title": t.title,
                                    "repo": t.repo,
                                    "scope": t.scope,
                                    "after": t.after.iter().map(task_str).collect::<Vec<_>>(),
                                    "critical": t.critical,
                                    "done": t.done,
                                    "held_by": holder,
                                })
                            })
                            .collect();
                        serde_json::json!({"lane": l.name, "tasks": tasks})
                    })
                    .collect();
                sprints.push(serde_json::json!({
                    "title": s.title,
                    "target": s.target,
                    "lanes": lanes,
                }));
            }
        }
    }
    Ok(serde_json::json!({"sprints": sprints, "leases": holders}))
}

/// Validate, write `capsules/<TSK>.json`, append the same capsule as an
/// artifact event, journal it, and release the task's files.
async fn handoff(ctx: &LeaseCtx, task: &str, capsule: &serde_json::Value) -> Result<String> {
    let key = ctx
        .hub
        .project_of_run(&ctx.run)
        .await
        .ok_or_else(|| ParziError::Tool("board".into(), "this lane has no project".into()))?;
    // The capsule always names its own task, whatever the model repeated.
    let mut value = capsule.clone();
    if let Some(obj) = value.as_object_mut() {
        obj.insert("task".into(), serde_json::Value::String(task.to_string()));
    }
    let validated = parzi_core::capsule::validate(&value)?;
    parzi_core::capsule::save(&key.workspace, &key.slug, &validated)?;
    let path =
        parzi_core::capsule::path(&key.workspace, &key.slug, &validated.task).unwrap_or_default();
    if let Some(store) = ctx.hub.store_of_run(&ctx.run).await {
        store.append(
            &ctx.run,
            &Event::Artifact {
                id: format!("capsule-{}", task_str(&validated.task).to_lowercase()),
                title: format!("Handoff {}", task_str(&validated.task)),
                artifact_kind: "json".into(),
                version: 1,
                payload: serde_json::to_value(&validated)?,
            },
        )?;
    }
    ctx.hub
        .journal(
            &ctx.run,
            JournalKind::Handoff,
            Some(&validated.task),
            &format!(
                "handoff: {} (verified by `{}`)",
                validated.summary.lines().next().unwrap_or_default(),
                validated.verification
            ),
        )
        .await;
    ctx.hub.release(&ctx.run, &validated.task).await?;
    Ok(format!(
        "capsule stored at {} and the task's files are released",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_tool_names_are_the_contract() {
        for n in ["board.list", "board.handoff", "board.block"] {
            assert!(is_board_tool(n), "missing {n}");
            assert!(board_defs().iter().any(|d| d.name == n), "no def for {n}");
        }
        assert!(!is_board_tool("lease.claim"));
    }
}
