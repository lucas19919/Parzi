//! `project.plan`: the project's goal and ordered targets, which agents read
//! and change freely, and which drafts and helper sessions link themselves to.

use parzi_core::project_plan::{self, ProjectPlan};
use parzi_core::store::{SessionStatus, SessionStore};
use serde_json::Value;

use super::ToolHost;
use crate::tools::ApprovalMode;

/// How a target stands: done, being worked on by a running session, or open.
#[must_use]
pub fn target_state(store: &SessionStore, target: &project_plan::Target) -> &'static str {
    if target.done {
        return "done";
    }
    let working = target.sessions.iter().any(|id| {
        store
            .get(id)
            .is_ok_and(|m| matches!(m.status, SessionStatus::Active | SessionStatus::Queued))
    });
    if working {
        "working"
    } else {
        "todo"
    }
}

/// The plan as the window shows it: the goal and each target's title and
/// state. `None` when the project has no plan.
#[must_use]
pub fn view(store: &SessionStore, slug: &str) -> Option<Value> {
    let plan = project_plan::load(slug)?;
    let targets: Vec<Value> = plan
        .targets
        .iter()
        .map(|t| serde_json::json!({"title": t.title, "state": target_state(store, t)}))
        .collect();
    Some(serde_json::json!({"goal": plan.goal, "targets": targets}))
}

fn plan_text(store: &SessionStore, slug: &str, plan: &ProjectPlan) -> String {
    let mut out = format!("Project {slug}\nGoal: {}\nTargets:", plan.goal);
    if plan.targets.is_empty() {
        out.push_str(" none yet");
    }
    for (i, t) in plan.targets.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. [{}] {}",
            i + 1,
            target_state(store, t),
            t.title
        ));
    }
    out
}

impl ToolHost {
    fn plan_project(&self, args: &Value) -> Result<String, String> {
        let named = args
            .get("project")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|p| !p.is_empty());
        let own = self
            .p
            .store
            .get(&self.p.session_id)
            .map(|m| m.project)
            .unwrap_or_default();
        let slug = named.map_or(own, str::to_string);
        if slug.is_empty() || slug == "default" {
            return Err("this session has no project; switch to one first (project_switch)".into());
        }
        Ok(slug)
    }

    pub(super) fn execute_project_plan(&self, args: &Value) -> (bool, String) {
        let slug = match self.plan_project(args) {
            Ok(s) => s,
            Err(e) => return (false, e),
        };
        let goal = args.get("goal").and_then(Value::as_str);
        let targets = args.get("targets").and_then(Value::as_array).map(|list| {
            list.iter()
                .filter_map(|t| match t {
                    Value::String(s) => Some((s.clone(), false)),
                    Value::Object(o) => Some((
                        o.get("title")?.as_str()?.to_string(),
                        o.get("done").and_then(Value::as_bool).unwrap_or(false),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        let mut plan = project_plan::load(&slug).unwrap_or_default();
        if goal.is_none() && targets.is_none() {
            return (true, plan_text(&self.p.store, &slug, &plan));
        }
        if self.p.mode == ApprovalMode::Deny {
            return (
                false,
                "agents are read-only here: the project plan can be read, not changed".into(),
            );
        }
        plan.rewrite(goal, targets);
        if let Err(e) = project_plan::save(&slug, &plan) {
            return (false, e.to_string());
        }
        (
            true,
            format!("saved.\n{}", plan_text(&self.p.store, &slug, &plan)),
        )
    }

    /// Ties a draft or helper session to a target of this session's project
    /// plan. Says what happened, for the tool's answer.
    pub(super) fn link_target(&self, title: &str, session: &str) -> String {
        let slug = match self.plan_project(&Value::Null) {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
        let Some(mut plan) = project_plan::load(&slug) else {
            return format!(" (no project plan in {slug} to link the target to)");
        };
        if !plan.link(title, session) {
            return format!(" (no target \"{title}\" in the project plan)");
        }
        match project_plan::save(&slug, &plan) {
            Ok(()) => format!("; linked to the target \"{title}\""),
            Err(e) => format!(" (could not link the target: {e})"),
        }
    }
}
