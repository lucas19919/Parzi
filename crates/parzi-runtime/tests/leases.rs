//! PLAN §4 on one machine: two lanes, one file, and every way that ends.
//!
//! The lease tools go through the real tool surface (`ToolExecutor::execute`)
//! with the real hub, so a rename in the tool names breaks these tests — which
//! is the point. The agent's own edits and commands go through the gate a
//! vendor agent asks before it acts (`ToolHost`), and the end-to-end case
//! drives a run whose scripted agent calls the lease tools over MCP.

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use parzi_core::journal;
use parzi_core::lease::Holder;
use parzi_core::project::{self, Project, Status};
use parzi_core::store::{Event, SessionStore};
use parzi_core::workspace::{self, RepoRef, Workspace};
use parzi_providers::{PermissionDecision, PermissionGate, PermissionRequest, TurnEnd};
use parzi_runtime::handler::{RunEvent, RunSink};
use parzi_runtime::inter::{self, InterKind};
use parzi_runtime::lease_tools::{LeaseCtx, LeaseHub};
use parzi_runtime::mcp::McpManager;
use parzi_runtime::toolhost::{ToolHost, ToolHostParts};
use parzi_runtime::tools::{
    to_mcp, Approval, ApprovalMode, Approver, AutoApprover, ToolCallInfo, ToolExecutor,
};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// An approver that remembers what it was shown — the approval card a
/// `critical` transfer must raise.
struct Recording {
    seen: Arc<Mutex<Vec<ToolCallInfo>>>,
    answer: Approval,
}

#[async_trait::async_trait]
impl Approver for Recording {
    async fn approve(&self, call: &ToolCallInfo) -> Approval {
        self.seen.lock().unwrap().push(call.clone());
        self.answer
    }
}

/// One lane as the test drives it: its tools, the gate its agent asks, its
/// run id, and the events the app would have shown a person.
struct Lane {
    tools: Arc<ToolExecutor>,
    host: ToolHost,
    run: String,
    notices: mpsc::UnboundedReceiver<RunEvent>,
}

impl Lane {
    async fn call(&self, name: &str, args: serde_json::Value) -> (bool, String) {
        self.tools.execute(name, &args).await
    }

    /// Ask the gate before touching `file`, as a vendor agent does before
    /// one of its own edits or reads. Vendors name files absolutely.
    async fn ask(&self, tool: &str, file: &Path) -> PermissionDecision {
        self.host
            .decide(PermissionRequest {
                id: format!("req-{}", uuid::Uuid::new_v4()),
                tool: tool.into(),
                title: tool.into(),
                input: json!({"file_path": file}),
                paths: vec![file.display().to_string()],
            })
            .await
    }

    /// Every event the lane was sent so far, without blocking.
    fn drain(&mut self) -> Vec<RunEvent> {
        let mut out = vec![];
        while let Ok(e) = self.notices.try_recv() {
            out.push(e);
        }
        out
    }
}

struct World {
    hub: Arc<LeaseHub>,
    store: SessionStore,
    workspace: String,
    slug: String,
    cwd: std::path::PathBuf,
}

/// A workspace with one repo, a project with its `critical:` globs, and a hub
/// whose "no answer" timeout is short enough to test.
fn world(tag: &str, critical: &[&str]) -> World {
    home("leases");
    let hub = LeaseHub::new().with_answer_timeout(Duration::from_millis(250));
    world_on(tag, critical, Arc::new(hub), SessionStore::open().unwrap())
}

fn world_on(tag: &str, critical: &[&str], hub: Arc<LeaseHub>, store: SessionStore) -> World {
    home("leases");
    let name = format!("ws-{tag}");
    let cwd = std::env::temp_dir().join(format!("parzi-lease-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(cwd.join("src")).unwrap();
    workspace::create(Workspace {
        name: name.clone(),
        repos: vec![RepoRef {
            name: "api".into(),
            remote: "git@example.com:acme/api.git".into(),
            default_branch: "main".into(),
            local_path: Some(cwd.clone()),
        }],
        ..Workspace::solo(&name)
    })
    .unwrap();
    let project = Project {
        slug: "checkout".into(),
        title: "Checkout flow".into(),
        workspace: name.clone(),
        repos: vec!["api".into()],
        status: Status::Running,
        critical: critical.iter().map(|s| (*s).to_string()).collect(),
        why: "so a person can pay".into(),
        ..Project::default()
    };
    project::save(&project).unwrap();
    World {
        hub,
        store,
        workspace: name,
        slug: "checkout".into(),
        cwd,
    }
}

impl World {
    /// Register one lane on the hub, exactly as `project_flow` does before it
    /// dispatches a coder run. The lane's approval mode is Auto, so the lease
    /// layer is the only thing that can refuse.
    async fn lane(&self, lane: &str, approver: Option<Arc<dyn Approver>>) -> Lane {
        let run = self
            .store
            .create(lane, &self.slug, lane, "claude/m")
            .unwrap()
            .id;
        let (tx, notices) = mpsc::unbounded_channel();
        self.hub
            .register(
                &run,
                Holder {
                    user: "ada".into(),
                    machine: "desk".into(),
                    run: Some(run.clone()),
                    lane: lane.to_string(),
                },
                Some(tx.clone()),
                approver,
                Some(self.store.clone()),
            )
            .await;
        self.hub
            .bind_project(&run, &self.workspace, &self.slug)
            .await;
        self.hub.bind_repo(&run, "api").await;
        let tools = Arc::new(ToolExecutor {
            cwd: self.cwd.to_string_lossy().to_string(),
            mcp: Arc::new(McpManager::new(std::collections::HashMap::new(), 60)),
            allowed: vec!["*".into()],
            leases: Some(LeaseCtx::new(self.hub.clone(), &run)),
        });
        let host = ToolHost::new(ToolHostParts {
            session_id: run.clone(),
            lane: lane.to_string(),
            mode: ApprovalMode::Auto,
            edits_auto: false,
            store: self.store.clone(),
            tools: tools.clone(),
            approver: Arc::new(AutoApprover),
            harness: None,
            role: None,
            sink: RunSink::new(&run, tx, None),
            cancel: CancellationToken::new(),
        });
        Lane {
            tools,
            host,
            run,
            notices,
        }
    }

    fn journal(&self) -> Vec<journal::JournalLine> {
        journal::read(&self.workspace, &self.slug)
    }

    /// The id of the request waiting on `run`, once it has arrived.
    async fn open_request(&self, run: &str) -> String {
        for _ in 0..200 {
            if let Some(id) = self.hub.open_requests_for(run).await.first() {
                return id.clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("no lease request ever reached {run}");
    }

    async fn holder_lane_of(&self, path: &str) -> Option<String> {
        self.hub.holder_of_path(path).await.map(|l| l.holder.lane)
    }
}

fn claim(task: &str, paths: &[&str]) -> serde_json::Value {
    json!({"task": task, "paths": paths})
}

#[tokio::test]
async fn a_second_lane_is_told_who_holds_the_file() {
    let w = world("collision", &[]);
    let api = w.lane("api", None).await;
    let web = w.lane("web", None).await;

    let (ok, _) = api
        .call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;
    assert!(ok, "the first claim must be granted");

    let (ok, msg) = web
        .call("lease.claim", claim("TSK-9", &["src/routes.rs"]))
        .await;
    assert!(!ok, "an overlapping claim must not be granted");
    assert!(
        msg.contains("lane api") && msg.contains("TSK-8"),
        "the refusal must name the holder and its task: {msg}"
    );
    assert!(
        msg.contains("lease.request"),
        "the refusal must say how to ask: {msg}"
    );
}

#[tokio::test]
async fn a_write_into_a_held_file_is_refused_and_a_read_is_not() {
    let w = world("gate", &[]);
    let api = w.lane("api", None).await;
    let web = w.lane("web", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    let file = w.cwd.join("src/routes.rs");
    match web.ask("Edit", &file).await {
        PermissionDecision::Deny(why) => assert!(why.contains("lane api"), "name the holder: {why}"),
        other => panic!("an edit of another lane's file must be refused: {other:?}"),
    }
    assert_eq!(
        web.ask("Read", &file).await,
        PermissionDecision::Allow,
        "reads are never blocked by a lease"
    );
}

#[tokio::test]
async fn a_shell_write_into_a_held_file_is_refused_and_undone() {
    let w = world("shell-gate", &[]);
    let api = w.lane("api", None).await;
    let web = w.lane("web", None).await;
    std::fs::write(w.cwd.join("src/routes.rs"), "fn main() {}").unwrap();
    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    // A command cannot say what it will write: the gate lets it run, and
    // the lease layer looks at the worktree once it is done.
    let asked = web
        .host
        .decide(PermissionRequest {
            id: "sh-1".into(),
            tool: "Bash".into(),
            title: "echo mine > src/routes.rs".into(),
            input: json!({"command": "echo mine > src/routes.rs"}),
            paths: vec![],
        })
        .await;
    assert_eq!(asked, PermissionDecision::Allow);
    web.host.tool_started("sh-1", "Bash").await;
    std::fs::write(w.cwd.join("src/routes.rs"), "mine").unwrap();

    let refusal = web
        .host
        .tool_finished("sh-1")
        .await
        .expect("a command that wrote into a held file is refused");
    assert!(
        refusal.contains("lane api") && refusal.contains("lease_violation"),
        "the refusal must name the holder: {refusal}"
    );
    assert_eq!(
        std::fs::read_to_string(w.cwd.join("src/routes.rs")).unwrap(),
        "fn main() {}",
        "the held file must be restored"
    );
}

#[tokio::test]
async fn a_scope_notice_reaches_the_host_bus() {
    home("leases");
    let (bus, _) = tokio::sync::broadcast::channel(16);
    let mut rx = bus.subscribe();
    let hub = LeaseHub::new()
        .with_answer_timeout(Duration::from_millis(250))
        .with_bus(bus);
    let w = world_on("bus", &[], Arc::new(hub), SessionStore::open().unwrap());
    let api = w.lane("api", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/page.rs"]))
        .await;

    assert_eq!(
        api.ask("Write", &w.cwd.join("src/other.rs")).await,
        PermissionDecision::Allow,
        "scope creep is allowed (and noticed) unless strict"
    );
    let (_sid, ev) = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("the host bus must hear the notice")
        .expect("bus still open");
    match ev {
        RunEvent::Notice { text } => {
            assert!(
                text.contains("scope_creep") && text.contains("src/other.rs"),
                "notice names the file: {text}"
            );
        }
        other => panic!("expected a Notice, got {other:?}"),
    }
}

#[tokio::test]
async fn a_request_the_holder_grants_moves_the_file() {
    let w = world("grant", &[]);
    let api = w.lane("api", None).await;
    let web = w.lane("web", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    let asking = {
        let tools = web.tools;
        tokio::spawn(async move {
            tools
                .execute(
                    "lease.request",
                    &serde_json::json!({
                        "path": "src/routes.rs",
                        "for_task": "TSK-9",
                        "reason": "the route table is where the endpoint lands",
                    }),
                )
                .await
        })
    };

    let id = w.open_request(&api.run).await;
    // H-5: the request reached the holder's transcript as typed data, not as
    // a user turn.
    let inbox = inter::inbox(&w.store, &api.run).unwrap();
    assert_eq!(inbox.len(), 1, "one inter-session message: {inbox:?}");
    assert_eq!(inbox[0].kind, InterKind::LeaseRequest);
    assert!(inbox[0].body.contains(&id), "the body carries the id");

    let (ok, _) = api
        .call("lease.grant", serde_json::json!({"request_id": id}))
        .await;
    assert!(ok, "the holder may grant");

    let (ok, msg) = asking.await.unwrap();
    assert!(ok, "the asking lane must be told yes: {msg}");
    assert_eq!(
        w.holder_lane_of("api/src/routes.rs").await.as_deref(),
        Some("web"),
        "the file moves to the lane that asked"
    );
    let kinds: Vec<journal::Kind> = w.journal().iter().map(|l| l.kind).collect();
    assert!(kinds.contains(&journal::Kind::Request) && kinds.contains(&journal::Kind::Grant));
}

#[tokio::test]
async fn a_refusal_keeps_the_file_where_it_is() {
    let w = world("deny", &[]);
    let api = w.lane("api", None).await;
    let web = w.lane("web", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    let asking = {
        let tools = web.tools;
        tokio::spawn(async move {
            tools
                .execute(
                    "lease.request",
                    &serde_json::json!({
                        "path": "src/routes.rs",
                        "for_task": "TSK-9",
                        "reason": "I want it",
                    }),
                )
                .await
        })
    };
    let id = w.open_request(&api.run).await;
    api.call(
        "lease.deny",
        serde_json::json!({"request_id": id, "reason": "mid-refactor, ask after TSK-8"}),
    )
    .await;

    let (ok, msg) = asking.await.unwrap();
    assert!(!ok, "a denial is a tool failure for the asker");
    assert!(msg.contains("mid-refactor"), "the reason travels: {msg}");
    assert_eq!(
        w.holder_lane_of("api/src/routes.rs").await.as_deref(),
        Some("api")
    );
}

#[tokio::test]
async fn silence_is_a_denial() {
    let w = world("timeout", &[]);
    let api = w.lane("api", None).await;
    let web = w.lane("web", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    // Nobody answers: the hub's timeout (250 ms here, 120 s in production)
    // turns the request into a deny on its own.
    let (ok, msg) = web
        .call(
            "lease.request",
            serde_json::json!({
                "path": "src/routes.rs",
                "for_task": "TSK-9",
                "reason": "still need it",
            }),
        )
        .await;
    assert!(!ok, "no answer must not be a grant");
    assert!(msg.contains("no answer"), "say why: {msg}");
    assert_eq!(
        w.holder_lane_of("api/src/routes.rs").await.as_deref(),
        Some("api"),
        "an unanswered request never moves a file"
    );
}

#[tokio::test]
async fn a_critical_path_asks_a_human_and_allow_moves_it() {
    let w = world("critical", &["api/src/payments/**"]);
    let seen = Arc::new(Mutex::new(vec![]));
    let approver: Arc<dyn Approver> = Arc::new(Recording {
        seen: seen.clone(),
        answer: Approval::Allow,
    });
    let mut api = w.lane("api", Some(approver)).await;
    let web = w.lane("web", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/payments/intent.rs"]))
        .await;

    let (ok, msg) = web
        .call(
            "lease.request",
            serde_json::json!({
                "path": "src/payments/intent.rs",
                "for_task": "TSK-9",
                "reason": "the web form needs the intent shape",
            }),
        )
        .await;
    assert!(ok, "an approved transfer is a grant: {msg}");

    let cards = seen.lock().unwrap().clone();
    assert_eq!(cards.len(), 1, "exactly one approval card");
    assert_eq!(cards[0].name, "lease.transfer", "it is a transfer card");
    assert_eq!(cards[0].args["critical"], serde_json::json!(true));
    assert!(
        api.drain()
            .iter()
            .any(|e| matches!(e, RunEvent::ApprovalRequest { .. })),
        "the holder's app must be shown the card"
    );
    assert_eq!(
        w.holder_lane_of("api/src/payments/intent.rs")
            .await
            .as_deref(),
        Some("web")
    );
    assert!(
        w.journal().iter().any(|l| l.kind == journal::Kind::Approve),
        "a human decision is journalled"
    );
}

#[tokio::test]
async fn a_critical_path_nobody_approves_stays_put() {
    let w = world("critical-deny", &["api/src/payments/**"]);
    let approver: Arc<dyn Approver> = Arc::new(Recording {
        seen: Arc::new(Mutex::new(vec![])),
        answer: Approval::Deny,
    });
    let api = w.lane("api", Some(approver)).await;
    let web = w.lane("web", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/payments/intent.rs"]))
        .await;

    let (ok, msg) = web
        .call(
            "lease.request",
            serde_json::json!({
                "path": "src/payments/intent.rs",
                "for_task": "TSK-9",
                "reason": "I would like it",
            }),
        )
        .await;
    assert!(!ok, "a refused card is a refusal: {msg}");
    assert_eq!(
        w.holder_lane_of("api/src/payments/intent.rs")
            .await
            .as_deref(),
        Some("api")
    );
}

#[tokio::test]
async fn a_write_outside_the_scope_is_noticed_not_refused() {
    let w = world("creep", &[]);
    let mut api = w.lane("api", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    assert_eq!(
        api.ask("Write", &w.cwd.join("src/elsewhere.rs")).await,
        PermissionDecision::Allow,
        "creep is allowed — this is a notice, not a wall"
    );
    assert!(
        api.drain()
            .iter()
            .any(|e| matches!(e, RunEvent::Notice { text } if text.contains("scope_creep"))),
        "the lane is told"
    );
    assert!(
        w.journal()
            .iter()
            .any(|l| l.kind == journal::Kind::Note && l.text.contains("scope_creep")),
        "and the project remembers"
    );
}

#[tokio::test]
async fn strict_mode_refuses_the_creep() {
    let w = world("strict", &[]);
    let file = workspace::workspace_file(&w.workspace);
    let raw = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, format!("lease_mode = \"strict\"\n{raw}")).unwrap();
    let api = w.lane("api", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    match api.ask("Write", &w.cwd.join("src/elsewhere.rs")).await {
        PermissionDecision::Deny(why) => assert!(why.contains("strict"), "say which rule bit: {why}"),
        other => panic!("strict workspaces refuse: {other:?}"),
    }
}

#[tokio::test]
async fn a_capsule_without_a_verification_line_is_not_a_handoff() {
    let w = world("capsule", &[]);
    let api = w.lane("api", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    let (ok, msg) = api
        .call(
            "board.handoff",
            serde_json::json!({
                "task": "TSK-8",
                "capsule": {"summary": "endpoint added", "touched_files": ["src/routes.rs"]},
            }),
        )
        .await;
    assert!(!ok, "a capsule with no verification is not a handoff");
    assert!(msg.contains("verification"), "say what is missing: {msg}");
    assert_eq!(
        w.holder_lane_of("api/src/routes.rs").await.as_deref(),
        Some("api"),
        "a failed handoff releases nothing"
    );

    let (ok, msg) = api
        .call(
            "board.handoff",
            serde_json::json!({
                "task": "TSK-8",
                "capsule": {
                    "summary": "checkout session endpoint",
                    "touched_files": ["src/routes.rs"],
                    "exported_symbols": ["pub fn create_session"],
                    "verification": "cargo test -p api checkout",
                },
            }),
        )
        .await;
    assert!(ok, "a complete capsule ends the task: {msg}");
    let stored = parzi_core::capsule::load(
        &w.workspace,
        &w.slug,
        &parzi_core::plan::TaskId("TSK-8".into()),
    )
    .unwrap();
    assert_eq!(stored.verification, "cargo test -p api checkout");
    // §5: on disk twice — the project's capsule file and the session's own
    // artifact — so neither truth depends on the other.
    let artifacts = w
        .store
        .events(&api.run)
        .unwrap()
        .into_iter()
        .filter(|e| matches!(e, Event::Artifact { .. }))
        .count();
    assert_eq!(
        artifacts, 1,
        "the capsule is an artifact of the session too"
    );
    assert!(
        w.holder_lane_of("api/src/routes.rs").await.is_none(),
        "the handoff released the files"
    );
}

/// End to end: the agent of a project lane is offered the lease and board
/// tools over MCP, and a claim on a file another lane holds comes back
/// refused — in its own transcript, naming the holder.
#[tokio::test]
async fn a_real_run_sees_the_lease_tools_and_the_collision() {
    home("leases");
    let coder = Fake::new(
        "claude",
        script(|a: Agent| async move {
            let names = a.tool_names().await;
            let offered = [to_mcp("lease.claim"), to_mcp("board.handoff")]
                .iter()
                .all(|t| names.contains(t));
            let (ok, out) = a
                .parzi("lease.claim", claim("TSK-9", &["src/routes.rs"]))
                .await;
            a.say(&format!("offered={offered} ok={ok}: {out}"));
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(&[coder]);
    let w = world_on("run", &[], orch.leases(), store.clone());
    let api = w.lane("api", None).await;
    let web = w.lane("web", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    drop(
        orch.send_to(&web.run, "do TSK-9", None, "", "low", vec![], None, None)
            .await
            .unwrap(),
    );
    settle(&store, &web.run).await;

    let reply = last_reply(&store, &web.run);
    assert!(
        reply.contains("offered=true"),
        "a project lane is offered the lease and board tools: {reply}"
    );
    let refused = events(&store, &web.run).into_iter().any(|e| {
        matches!(e, Event::ToolResult { name, ok, output, .. }
            if name == "lease.claim" && !ok && output.contains("lane api"))
    });
    assert!(
        refused,
        "the run was told, in its own transcript, who holds it: {reply}"
    );
}

#[test]
fn a_coder_sees_its_task_its_files_and_the_capsule_it_comes_after() {
    let w = world("micro", &[]);
    std::fs::write(
        w.cwd.join("src/checkout.rs"),
        "pub fn create_session() {}\nfn private() {}\n",
    )
    .unwrap();
    std::fs::write(w.cwd.join("src/unrelated.rs"), "pub fn elsewhere() {}\n").unwrap();
    std::fs::write(
        parzi_core::project::dir(&w.workspace, &w.slug).join("KNOWLEDGE.md"),
        "- the payment provider rejects zero amounts\n",
    )
    .unwrap();
    let plan = parzi_core::plan::parse_plan_v1(
        "parzi: 1\n\n\
         ## Sprint 1 — API surface (target: 1 day)\n\
         ### lane api\n\
         - [ ] TSK-7  Checkout session endpoint  [repo:api] [scope:src/checkout.rs]\n  \
           - acceptance: POST /checkout returns 201\n\
         - [ ] TSK-8  Payment intent  [repo:api] [scope:src/payments/**] [after:TSK-7]\n",
    )
    .unwrap();
    let project = parzi_core::project::load(&w.workspace, &w.slug).unwrap();
    let capsule = parzi_core::capsule::HandoffCapsule {
        task: "TSK-7".into(),
        summary: "endpoint lives in src/checkout.rs".into(),
        verification: "cargo test -p api checkout".into(),
        ..Default::default()
    };

    let task = plan.task(&"TSK-8".into()).unwrap();
    let parts = parzi_runtime::context_micro::micro_context(
        &project,
        &plan,
        task,
        std::slice::from_ref(&capsule),
    );
    let all = parts
        .iter()
        .map(|p| format!("{}: {}", p.label, p.text))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(all.contains("TSK-8"), "the task it was given: {all}");
    assert!(all.contains("Sprint 1"), "the sprint it belongs to");
    assert!(
        all.contains("cargo test -p api checkout"),
        "the capsule of its `after:` task, verification and all"
    );
    assert!(
        all.contains("zero amounts"),
        "KNOWLEDGE.md travels with every task"
    );

    // The first task's scope: the files, and the public surface only.
    let first = plan.task(&"TSK-7".into()).unwrap();
    let scoped = parzi_runtime::context_micro::micro_context(&project, &plan, first, &[])
        .iter()
        .map(|p| format!("{}: {}", p.label, p.text))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(scoped.contains("src/checkout.rs"), "the file in scope");
    assert!(!scoped.contains("src/unrelated.rs"), "and nothing else");
    assert!(
        scoped.contains("pub fn create_session"),
        "public signatures"
    );
    assert!(!scoped.contains("fn private"), "private ones stay private");
}
