//! The H1 project lifecycle: drafting → planned → running → done | parked.
//!
//! One place that knows the order of the moves a person makes — open the
//! project, ask the header what to build, send a draft to the orchestrator,
//! approve the plan, watch sprint 1's lanes check out their tasks. The shell
//! calls these functions and forwards their results; all writes into the
//! project directory go through `parzi-core` and are journaled.

mod dispatch;
pub mod files;
mod tools;

use std::sync::Arc;

use parzi_core::error::{ParziError, Result};
use parzi_core::journal::Kind;
use parzi_core::plan::{Plan, TaskId};
use parzi_core::project::{self, Project, Status};
use parzi_core::store::{SessionStatus, SessionStore};
use tokio::sync::mpsc;

use crate::handler::RunEvent;
use crate::roles::{Role, RoleCtx};
use crate::tools::Approver;
use crate::Orchestrator;

pub use dispatch::{approve, dispatch_lane, Dispatched};
pub use files::{
    capsule, capsule_ids, drafts, journal, plan, sessions, write_plan, Draft, Sessions,
    SYSTEM_ACTOR,
};
pub use tools::{execute_project_tool, is_project_tool, project_defs, project_defs_for};

/// How long `audit` waits for the orchestrator turn before handing back a
/// timeout. The run keeps going; PLAN.md is written atomically or not at all.
const AUDIT_WAIT_SECS: u64 = 300;

/// A project with its header session ready to be talked to.
#[derive(Debug, Clone)]
pub struct Opened {
    pub project: Project,
    /// Session id of the project's one header run (§1.2).
    pub header_session: String,
    pub status: String,
}

/// A question sent to a role: the session it went to, and the live events.
pub struct Asked {
    pub session_id: String,
    pub events: mpsc::UnboundedReceiver<RunEvent>,
}

/// What an audit turn produced.
#[derive(Debug, Clone)]
pub struct Audited {
    pub session_id: String,
    pub draft: String,
    /// The orchestrator's ≤ 15-line summary, as written by `project.audit`.
    pub summary: String,
    pub plan: Plan,
}

// ------------------------------------------------------------------- open

/// Open a project: load it and make sure its header session exists. Idempotent
/// — a second open returns the same session, because a project has exactly one
/// header (§1.2). The session is created empty; it runs when somebody asks.
pub async fn open(orch: &Arc<Orchestrator>, workspace: &str, slug: &str) -> Result<Opened> {
    let project = project::load(workspace, slug)?;
    let session = ensure_session(orch, &project, Role::Header, "")?;
    Ok(Opened {
        header_session: session,
        status: status(workspace, slug)?,
        project,
    })
}

/// Ask the header or the orchestrator something, in the project's context.
/// The coder is not reachable this way on purpose: it talks to nobody (§1.2).
pub async fn ask(
    orch: &Arc<Orchestrator>,
    workspace: &str,
    slug: &str,
    role: Role,
    text: &str,
    approver: Option<Arc<dyn Approver>>,
) -> Result<Asked> {
    if role == Role::Coder {
        return Err(ParziError::Validation(
            "coders do not take messages: their capsule and their diff are the interface".into(),
        ));
    }
    if text.trim().is_empty() {
        return Err(ParziError::Validation("nothing to ask".into()));
    }
    let project = project::load(workspace, slug)?;
    let session = ensure_session(orch, &project, role, "")?;
    let events = orch
        .send_role(workspace, slug, role, &session, text, approver)
        .await?;
    Ok(Asked {
        session_id: session,
        events,
    })
}

// ------------------------------------------------------------------ audit

/// Send a draft to the orchestrator and wait for the plan it writes. The
/// orchestrator is the only writer of PLAN.md (§15.2); this function never
/// writes it — `project.audit` does, inside the run, through core.
pub async fn audit(
    orch: &Arc<Orchestrator>,
    workspace: &str,
    slug: &str,
    draft: Option<&str>,
    approver: Option<Arc<dyn Approver>>,
) -> Result<Audited> {
    let project = project::load(workspace, slug)?;
    let all = files::drafts(workspace, slug)?;
    let draft = match draft {
        Some(name) => files::draft(workspace, slug, name)?,
        None => all.last().cloned().ok_or_else(|| {
            ParziError::Validation(
                "no draft to audit: ask the header what you are building first".into(),
            )
        })?,
    };
    let session = ensure_session(orch, &project, Role::Orchestrator, "")?;
    let prompt = format!(
        "Audit draft `{}` against PROJECT.md and write the plan.\n\n\
         When it is right, call `project.audit` with the full PLAN.md text and a summary of at \
         most 15 lines. Do not answer with the plan in prose — the tool call is the plan.",
        draft.name
    );
    let events = orch
        .send_role(
            workspace,
            slug,
            Role::Orchestrator,
            &session,
            &prompt,
            approver,
        )
        .await?;
    wait_for(orch.store(), &session, events, AUDIT_WAIT_SECS).await?;

    let plan = files::plan(workspace, slug)?;
    if plan.sprints.is_empty() {
        return Err(ParziError::Store(
            "the orchestrator wrote no plan; read its last turn and try again".into(),
        ));
    }
    Ok(Audited {
        session_id: session,
        draft: draft.name,
        summary: files::read_summary(workspace, slug).unwrap_or_default(),
        plan,
    })
}

// ----------------------------------------------------------------- status

/// STATUS.md without a live lease table: what a project looks like when
/// nothing is running on this machine. Deterministic, no model (§15.7).
pub fn status(workspace: &str, slug: &str) -> Result<String> {
    files::status_with(workspace, slug, &parzi_core::lease::LeaseTable::default())
}

/// STATUS.md with the live leases of this process — what the deck shows.
pub async fn status_live(orch: &Arc<Orchestrator>, workspace: &str, slug: &str) -> Result<String> {
    let table = orch.leases().table();
    let guard = table.lock().await;
    files::status_with(workspace, slug, &guard)
}

/// Re-read the plan and settle the project's status: a project whose every
/// task is done is Done. Called after a handoff; cheap and idempotent.
pub fn refresh(workspace: &str, slug: &str) -> Result<Status> {
    let mut project = project::load(workspace, slug)?;
    let plan = files::plan(workspace, slug)?;
    // A task is done when the plan says so or when its capsule is on disk:
    // PLAN.md has one writer (§15.2), so a finished lane proves itself with
    // the capsule, not by editing the plan behind the orchestrator's back.
    let capsules = files::capsule_ids(workspace, slug);
    let done = |t: &parzi_core::plan::Task| t.done || capsules.iter().any(|c| c == t.id.as_str());
    let mut tasks = plan.tasks().peekable();
    let all_done = tasks.peek().is_some() && plan.tasks().all(done);
    if all_done && project.status == Status::Running {
        project.status = Status::Done;
        project::save(&project)?;
        files::note(
            workspace,
            slug,
            SYSTEM_ACTOR,
            Kind::Note,
            None,
            "every task is done",
        )?;
    }
    Ok(project.status)
}

/// Park a project: nothing is dispatched until somebody approves again.
pub fn park(workspace: &str, slug: &str, who: &str, why: &str) -> Result<Status> {
    let mut project = project::load(workspace, slug)?;
    project.status = Status::Parked;
    project::save(&project)?;
    files::note(workspace, slug, who, Kind::Block, None, why)?;
    Ok(project.status)
}

/// Toggle a task's done status in PLAN.md and log a note.
pub fn toggle_task(workspace: &str, slug: &str, task_id: &str, done: bool) -> Result<Plan> {
    let mut plan = files::plan(workspace, slug)?;
    let mut found = false;
    for sprint in &mut plan.sprints {
        for lane in &mut sprint.lanes {
            for task in &mut lane.tasks {
                if task.id.as_str() == task_id {
                    task.done = done;
                    found = true;
                }
            }
        }
    }
    if found {
        files::write_plan(workspace, slug, &plan)?;
        let who = std::env::var("PARZI_USER")
            .or_else(|_| std::env::var("USERNAME"))
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "me".to_string());
        let _ = files::note(
            workspace,
            slug,
            &who,
            Kind::Note,
            Some(task_id),
            &format!(
                "task {task_id} marked {}",
                if done { "done" } else { "pending" }
            ),
        );
    }
    Ok(plan)
}

// --------------------------------------------------------------- sessions

/// The session a role runs in, created empty on first use and remembered in
/// `sessions.toml`. A session the store no longer has is replaced, not
/// resurrected: the transcript is gone, the project is not.
fn ensure_session(
    orch: &Arc<Orchestrator>,
    project: &Project,
    role: Role,
    lane: &str,
) -> Result<String> {
    let recorded = files::sessions(&project.workspace, &project.slug);
    let existing = match role {
        Role::Header => recorded.header.clone(),
        Role::Orchestrator => recorded.orchestrator.clone(),
        Role::Coder => recorded.lanes.get(lane).cloned(),
    };
    if let Some(id) = existing {
        if orch.store().get(&id).is_ok() {
            return Ok(id);
        }
    }
    let lane_name = if lane.is_empty() {
        role.lane_name()
    } else {
        lane
    };
    let cwd = project::dir(&project.workspace, &project.slug)
        .display()
        .to_string();
    let meta = orch.role_session(
        &project.workspace,
        &project.slug,
        role,
        lane_name,
        None,
        &cwd,
    )?;
    files::set_session(&project.workspace, &project.slug, role, lane_name, &meta.id)?;
    Ok(meta.id)
}

/// The context a role run is briefed with. Public so the shell can show a
/// person exactly what a role sees before it spends a token.
#[must_use]
pub fn role_context(
    workspace: &str,
    slug: &str,
    role: Role,
    lane: &str,
    task: Option<&str>,
) -> Vec<crate::roles::ContextPart> {
    let mut ctx = RoleCtx::new(workspace, slug).lane(lane);
    if let Some(t) = task {
        ctx = ctx.task(t);
    }
    crate::roles::context_for(role, &ctx)
}

/// Drain a run's live events and then wait for its session to settle. The
/// channel closes early for a queued run, so the status poll is the backstop.
pub async fn wait_for(
    store: &SessionStore,
    session_id: &str,
    mut events: mpsc::UnboundedReceiver<RunEvent>,
    secs: u64,
) -> Result<SessionStatus> {
    while let Some(ev) = events.recv().await {
        if matches!(ev, RunEvent::Done { .. } | RunEvent::Error(_)) {
            break;
        }
    }
    for _ in 0..(secs * 4) {
        let status = store.get(session_id)?.status;
        if matches!(
            status,
            SessionStatus::Done | SessionStatus::Idle | SessionStatus::Killed
        ) {
            return Ok(status);
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    Err(ParziError::Store(format!(
        "run {session_id} is still going after {secs}s"
    )))
}

/// The task id a lane's coder session is on, from the plan. Used by the deck
/// and by the tests; no model, no guessing.
#[must_use]
pub fn lane_task(plan: &Plan, lane: &str) -> Option<TaskId> {
    plan.sprints
        .iter()
        .flat_map(|s| &s.lanes)
        .find(|l| l.name == lane)
        .and_then(|l| l.tasks.iter().find(|t| !t.done))
        .map(|t| t.id.clone())
}
