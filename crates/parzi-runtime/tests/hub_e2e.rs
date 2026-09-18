//! Round 1, end to end, on one machine: a workspace with a real git repo, a
//! project, the header's rough plan, the orchestrator's audit, a human's
//! approval, and then the part that only shows up when two lanes really run —
//! a forced collision on one file, a request the holder grants, and a second
//! request for a `critical:` path that only a person can answer.
//!
//! `project_flow.rs` proves the happy path; this proves §4. Everything runs
//! through the real tool surface with a scripted agent that works the way a
//! vendor's does: its own edits ask Parzi's gate, Parzi's tools go over MCP.
//! No network, no model, no repo outside the temp dir.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{home, orch_with, script, Agent, Fake};
use parzi_core::config::ParziConfig;
use parzi_core::project::{self, Project, Roster, Status};
use parzi_core::store::{Event, SessionStatus, SessionStore};
use parzi_core::workspace::{self, RepoRef, Workspace};
use parzi_providers::{PermissionDecision, TurnEnd};
use parzi_runtime::inter;
use parzi_runtime::project_flow;
use parzi_runtime::roles::Role;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};
use parzi_runtime::Orchestrator;
use serde_json::json;
use tokio::sync::Semaphore;

const WORKSPACE: &str = "acme-e2e";
const SLUG: &str = "checkout";
/// The file both lanes want. `api` claims it; `web` has to ask.
const CONTESTED: &str = "src/routes.rs";
/// Under `critical:` in PROJECT.md, so its transfer is a human's decision.
const GUARDED: &str = "src/payments/intent.rs";

/// Two lanes, no overlapping scope — the collision comes from the work, not
/// from a sloppy plan, exactly as PLAN §4 describes it.
const PLAN_TEXT: &str = "parzi: 1\n\
# Plan: Checkout\n\
\n\
## Sprint 1 — first cut (target: 1 day)\n\
### lane api\n\
- [ ] TSK-1  Checkout session endpoint [repo:app] [scope:src/api/**,src/routes.rs,src/payments/intent.rs]\n\
  - acceptance: `cargo test -p app api` passes\n\
### lane web\n\
- [ ] TSK-2  Checkout page [repo:app] [scope:src/web/**]\n\
  - acceptance: the page posts to the endpoint\n";

// ---------------------------------------------------------------- scheduling

/// `web` waits here until `api` really holds the contested file, so the
/// collision is a collision and not a race the test happened to win.
fn web_start() -> &'static Semaphore {
    static G: std::sync::OnceLock<Semaphore> = std::sync::OnceLock::new();
    G.get_or_init(|| Semaphore::new(0))
}

/// `api`'s first turn holds its files here — a real coder would be working —
/// until the test has seen the request land in its transcript. The request
/// is then `api`'s next turn.
fn api_idle() -> &'static Semaphore {
    static G: std::sync::OnceLock<Semaphore> = std::sync::OnceLock::new();
    G.get_or_init(|| Semaphore::new(0))
}

/// `api` waits here before its handoff: a handoff releases everything it
/// holds, and the test still needs it to hold the guarded file.
fn api_handoff() -> &'static Semaphore {
    static G: std::sync::OnceLock<Semaphore> = std::sync::OnceLock::new();
    G.get_or_init(|| Semaphore::new(0))
}

/// `api` answers one request; any later turn only takes note.
static API_ANSWERED: AtomicBool = AtomicBool::new(false);

// --------------------------------------------------------------------- agent

/// Every `req-N` in `t`. A holder learns the id the same way a model would:
/// from the inter-session message the hub delivered into its context.
fn request_ids(t: &str) -> Vec<String> {
    let bytes = t.as_bytes();
    let mut out: Vec<String> = vec![];
    let mut i = 0;
    while let Some(p) = t[i..].find("req-") {
        let start = i + p;
        let mut end = start + 4;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end > start + 4 {
            let id = t[start..end].to_string();
            if !out.contains(&id) {
                out.push(id);
            }
        }
        i = start + 4;
    }
    out
}

fn capsule(task: &str, lane: &str) -> serde_json::Value {
    json!({
        "task": task,
        "capsule": {
            "task": task,
            "summary": format!("{lane} lane landed {task}"),
            "touched_files": [CONTESTED],
            "exported_symbols": [format!("{lane}::run")],
            "verification": "cargo test -p app (0 failed)",
            "invariants": ["the route table stays alphabetical"],
            "gotchas": [],
        },
    })
}

/// One agent for all three roles. It reads which role and which lane it is
/// out of its standing instructions (the role binding puts them there) and
/// what to do out of its prompt — the same two things a real agent has.
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
            } else if sys.contains("You are the orchestrator of project") {
                a.parzi(
                    "project.audit",
                    json!({
                        "plan": PLAN_TEXT,
                        "summary": "Two lanes, one sprint.\n\
                                    api owns the endpoint and the route table.\n\
                                    web owns the page and will have to ask for routes.rs.",
                    }),
                )
                .await;
                a.say("Plan written.");
            } else if sys.contains("Your lane is `api`") {
                api_turn(&a).await;
            } else {
                web_turn(&a).await;
            }
            Ok(TurnEnd::Completed)
        }),
    )
}

/// Lane `api`: check the task out and hold it; the request that reaches it
/// is its next turn, and it grants it — then it hands off, but only once the
/// test says the guarded file has settled.
async fn api_turn(a: &Agent) {
    if a.spec.prompt.contains("task TSK-1 ") {
        a.parzi(
            "lease.claim",
            json!({
                "task": "TSK-1",
                "paths": ["app/src/api/**", format!("app/{CONTESTED}"), format!("app/{GUARDED}")],
            }),
        )
        .await;
        drop(api_idle().acquire().await);
        a.say("holding the route table");
        return;
    }
    let Some(id) = request_ids(&a.spec.prompt).into_iter().next() else {
        a.say("noted");
        return;
    };
    if API_ANSWERED.swap(true, Ordering::SeqCst) {
        a.say("noted");
        return;
    }
    a.parzi("lease.grant", json!({ "request_id": id })).await;
    drop(api_handoff().acquire().await);
    a.parzi("board.handoff", capsule("TSK-1", "api")).await;
    a.say("api done");
}

/// Lane `web`: claim, walk into the collision, ask for the file, write it,
/// then ask for the guarded one — which is not the holder's to give.
async fn web_turn(a: &Agent) {
    drop(web_start().acquire().await);
    a.parzi(
        "lease.claim",
        json!({ "task": "TSK-2", "paths": ["app/src/web/**"] }),
    )
    .await;
    // The collision: routes.rs belongs to lane api right now.
    write_contested(a, "w-write-1", "// web was here\n").await;
    a.parzi(
        "lease.request",
        json!({
            "path": CONTESTED,
            "for_task": "TSK-2",
            "reason": "the checkout route has to be registered somewhere",
        }),
    )
    .await;
    write_contested(a, "w-write-2", "// checkout route\n").await;
    a.parzi(
        "lease.request",
        json!({
            "path": GUARDED,
            "for_task": "TSK-2",
            "reason": "the form needs the payment intent shape",
        }),
    )
    .await;
    a.parzi("board.handoff", capsule("TSK-2", "web")).await;
    a.say("web done");
}

/// The agent's own write, the way Claude Code makes one: announce the tool,
/// ask Parzi's gate with the absolute path, write only when allowed.
async fn write_contested(a: &Agent, id: &str, body: &str) {
    let file = a.spec.cwd.join(CONTESTED);
    let input = json!({ "file_path": file, "content": body });
    a.own_tool(id, "Write", input.clone(), || async {
        match a.ask("Write", input.clone(), &[&file.display().to_string()]).await {
            PermissionDecision::Deny(why) => (false, why),
            PermissionDecision::Allow | PermissionDecision::AllowAlways => {
                match std::fs::write(&file, body) {
                    Ok(()) => (true, format!("wrote {}", file.display())),
                    Err(e) => (false, e.to_string()),
                }
            }
        }
    })
    .await;
}

// -------------------------------------------------------------------- humans

/// The person at the machine: allows everything and keeps every card, so the
/// test can prove the `critical:` transfer was actually shown to somebody.
struct Watching {
    seen: Arc<Mutex<Vec<ToolCallInfo>>>,
}

#[async_trait::async_trait]
impl Approver for Watching {
    async fn approve(&self, call: &ToolCallInfo) -> Approval {
        self.seen.lock().unwrap().push(call.clone());
        Approval::Allow
    }
}

// --------------------------------------------------------------------- world

fn git(repo: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A real, tiny git repo — the lanes get worktrees of it, which is what makes
/// the lease and not the filesystem the thing that keeps them apart.
fn tiny_repo(home: &std::path::Path) -> std::path::PathBuf {
    let repo = home.join("repos").join("app");
    for (rel, body) in [
        ("README.md", "# app\n"),
        ("src/routes.rs", "// routes\n"),
        ("src/api/mod.rs", "pub fn run() {}\n"),
        ("src/web/mod.rs", "pub fn run() {}\n"),
        ("src/payments/intent.rs", "pub struct Intent;\n"),
    ] {
        let path = repo.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@parzi.local"]);
    git(&repo, &["config", "user.name", "parzi test"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "first"]);
    repo
}

fn seed() -> Arc<Orchestrator> {
    let repo = tiny_repo(&home("e2e"));
    workspace::create(Workspace {
        repos: vec![RepoRef {
            name: "app".into(),
            remote: "git@example.com:acme/app.git".into(),
            default_branch: "main".into(),
            local_path: Some(repo),
        }],
        ..Workspace::solo(WORKSPACE)
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
        critical: vec!["app/src/payments/**".to_string()],
        why: "People cannot pay.".into(),
        ..Project::default()
    })
    .unwrap();

    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = 4;
    orch_with(cfg, &[scripted()]).0
}

/// Poll until `f` says yes, or give up after `secs` — every wait in this test
/// is on a real condition, never on a fixed sleep.
async fn until<F, Fut>(secs: u64, what: &str, f: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn settled(orch: &Arc<Orchestrator>, id: &str) -> bool {
    matches!(
        orch.store().get(id).map(|m| m.status),
        Ok(SessionStatus::Done | SessionStatus::Idle | SessionStatus::Killed)
    )
}

/// Did this run's transcript record a tool result matching `f`?
fn result_in(store: &SessionStore, run: &str, tool: &str, f: impl Fn(bool, &str) -> bool) -> bool {
    store.events(run).unwrap_or_default().into_iter().any(
        |e| matches!(e, Event::ToolResult { name, ok, output, .. } if name == tool && f(ok, &output)),
    )
}

// ---------------------------------------------------------------------- test

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_project_goes_from_a_question_to_two_lanes_trading_one_file() {
    let t0 = Instant::now();
    let orch = seed();
    let cards = Arc::new(Mutex::new(vec![]));
    let human: Option<Arc<dyn Approver>> = Some(Arc::new(Watching {
        seen: cards.clone(),
    }));
    let seeded = t0.elapsed();

    // 1. Open the project: the header session exists, nothing has been spent.
    let t = Instant::now();
    let opened = project_flow::open(&orch, WORKSPACE, SLUG).await.unwrap();
    assert!(opened.status.contains("Status — Checkout"));
    let opening = t.elapsed();

    // 2. "What are we building?" — the header writes a rough plan into drafts/.
    let t = Instant::now();
    let asked = project_flow::ask(
        &orch,
        WORKSPACE,
        SLUG,
        Role::Header,
        "what are we building?",
        human.clone(),
    )
    .await
    .unwrap();
    project_flow::wait_for(orch.store(), &asked.session_id, asked.events, 30)
        .await
        .unwrap();
    let drafts = project_flow::drafts(WORKSPACE, SLUG).unwrap();
    assert_eq!(drafts.len(), 1, "one rough plan in the dock");
    let drafting = t.elapsed();

    // 3. Send it to the orchestrator: PLAN.md and a summary a person can read.
    let t = Instant::now();
    let audited = project_flow::audit(&orch, WORKSPACE, SLUG, None, human.clone())
        .await
        .unwrap();
    assert_eq!(audited.plan.sprints[0].lanes.len(), 2, "two lanes");
    assert!(
        audited.summary.lines().count() <= 15,
        "a summary, not a wall"
    );
    assert_eq!(
        project::load(WORKSPACE, SLUG).unwrap().status,
        Status::Planned
    );
    let auditing = t.elapsed();

    // 4. Approve: one coder run per lane, each in its own worktree.
    let t = Instant::now();
    let dispatched = project_flow::approve(&orch, WORKSPACE, SLUG, "ada", human.clone())
        .await
        .unwrap();
    assert_eq!(dispatched.len(), 2);
    let api = dispatched.iter().find(|d| d.lane == "api").unwrap().clone();
    let web = dispatched.iter().find(|d| d.lane == "web").unwrap().clone();
    assert_ne!(api.cwd, web.cwd, "two lanes, two worktrees");
    assert!(
        std::path::Path::new(&api.cwd).join(CONTESTED).exists(),
        "the worktree is a real checkout of the repo"
    );
    let approving = t.elapsed();

    // 5. The collision. `api` checks its files out first; only then is `web`
    //    allowed to walk into one of them — and its agent's own write is
    //    refused at Parzi's gate, naming the holder.
    let t = Instant::now();
    let hub = orch.leases();
    let key = format!("app/{CONTESTED}");
    until(30, "api to hold the route table", || {
        let (hub, key) = (hub.clone(), key.clone());
        async move {
            hub.holder_of_path(&key)
                .await
                .is_some_and(|l| l.holder.lane == "api")
        }
    })
    .await;
    web_start().add_permits(1);

    let store = orch.store().clone();
    until(30, "web to be refused the file api holds", || {
        let (store, run) = (store.clone(), web.session_id.clone());
        async move {
            result_in(&store, &run, "Write", |ok, out| {
                !ok && out.contains("lane api") && out.contains("TSK-1")
            })
        }
    })
    .await;
    let colliding = t.elapsed();

    // 6. The ask. `web` requests the file; the request lands in `api`'s
    //    transcript as an inter-session message, becomes `api`'s next turn,
    //    `api` grants it, and the file is web's.
    let t = Instant::now();
    until(30, "the request to reach the holder's transcript", || {
        let (store, run) = (store.clone(), api.session_id.clone());
        async move {
            inter::inbox(&store, &run).is_ok_and(|m| {
                m.iter().any(|m| m.kind == inter::InterKind::LeaseRequest)
            })
        }
    })
    .await;
    api_idle().add_permits(1);
    until(60, "the route table to change hands", || {
        let (store, run) = (store.clone(), web.session_id.clone());
        async move {
            result_in(&store, &run, "lease.request", |ok, out| {
                ok && out.contains("granted") && out.contains(CONTESTED)
            })
        }
    })
    .await;
    until(30, "web to write the file it was given", || {
        let (store, run) = (store.clone(), web.session_id.clone());
        async move { result_in(&store, &run, "Write", |ok, _| ok) }
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&web.cwd).join(CONTESTED)).unwrap(),
        "// checkout route\n",
        "the write landed in web's own worktree"
    );
    let granting = t.elapsed();

    // 7. The guarded file: a `critical:` transfer is a person's decision, not
    //    the holder's, and it arrives as an ordinary approval card.
    let t = Instant::now();
    until(60, "the guarded file to be handed over", || {
        let (store, run) = (store.clone(), web.session_id.clone());
        async move {
            result_in(&store, &run, "lease.request", |ok, out| {
                ok && out.contains("granted") && out.contains(GUARDED)
            })
        }
    })
    .await;
    assert_eq!(
        store
            .events(&api.session_id)
            .unwrap()
            .into_iter()
            .filter(|e| matches!(e, Event::ToolResult { name, .. } if name == "lease.grant"))
            .count(),
        1,
        "the holder answered once; the guarded file was never its call"
    );
    let transfers: Vec<ToolCallInfo> = cards
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.name == "lease.transfer")
        .cloned()
        .collect();
    assert_eq!(transfers.len(), 1, "exactly one card, for the guarded file");
    assert_eq!(transfers[0].args["critical"], json!(true));
    assert!(
        transfers[0].args["path"]
            .as_str()
            .unwrap_or_default()
            .contains("payments"),
        "the card names the file: {:?}",
        transfers[0].args
    );
    assert_eq!(transfers[0].args["from_lane"], json!("api"));
    let escalating = t.elapsed();

    // 8. Both lanes end with a capsule; `api` only lets go once the guarded
    //    file has already moved, so nothing above was a lucky release.
    let t = Instant::now();
    api_handoff().add_permits(1);
    for d in [&api, &web] {
        let id = d.session_id.clone();
        until(60, "the lane to finish", || {
            let (orch, id) = (orch.clone(), id.clone());
            async move { settled(&orch, &id).await }
        })
        .await;
    }
    assert_eq!(
        project_flow::capsule_ids(WORKSPACE, SLUG),
        vec!["TSK-1".to_string(), "TSK-2".to_string()]
    );
    assert!(!project_flow::capsule(WORKSPACE, SLUG, "TSK-2")
        .unwrap()
        .verification
        .is_empty());
    assert!(
        hub.snapshot().await.is_empty(),
        "a finished run holds nothing"
    );
    let finishing = t.elapsed();

    // 9. What a person sees afterwards: STATUS.md and the journal, both
    //    rendered from state with no model in the loop.
    let status = project_flow::status(WORKSPACE, SLUG).unwrap();
    for needle in ["TSK-1", "TSK-2", "api", "web"] {
        assert!(
            status.contains(needle),
            "STATUS.md is missing {needle}:\n{status}"
        );
    }
    let journal = project_flow::journal(WORKSPACE, SLUG, 200);
    let kinds: Vec<&str> = journal.iter().map(|l| l.kind.as_str()).collect();
    let count = |k: &str| kinds.iter().filter(|x| **x == k).count();
    assert_eq!(count("claim"), 2, "one checkout per lane");
    assert_eq!(count("request"), 2, "the route table and the guarded file");
    assert_eq!(count("grant"), 1, "the holder answered exactly once");
    assert_eq!(count("handoff"), 2);
    assert_eq!(count("release"), 2, "a handoff lets go of what it held");
    assert_eq!(count("approve"), 2, "the plan and the transfer: {kinds:?}");
    assert!(
        journal
            .iter()
            .any(|l| l.kind.as_str() == "approve" && l.text.contains(GUARDED)),
        "the human's decision on the guarded file is in the record"
    );
    assert_eq!(
        project_flow::refresh(WORKSPACE, SLUG).unwrap(),
        Status::Done
    );

    eprintln!(
        "\n  hub e2e timings\n  \
         seed+git   {seeded:>10.1?}\n  \
         open       {opening:>10.1?}\n  \
         header     {drafting:>10.1?}\n  \
         audit      {auditing:>10.1?}\n  \
         approve    {approving:>10.1?}\n  \
         collision  {colliding:>10.1?}\n  \
         grant      {granting:>10.1?}\n  \
         critical   {escalating:>10.1?}\n  \
         handoffs   {finishing:>10.1?}\n  \
         total      {:>10.1?}\n",
        t0.elapsed(),
    );
}
