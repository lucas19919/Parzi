//! Round 1, end to end, on one machine: a workspace with a real git repo, a
//! project, the header's rough plan, the orchestrator's audit, a human's
//! approval, and then the part that only shows up when two lanes really run —
//! a forced collision on one file, a request the holder grants, and a second
//! request for a `critical:` path that only a person can answer.
//!
//! `project_flow.rs` proves the happy path; this proves §4. Everything runs
//! through the real tool surface with a scripted provider: no network, no
//! model, no repo outside the temp dir.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use parzi_core::config::ParziConfig;
use parzi_core::error::Result;
use parzi_core::project::{self, Project, Roster, Status};
use parzi_core::store::{Event, SessionStatus, SessionStore};
use parzi_core::workspace::{self, RepoRef, Workspace};
use parzi_providers::{AuthStatus, ChatReq, EventRx, Model, Provider, StreamEvent};
use parzi_runtime::project_flow;
use parzi_runtime::roles::Role;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};
use parzi_runtime::Orchestrator;
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

/// `api` waits here while it holds its files and nobody has asked for one —
/// a real coder would be working; this one would otherwise spin its whole
/// step budget in a few milliseconds and finish before web ever collided.
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

/// How many turns `api` spent before the request showed up in its context.
fn api_polls() -> &'static AtomicUsize {
    static P: std::sync::OnceLock<AtomicUsize> = std::sync::OnceLock::new();
    P.get_or_init(|| AtomicUsize::new(0))
}

// ------------------------------------------------------------------ provider

/// One scripted provider for all three roles. It reads which role and which
/// task it is out of the system prompt (the role binding puts them there) and
/// what it has already done out of its own transcript — the same two things a
/// real model has.
struct ScriptProvider;

fn transcript(req: &ChatReq) -> String {
    req.messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn calls(t: &str, tool: &str) -> usize {
    t.matches(&format!("[tool:{tool}")).count()
}

/// Every `req-N` the lane can see. A holder learns the id the same way a model
/// would: from the inter-session message the hub delivered into its context.
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

fn call(id: &str, name: &str, args: serde_json::Value) -> StreamEvent {
    StreamEvent::ToolCall {
        id: id.into(),
        name: name.into(),
        args,
    }
}

fn capsule(task: &str, lane: &str) -> serde_json::Value {
    serde_json::json!({
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

#[async_trait::async_trait]
impl Provider for ScriptProvider {
    fn id(&self) -> &'static str {
        "script"
    }
    async fn models(&self) -> Result<Vec<Model>> {
        Ok(vec![])
    }
    async fn chat_stream(&self, req: ChatReq) -> Result<EventRx> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let t = transcript(&req);
        let sys = req.system.clone();

        if sys.contains("You are the header of project") {
            let ev = if calls(&t, "project.draft_plan") == 0 {
                call(
                    "h1",
                    "project.draft_plan",
                    serde_json::json!({
                        "title": "Checkout, roughly",
                        "body": "- an API for the checkout session\n- a page to run it from",
                    }),
                )
            } else {
                StreamEvent::Text("Saved the rough plan; send it to the orchestrator.".into())
            };
            drop(tx.send(Ok(ev)));
            return Ok(rx);
        }

        if sys.contains("You are the orchestrator of project") {
            let ev = if calls(&t, "project.audit") == 0 {
                call(
                    "o1",
                    "project.audit",
                    serde_json::json!({
                        "plan": PLAN_TEXT,
                        "summary": "Two lanes, one sprint.\n\
                                    api owns the endpoint and the route table.\n\
                                    web owns the page and will have to ask for routes.rs.",
                    }),
                )
            } else {
                StreamEvent::Text("Plan written.".into())
            };
            drop(tx.send(Ok(ev)));
            return Ok(rx);
        }

        let ev = if sys.contains("`TSK-1`") {
            api_turn(&t).await
        } else {
            web_turn(&t).await
        };
        drop(tx.send(Ok(ev)));
        Ok(rx)
    }
    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Ok
    }
}

/// Lane `api`: check the task out, answer the one request that reaches it,
/// then hand off — but only once the test says the guarded file has settled.
async fn api_turn(t: &str) -> StreamEvent {
    if calls(t, "lease.claim") == 0 {
        return call(
            "a1",
            "lease.claim",
            serde_json::json!({
                "task": "TSK-1",
                "paths": ["app/src/api/**", format!("app/{CONTESTED}"), format!("app/{GUARDED}")],
            }),
        );
    }
    if calls(t, "lease.grant") == 0 {
        if let Some(id) = request_ids(t).first() {
            return call("a2", "lease.grant", serde_json::json!({ "request_id": id }));
        }
        // Nothing has been asked of it yet: keep holding, do a turn's worth of
        // work, and let the hub put the request into the next context.
        drop(api_idle().acquire().await);
        api_polls().fetch_add(1, Ordering::SeqCst);
        return call("a-poll", "fs.list", serde_json::json!({ "path": "src" }));
    }
    if calls(t, "board.handoff") == 0 {
        drop(api_handoff().acquire().await);
        return call("a3", "board.handoff", capsule("TSK-1", "api"));
    }
    StreamEvent::Text("api done".into())
}

/// Lane `web`: claim, walk into the collision, ask for the file, write it,
/// then ask for the guarded one — which is not the holder's to give.
async fn web_turn(t: &str) -> StreamEvent {
    let writes = calls(t, "fs.write");
    let requests = calls(t, "lease.request");
    if calls(t, "lease.claim") == 0 {
        drop(web_start().acquire().await);
        return call(
            "w1",
            "lease.claim",
            serde_json::json!({ "task": "TSK-2", "paths": ["app/src/web/**"] }),
        );
    }
    if writes == 0 {
        // The collision: routes.rs belongs to lane api right now.
        return call(
            "w2",
            "fs.write",
            serde_json::json!({ "path": CONTESTED, "content": "// web was here\n" }),
        );
    }
    if requests == 0 {
        return call(
            "w3",
            "lease.request",
            serde_json::json!({
                "path": CONTESTED,
                "for_task": "TSK-2",
                "reason": "the checkout route has to be registered somewhere",
            }),
        );
    }
    if writes == 1 {
        return call(
            "w4",
            "fs.write",
            serde_json::json!({ "path": CONTESTED, "content": "// checkout route\n" }),
        );
    }
    if requests == 1 {
        return call(
            "w5",
            "lease.request",
            serde_json::json!({
                "path": GUARDED,
                "for_task": "TSK-2",
                "reason": "the form needs the payment intent shape",
            }),
        );
    }
    if calls(t, "board.handoff") == 0 {
        return call("w6", "board.handoff", capsule("TSK-2", "web"));
    }
    StreamEvent::Text("web done".into())
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

fn test_home() -> std::path::PathBuf {
    static INIT: std::sync::Once = std::sync::Once::new();
    let dir = std::env::temp_dir().join(format!("parzi-test-e2e-{}", std::process::id()));
    INIT.call_once(|| {
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
    });
    dir
}

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
    let home = test_home();
    let repo = tiny_repo(&home);
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
            header: "script/head".into(),
            orchestrator: "script/orch".into(),
            coder: "script/coder".into(),
        },
        status: Status::Drafting,
        critical: vec!["app/src/payments/**".to_string()],
        why: "People cannot pay.".into(),
        ..Project::default()
    })
    .unwrap();

    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = 4;
    let store = SessionStore::open().unwrap();
    Arc::new(
        Orchestrator::new(cfg, store).with_factory(Arc::new(|_id: &str, _c: &ParziConfig| {
            Ok(Box::new(ScriptProvider) as Box<dyn Provider>)
        })),
    )
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
    //    allowed to walk into one of them.
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
            result_in(&store, &run, "fs.write", |ok, out| {
                !ok && out.contains("lane api") && out.contains("TSK-1")
            })
        }
    })
    .await;
    let colliding = t.elapsed();

    // 6. The ask. `web` requests the file; the request reaches `api` as an
    //    inter-session message, `api` grants it, and the file is web's.
    let t = Instant::now();
    until(30, "the request to reach the holder", || {
        let (hub, run) = (hub.clone(), api.session_id.clone());
        async move { !hub.open_requests_for(&run).await.is_empty() }
    })
    .await;
    api_idle().add_permits(4);
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
        async move { result_in(&store, &run, "fs.write", |ok, _| ok) }
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
    assert_eq!(transfers[0].args["critical"], serde_json::json!(true));
    assert!(
        transfers[0].args["path"]
            .as_str()
            .unwrap_or_default()
            .contains("payments"),
        "the card names the file: {:?}",
        transfers[0].args
    );
    assert_eq!(transfers[0].args["from_lane"], serde_json::json!("api"));
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
         total      {:>10.1?}   (api polled {} turns)\n",
        t0.elapsed(),
        api_polls().load(Ordering::SeqCst),
    );
}
