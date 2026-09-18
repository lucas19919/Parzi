//! H1 end to end on one machine, with a scripted agent: the header drafts
//! a rough plan, the orchestrator audits it into PLAN.md, a person approves,
//! and sprint 1's two lanes check their tasks out with file leases and end
//! with capsules. No network, no real model, no real repo.

mod common;

use std::sync::Arc;

use common::{home, orch_with, script, Agent, Fake};
use parzi_core::config::ParziConfig;
use parzi_core::error::{ParziError, Result};
use parzi_core::project::{self, Project, Roster, Status};
use parzi_core::store::SessionStatus;
use parzi_core::workspace::{self, Kind, RepoRef, Workspace};
use parzi_providers::TurnEnd;
use parzi_runtime::project_flow;
use parzi_runtime::roles::Role;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};
use parzi_runtime::Orchestrator;
use serde_json::json;
use tokio::sync::Semaphore;

const WORKSPACE: &str = "acme";
const SLUG: &str = "checkout";

/// The plan the scripted orchestrator writes: two lanes, non-overlapping
/// scopes, one task each, both startable at once.
const PLAN_TEXT: &str = "parzi: 1\n\
# Plan: Checkout\n\
\n\
## Sprint 1 — first cut (target: 1 day)\n\
### lane api\n\
- [ ] TSK-1  Session endpoint [repo:app] [scope:src/api/**]\n\
  - acceptance: `cargo test -p app api` passes\n\
### lane web\n\
- [ ] TSK-2  Checkout page [repo:app] [scope:src/web/**]\n\
  - acceptance: `npm test` passes\n";

/// Both coders park here after their claim, so the test can look at the live
/// lease table while two lanes really hold files. The test hands out the
/// permits; a semaphore (not a notify) so the order of the two never matters.
fn gate() -> &'static Semaphore {
    static GATE: std::sync::OnceLock<Semaphore> = std::sync::OnceLock::new();
    GATE.get_or_init(|| Semaphore::new(0))
}

/// One agent for all three roles. It reads which role it is out of its
/// standing instructions (the role binding writes them there) and does the
/// role's whole job in one turn, calling Parzi's tools over MCP the way a
/// vendor agent does.
fn scripted() -> Arc<Fake> {
    Fake::new(
        "claude",
        script(|a: Agent| async move {
            let sys = a.spec.instructions.clone().unwrap_or_default();
            if sys.contains("You are the header of project") {
                a.parzi(
                    "project.draft_plan",
                    json!({
                        "title": "Checkout, roughly",
                        "body": "- an API for the checkout session\n- a page to run it from",
                    }),
                )
                .await;
                a.say("Saved the rough plan; send it to the orchestrator.");
                return Ok(TurnEnd::Completed);
            }
            if sys.contains("You are the orchestrator of project") {
                a.parzi(
                    "project.audit",
                    json!({
                        "plan": PLAN_TEXT,
                        "summary": "Two lanes, one sprint.\napi owns src/api, web owns src/web.\nNo overlap, so both start now.",
                    }),
                )
                .await;
                a.say("Plan written.");
                return Ok(TurnEnd::Completed);
            }
            // Coder: claim first, then park at the gate, then hand off.
            let (task, scope) = if a.spec.prompt.contains("task TSK-1 ") {
                ("TSK-1", "api")
            } else {
                ("TSK-2", "web")
            };
            a.parzi(
                "lease.claim",
                json!({"task": task, "paths": [format!("app/src/{scope}/**")]}),
            )
            .await;
            drop(gate().acquire().await);
            a.parzi(
                "board.handoff",
                json!({
                    "task": task,
                    "capsule": {
                        "task": task,
                        "summary": format!("{scope} lane done"),
                        "touched_files": [format!("src/{scope}/mod.rs")],
                        "exported_symbols": [format!("{scope}::run")],
                        "verification": "cargo test passed (0 failed)",
                        "invariants": ["scope is not shared"],
                        "gotchas": [],
                    },
                }),
            )
            .await;
            a.say("done");
            Ok(TurnEnd::Completed)
        }),
    )
}

/// Ask mode is the default; the flow's tools are approved for this test the
/// way a person clicking "allow" would.
struct AllowAll;
#[async_trait::async_trait]
impl Approver for AllowAll {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

fn approver() -> Option<Arc<dyn Approver>> {
    Some(Arc::new(AllowAll))
}

fn fake_repo(home: &std::path::Path) -> std::path::PathBuf {
    let repo = home.join("repos").join("app");
    for (rel, body) in [
        ("README.md", "# app\n\nThe test repo.\n"),
        ("src/api/mod.rs", "pub fn run() {}\n"),
        ("src/web/mod.rs", "pub fn run() {}\n"),
    ] {
        let path = repo.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    repo
}

fn seed() -> Arc<Orchestrator> {
    let repo = fake_repo(&home("flow"));
    workspace::create(Workspace {
        name: WORKSPACE.into(),
        kind: Kind::Solo,
        repos: vec![RepoRef {
            name: "app".into(),
            remote: String::new(),
            default_branch: "main".into(),
            local_path: Some(repo),
        }],
        members: vec![],
        policy: Default::default(),
        defaults: Default::default(),
    })
    .unwrap();
    project::save(&Project {
        slug: SLUG.into(),
        title: "Checkout".into(),
        workspace: WORKSPACE.into(),
        repos: vec!["app".into()],
        roster: Roster {
            header: "claude/head".into(),
            orchestrator: "claude/orch".into(),
            coder: "claude/coder".into(),
        },
        status: Status::Drafting,
        why: "People cannot pay.".into(),
        ..Project::default()
    })
    .unwrap();

    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = 4;
    orch_with(cfg, &[scripted()]).0
}

async fn settle(orch: &Arc<Orchestrator>, asked: project_flow::Asked) -> Result<SessionStatus> {
    project_flow::wait_for(orch.store(), &asked.session_id, asked.events, 30).await
}

#[tokio::test]
async fn draft_audit_approve_runs_two_lanes_with_leases_and_capsules() {
    let orch = seed();

    // 1. Open: the header session exists and costs nothing to make.
    let opened = project_flow::open(&orch, WORKSPACE, SLUG).await.unwrap();
    assert_eq!(opened.project.slug, SLUG);
    assert!(opened.status.contains("Status — Checkout"));
    let again = project_flow::open(&orch, WORKSPACE, SLUG).await.unwrap();
    assert_eq!(again.header_session, opened.header_session);

    // 2. "What are we building?" — the header writes a rough plan.
    let asked = project_flow::ask(
        &orch,
        WORKSPACE,
        SLUG,
        Role::Header,
        "what are we building?",
        approver(),
    )
    .await
    .unwrap();
    assert_eq!(asked.session_id, opened.header_session);
    settle(&orch, asked).await.unwrap();
    let drafts = project_flow::drafts(WORKSPACE, SLUG).unwrap();
    assert_eq!(drafts.len(), 1, "the header saved one draft");
    assert_eq!(drafts[0].name, "1.md");
    assert!(drafts[0].body.contains("checkout session"));

    // A coder is not reachable by asking.
    assert!(matches!(
        project_flow::ask(&orch, WORKSPACE, SLUG, Role::Coder, "hi", approver()).await,
        Err(ParziError::Validation(_))
    ));

    // 3. Audit: the orchestrator turns the draft into PLAN.md.
    let audited = project_flow::audit(&orch, WORKSPACE, SLUG, None, approver())
        .await
        .unwrap();
    assert_eq!(audited.draft, "1.md");
    assert_eq!(audited.plan.sprints.len(), 1);
    assert_eq!(audited.plan.sprints[0].lanes.len(), 2, "two lanes");
    assert!(audited.summary.lines().count() <= 15);
    assert!(audited.summary.contains("Two lanes"));
    // PLAN.md on disk is what the parser accepted, and the project is planned.
    let plan = project_flow::plan(WORKSPACE, SLUG).unwrap();
    assert_eq!(plan, audited.plan);
    assert_eq!(
        project::load(WORKSPACE, SLUG).unwrap().status,
        Status::Planned
    );

    // 4. Approve: one coder run per lane of sprint 1.
    let dispatched = project_flow::approve(&orch, WORKSPACE, SLUG, "ada", approver())
        .await
        .unwrap();
    assert_eq!(dispatched.len(), 2);
    let lanes: Vec<&str> = dispatched.iter().map(|d| d.lane.as_str()).collect();
    assert!(lanes.contains(&"api") && lanes.contains(&"web"));
    assert_eq!(
        project::load(WORKSPACE, SLUG).unwrap().status,
        Status::Running
    );

    // 5. Both lanes hold their files before either hands off.
    let held = wait_for_leases(&orch, 2).await;
    assert_eq!(held.len(), 2, "both lanes hold a lease: {held:?}");
    let tasks: Vec<String> = held.iter().map(|l| l.task.0.clone()).collect();
    assert!(tasks.contains(&"TSK-1".to_string()));
    assert!(tasks.contains(&"TSK-2".to_string()));
    assert!(held.iter().any(|l| l.holder.lane == "api"));
    assert!(held.iter().any(|l| l.holder.lane == "web"));

    // Let both coders hand off.
    gate().add_permits(2);
    for d in &dispatched {
        wait_for_session(&orch, &d.session_id).await;
    }

    // 6. The capsules landed and the status renders both lanes.
    let ids = project_flow::capsule_ids(WORKSPACE, SLUG);
    assert_eq!(ids, vec!["TSK-1".to_string(), "TSK-2".to_string()]);
    let capsule = project_flow::capsule(WORKSPACE, SLUG, "TSK-1").unwrap();
    assert!(!capsule.verification.is_empty());
    let status = project_flow::status(WORKSPACE, SLUG).unwrap();
    assert!(
        status.contains("TSK-1") && status.contains("TSK-2"),
        "{status}"
    );
    // Every task has its capsule, so the project has landed.
    assert_eq!(
        project_flow::refresh(WORKSPACE, SLUG).unwrap(),
        Status::Done
    );

    // 7. The journal is the record: approval, both claims, both handoffs.
    let journal = project_flow::journal(WORKSPACE, SLUG, 100);
    let kinds: Vec<&str> = journal.iter().map(|l| l.kind.as_str()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "approve").count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == "claim").count(), 2);
    assert_eq!(kinds.iter().filter(|k| **k == "handoff").count(), 2);
    assert!(kinds.contains(&"plan_changed"));
}

/// Poll the live lease table until `n` leases are held, or the runs are gone.
async fn wait_for_leases(orch: &Arc<Orchestrator>, n: usize) -> Vec<parzi_core::lease::Lease> {
    let hub = orch.leases();
    for _ in 0..200 {
        let held = hub.snapshot().await;
        if held.len() >= n {
            return held;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    hub.snapshot().await
}

async fn wait_for_session(orch: &Arc<Orchestrator>, id: &str) {
    for _ in 0..200 {
        let status = orch.store().get(id).unwrap().status;
        if matches!(
            status,
            SessionStatus::Done | SessionStatus::Idle | SessionStatus::Killed
        ) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("run {id} never settled");
}

#[test]
fn the_role_tools_are_the_ones_the_flow_names() {
    let names: Vec<String> = project_flow::project_defs()
        .into_iter()
        .map(|d| d.name)
        .collect();
    for t in ["project.draft_plan", "project.audit", "project.status"] {
        assert!(names.contains(&t.to_string()), "missing def {t}");
        assert!(project_flow::is_project_tool(t));
    }
    // A person approves; no role is offered the tool.
    assert!(project_flow::is_project_tool("project.approve"));
    assert!(!names.contains(&"project.approve".to_string()));
    let header: Vec<String> = project_flow::project_defs_for(Role::Header)
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(
        header,
        vec![
            "project.draft_plan".to_string(),
            "project.status".to_string()
        ]
    );
    let coder = project_flow::project_defs_for(Role::Coder);
    assert!(coder.is_empty(), "a coder never touches the project file");
}
