//! The `project.*` tools: the three moves a role run makes on the project
//! itself. Each one is a thin call into `parzi-core` plus a journal line —
//! the file on disk is the result, the tool output only says what happened.
//!
//! Who may call what is the role's tool list (`roles::role_tools`); the checks
//! here are the second lock on the same door.

use parzi_core::journal::Kind;
use parzi_core::plan::{parse_plan_v1, Plan};
use parzi_core::project::{self, Status};
use crate::tools::ToolDef;
use serde_json::Value;

use super::files;
use crate::roles::{role_tools, Role, RoleCtx};

/// Names the project flow answers to. `project.approve` is listed so a model
/// that tries it gets told whose move it is, instead of "unknown tool".
#[must_use]
pub fn is_project_tool(name: &str) -> bool {
    matches!(
        name,
        "project.draft_plan" | "project.audit" | "project.approve" | "project.status"
    )
}

/// Every project tool, for the shell's tool catalogue.
#[must_use]
pub fn project_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "project.draft_plan".into(),
            description: "Save a rough plan for this project: prose and a bullet list of the pieces of work, in the order you would do them. Not PLAN.md — the orchestrator turns a draft into lanes and tasks.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "one line, what this draft proposes"},
                    "body": {"type": "string", "description": "the draft, markdown"},
                },
                "required": ["body"],
            }),
        },
        ToolDef {
            name: "project.audit".into(),
            description: "Write PLAN.md for this project from the draft you were given, in the plan grammar, plus a summary of at most 15 lines. This is the only way PLAN.md is written.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "plan": {"type": "string", "description": "the full PLAN.md text, first line `parzi: 1`"},
                    "summary": {"type": "string", "description": "what you planned and why, 15 lines at most"},
                },
                "required": ["plan", "summary"],
            }),
        },
        ToolDef {
            name: "project.status".into(),
            description: "The project's computed status: sprints, lanes, who holds what, the journal tail. Costs nothing and is never out of date.".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
        },
    ]
}

/// The project tools a given role may call — the defs filtered by the same
/// list the allowlist is built from, so the model is never shown a door that
/// is locked.
#[must_use]
pub fn project_defs_for(role: Role) -> Vec<ToolDef> {
    let allowed = role_tools(role);
    project_defs()
        .into_iter()
        .filter(|d| allowed.contains(&d.name))
        .collect()
}

/// Run one project tool for a role run. Sync work behind an async call site:
/// every branch is a file read or an atomic write, the same as `fs.*`.
#[must_use]
pub fn execute_project_tool(role: Role, ctx: &RoleCtx, name: &str, args: &Value) -> (bool, String) {
    if !role_tools(role).iter().any(|t| t == name) {
        return (
            false,
            format!("`{name}` is not the {}'s to call", role.as_str()),
        );
    }
    match name {
        "project.draft_plan" => draft_plan(ctx, args),
        "project.audit" => audit(ctx, args),
        "project.status" => status(ctx),
        "project.approve" => (
            false,
            "approving a plan is a person's move, not an agent's".into(),
        ),
        _ => (false, format!("unknown project tool `{name}`")),
    }
}

/// Header: save a rough plan. Returns the path, because that is what the
/// person clicks in the dock.
fn draft_plan(ctx: &RoleCtx, args: &Value) -> (bool, String) {
    let body = args.get("body").and_then(Value::as_str).unwrap_or("");
    let title = args.get("title").and_then(Value::as_str).unwrap_or("");
    match files::write_draft(&ctx.workspace, &ctx.slug, title, body) {
        Ok(draft) => {
            if let Err(e) = files::note(
                &ctx.workspace,
                &ctx.slug,
                Role::Header.as_str(),
                Kind::Note,
                None,
                &format!("draft {} — {}", draft.name, draft.title),
            ) {
                return (false, format!("draft saved but not journaled: {e}"));
            }
            (
                true,
                format!(
                    "saved draft {} at {}",
                    draft.name,
                    files::drafts_dir(&ctx.workspace, &ctx.slug)
                        .join(&draft.name)
                        .display()
                ),
            )
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Orchestrator: write PLAN.md. The text is parsed before it is written, and
/// written from the parse, so what lands on disk is what the parser accepted
/// (§15.6) — a plan that does not parse is a tool error the model can fix.
fn audit(ctx: &RoleCtx, args: &Value) -> (bool, String) {
    let text = args.get("plan").and_then(Value::as_str).unwrap_or("");
    let summary = args.get("summary").and_then(Value::as_str).unwrap_or("");
    if summary.trim().is_empty() {
        return (false, "the summary is required: 15 lines at most".into());
    }
    let plan = match parse_plan_v1(text) {
        Ok(p) => p,
        Err(e) => return (false, format!("PLAN.md does not parse: {e}")),
    };
    let mut project = match project::load(&ctx.workspace, &ctx.slug) {
        Ok(p) => p,
        Err(e) => return (false, e.to_string()),
    };
    if let Err(why) = check(&plan, &project.repos) {
        return (false, why);
    }
    if let Err(e) = files::write_plan(&ctx.workspace, &ctx.slug, &plan) {
        return (false, e.to_string());
    }
    if let Err(e) = files::write_summary(&ctx.workspace, &ctx.slug, summary) {
        return (
            false,
            format!("PLAN.md written but the summary was not: {e}"),
        );
    }
    if project.status == Status::Drafting {
        project.status = Status::Planned;
        if let Err(e) = project::save(&project) {
            return (
                false,
                format!("PLAN.md written but PROJECT.md was not: {e}"),
            );
        }
    }
    let lanes: usize = plan.sprints.iter().map(|s| s.lanes.len()).sum();
    let tasks = plan.tasks().count();
    if let Err(e) = files::note(
        &ctx.workspace,
        &ctx.slug,
        Role::Orchestrator.as_str(),
        Kind::PlanChanged,
        None,
        &format!(
            "{} sprints, {lanes} lanes, {tasks} tasks",
            plan.sprints.len()
        ),
    ) {
        return (false, format!("PLAN.md written but not journaled: {e}"));
    }
    (
        true,
        format!(
            "PLAN.md written: {} sprints, {lanes} lanes, {tasks} tasks. The person approves it; \
             do not dispatch anybody yourself.",
            plan.sprints.len()
        ),
    )
}

/// The checks a plan must pass before it is anybody's marching order. Each
/// failure names the task, so one tool error is enough to fix it.
fn check(plan: &Plan, repos: &[String]) -> std::result::Result<(), String> {
    let mut seen: Vec<&str> = vec![];
    for task in plan.tasks() {
        let id = task.id.as_str();
        if id.is_empty() {
            return Err("every task needs an id like TSK-7".into());
        }
        if seen.contains(&id) {
            return Err(format!("task id {id} is used twice; ids are unique"));
        }
        seen.push(id);
        if !repos.is_empty() && !repos.iter().any(|r| r == &task.repo) {
            return Err(format!(
                "{id} names repo `{}`, which is not one of this project's repos ({})",
                task.repo,
                repos.join(", ")
            ));
        }
        if task.scope.is_empty() {
            return Err(format!(
                "{id} has no [scope:…]; a task without files cannot be leased"
            ));
        }
    }
    for task in plan.tasks() {
        for dep in &task.after {
            if plan.task(dep).is_none() {
                return Err(format!(
                    "{} comes after {}, which is not in the plan",
                    task.id.as_str(),
                    dep.as_str()
                ));
            }
            if dep == &task.id {
                return Err(format!("{} comes after itself", task.id.as_str()));
            }
        }
    }
    if plan.sprints.is_empty() {
        return Err("a plan with no sprints is not a plan".into());
    }
    Ok(())
}

/// Any role: the computed status (§15.7). No model, no clock, no cost.
fn status(ctx: &RoleCtx) -> (bool, String) {
    match super::status(&ctx.workspace, &ctx.slug) {
        Ok(text) => (true, text),
        Err(e) => (false, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parzi_core::plan::{LanePlan, Sprint, Task, TaskId};

    fn plan_with(tasks: Vec<Task>) -> Plan {
        Plan {
            sprints: vec![Sprint {
                title: "Sprint 1".into(),
                target: String::new(),
                lanes: vec![LanePlan {
                    name: "api".into(),
                    tasks,
                }],
            }],
        }
    }

    fn task(id: &str, repo: &str) -> Task {
        Task {
            id: TaskId::from(id),
            title: "t".into(),
            repo: repo.into(),
            scope: vec!["src/**".into()],
            ..Task::default()
        }
    }

    #[test]
    fn a_plan_that_names_a_foreign_repo_is_refused() {
        let plan = plan_with(vec![task("TSK-1", "other")]);
        let err = check(&plan, &["api".to_string()]).unwrap_err();
        assert!(err.contains("TSK-1"), "{err}");
        assert!(check(&plan_with(vec![task("TSK-1", "api")]), &["api".to_string()]).is_ok());
    }

    #[test]
    fn duplicate_ids_and_ghost_dependencies_are_refused() {
        let dup = plan_with(vec![task("TSK-1", "api"), task("TSK-1", "api")]);
        assert!(check(&dup, &[]).unwrap_err().contains("twice"));
        let mut ghost = task("TSK-2", "api");
        ghost.after = vec![TaskId::from("TSK-9")];
        let plan = plan_with(vec![ghost]);
        assert!(check(&plan, &[]).unwrap_err().contains("TSK-9"));
    }

    #[test]
    fn a_task_without_scope_cannot_be_leased() {
        let mut t = task("TSK-1", "api");
        t.scope.clear();
        assert!(check(&plan_with(vec![t]), &[])
            .unwrap_err()
            .contains("scope"));
    }
}
