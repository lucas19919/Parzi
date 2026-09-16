//! Approve and dispatch: the human's move that turns a plan into running
//! lanes. One coder run per lane of the first unfinished sprint, each in its
//! own worktree, each with its lease identity registered before it starts —
//! a lane whose first action is `lease.claim` must already be somebody the
//! lease table knows.

use std::sync::Arc;

use parzi_core::error::{ParziError, Result};
use parzi_core::journal::Kind;
use parzi_core::plan::{Plan, Task};
use parzi_core::project::{self, Project, Status};

use serde::Serialize;

use super::files;
use crate::lease_tools::LeaseHub;
use crate::roles::Role;
use crate::tools::Approver;
use crate::Orchestrator;

/// One lane started by `approve` or `dispatch_lane`.
#[derive(Debug, Clone, Serialize)]
pub struct Dispatched {
    pub lane: String,
    pub task: String,
    pub session_id: String,
    pub cwd: String,
}

/// The human's move: the plan is good, start sprint 1. Every lane of the
/// first unfinished sprint gets one coder run on its first ready task, in a
/// worktree of that task's repo, with `lease.claim` as its first instruction.
pub async fn approve(
    orch: &Arc<Orchestrator>,
    workspace: &str,
    slug: &str,
    who: &str,
    approver: Option<Arc<dyn Approver>>,
) -> Result<Vec<Dispatched>> {
    let hub = orch.leases();
    let mut project = project::load(workspace, slug)?;
    if project.status == Status::Drafting {
        return Err(ParziError::Validation(
            "nothing to approve yet: send a draft to the orchestrator first".into(),
        ));
    }
    let plan = files::plan(workspace, slug)?;
    let sprint = plan
        .sprints
        .iter()
        .find(|s| s.lanes.iter().any(|l| l.tasks.iter().any(|t| !t.done)))
        .ok_or_else(|| ParziError::Validation("every task in the plan is done".into()))?;

    files::note(
        workspace,
        slug,
        who,
        Kind::Approve,
        None,
        &format!("approved {}", sprint.title),
    )?;

    let mut out = vec![];
    for lane in &sprint.lanes {
        let Some(task) = next_ready(lane.tasks.as_slice(), &plan) else {
            continue;
        };
        let cwd = task_cwd(workspace, &project, task, &lane.name)?;
        let prompt = coder_prompt(task, &lane.name);
        // The session first, then its lease identity, and only then the run:
        // a coder whose first action is `lease.claim` must already be somebody
        // the lease table knows.
        let meta = orch.role_session(
            workspace,
            slug,
            Role::Coder,
            &lane.name,
            Some(task.id.as_str()),
            &cwd,
        )?;
        files::set_session(workspace, slug, Role::Coder, &lane.name, &meta.id)?;
        bind_run(
            &hub,
            &meta.id,
            workspace,
            slug,
            &lane.name,
            &task.repo,
            orch,
            approver.clone(),
        )
        .await;
        // The lane's events go to the host bus, which the deck subscribes to;
        // nobody waits on a coder here.
        drop(
            orch.send_role(
                workspace,
                slug,
                Role::Coder,
                &meta.id,
                &prompt,
                approver.clone(),
            )
            .await?,
        );
        files::note(
            workspace,
            slug,
            &lane.name,
            Kind::Note,
            Some(task.id.as_str()),
            &format!("lane {} dispatched on {}", lane.name, task.id),
        )?;
        out.push(Dispatched {
            lane: lane.name.clone(),
            task: task.id.0.clone(),
            session_id: meta.id,
            cwd,
        });
    }
    if out.is_empty() {
        return Err(ParziError::Validation(
            "no task in this sprint is ready: every lane waits on an `after:` that is not done"
                .into(),
        ));
    }
    project.status = Status::Running;
    project::save(&project)?;
    Ok(out)
}

/// Start an agent on a specific lane or task.
pub async fn dispatch_lane(
    orch: &Arc<Orchestrator>,
    workspace: &str,
    slug: &str,
    lane_name: &str,
    task_id: Option<&str>,
    who: &str,
    approver: Option<Arc<dyn Approver>>,
) -> Result<Dispatched> {
    let hub = orch.leases();
    let mut project = project::load(workspace, slug)?;
    let plan = files::plan(workspace, slug)?;

    let task = if let Some(tid) = task_id {
        plan.tasks()
            .find(|t| t.id.as_str() == tid)
            .ok_or_else(|| ParziError::Validation(format!("task {tid} not found in plan")))?
    } else {
        let lane_plan = plan
            .sprints
            .iter()
            .flat_map(|s| s.lanes.iter())
            .find(|l| l.name == lane_name)
            .ok_or_else(|| ParziError::Validation(format!("lane {lane_name} not found in plan")))?;
        next_ready(lane_plan.tasks.as_slice(), &plan)
            .ok_or_else(|| ParziError::Validation(format!("no ready tasks in lane {lane_name}")))?
    };

    let cwd = task_cwd(workspace, &project, task, lane_name)?;
    let prompt = coder_prompt(task, lane_name);
    let meta = orch.role_session(
        workspace,
        slug,
        Role::Coder,
        lane_name,
        Some(task.id.as_str()),
        &cwd,
    )?;
    files::set_session(workspace, slug, Role::Coder, lane_name, &meta.id)?;
    bind_run(
        &hub,
        &meta.id,
        workspace,
        slug,
        lane_name,
        &task.repo,
        orch,
        approver.clone(),
    )
    .await;
    drop(
        orch.send_role(
            workspace,
            slug,
            Role::Coder,
            &meta.id,
            &prompt,
            approver.clone(),
        )
        .await?,
    );
    files::note(
        workspace,
        slug,
        lane_name,
        Kind::Note,
        Some(task.id.as_str()),
        &format!("{who}: lane {lane_name} dispatched on {}", task.id),
    )?;
    if project.status == Status::Drafting || project.status == Status::Planned {
        project.status = Status::Running;
        let _ = project::save(&project);
    }
    Ok(Dispatched {
        lane: lane_name.to_string(),
        task: task.id.0.clone(),
        session_id: meta.id,
        cwd,
    })
}

/// First task of a lane that is not done and whose `after:` tasks are all
/// done. `after` pointing at a task that does not exist is treated as unmet —
/// a plan that names a ghost never silently starts work.
fn next_ready<'a>(tasks: &'a [Task], plan: &Plan) -> Option<&'a Task> {
    tasks.iter().find(|t| {
        !t.done
            && t.after
                .iter()
                .all(|dep| plan.task(dep).is_some_and(|d| d.done))
    })
}

/// Where a coder run works: an isolated worktree of the task's repo when the
/// repo is a git checkout (§4), the repo itself when it is not, and the
/// project directory when the repo is not mapped on this machine.
fn task_cwd(workspace: &str, project: &Project, task: &Task, lane: &str) -> Result<String> {
    let ws = parzi_core::workspace::load(workspace)?;
    let Some(repo) = ws.repo(&task.repo).and_then(|r| r.local_path.clone()) else {
        return Ok(project::dir(workspace, &project.slug).display().to_string());
    };
    let repo = repo.display().to_string();
    if crate::git_worktree::is_git_repo(&repo) {
        if let Ok(wt) = crate::git_worktree::create_worktree(
            &repo,
            &project.slug,
            lane,
            &worktree_key(task.id.as_str()),
        ) {
            return Ok(wt.display().to_string());
        }
    }
    Ok(repo)
}

/// Worktree directory + branch component for a task. `TSK-7` is already safe;
/// anything else is folded so a plan can never name a path.
fn worktree_key(task: &str) -> String {
    task.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

/// The dispatch message a coder wakes up to. Its standing instructions come
/// from `roles::system_prompt`; this is the one task, spelled out.
fn coder_prompt(task: &Task, lane: &str) -> String {
    let paths: Vec<String> = task
        .scope
        .iter()
        .map(|s| format!("{}/{}", task.repo, s.trim_start_matches('/')))
        .collect();
    let claim = serde_json::json!({ "task": task.id.as_str(), "paths": paths }).to_string();
    let mut out = format!(
        "Lane `{lane}` — task {} — {}\nrepo: {}\nscope: {}\n\n",
        task.id.as_str(),
        task.title,
        task.repo,
        task.scope.join(", ")
    );
    if !task.acceptance.is_empty() {
        out.push_str("Done means:\n");
        for a in &task.acceptance {
            out.push_str(&format!("- {a}\n"));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "Your first action is the checkout — call `lease.claim` with exactly:\n{claim}\n\n\
         Then do the task inside your working directory, verify it, and end with \
         `board.handoff {{task, capsule}}` — the handoff releases what you hold."
    ));
    out
}

/// Make sure the lease layer knows who this run is before it claims anything.
/// Registration is idempotent: a run the hub already knows keeps its channels.
#[allow(clippy::too_many_arguments)]
async fn bind_run(
    hub: &Arc<LeaseHub>,
    run: &str,
    workspace: &str,
    slug: &str,
    lane: &str,
    repo: &str,
    orch: &Arc<Orchestrator>,
    approver: Option<Arc<dyn Approver>>,
) {
    if hub.holder_of_run(run).await.is_none() {
        hub.register(
            run,
            parzi_core::lease::Holder {
                user: whoami(),
                machine: machine(),
                run: Some(run.to_string()),
                lane: lane.to_string(),
            },
            None,
            // A `critical:` path turns a lease request into an approval card:
            // without the approver the card has nobody to reach (§4).
            approver,
            Some(orch.store().clone()),
        )
        .await;
    }
    hub.bind_project(run, workspace, slug).await;
    if !repo.is_empty() {
        hub.bind_repo(run, repo).await;
    }
}

fn whoami() -> String {
    std::env::var("PARZI_USER")
        .or_else(|_| std::env::var("USERNAME"))
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "me".to_string())
}

fn machine() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "this machine".to_string())
}
