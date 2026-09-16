//! PLAN §4 on one machine: two lanes, one file, and every way that ends.
//!
//! Everything here goes through the real tool surface (`ToolExecutor::execute`)
//! with the real hub, so a rename in the tool names breaks these tests — which
//! is the point. The one end-to-end case drives an `AgentRun` with a scripted
//! provider, to prove the tools reach a model at all.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use parzi_core::journal;
use parzi_core::lease::Holder;
use parzi_core::project::{self, Project, Status};
use parzi_core::store::{Event, SessionStore};
use parzi_core::workspace::{self, RepoRef, Workspace};
use parzi_providers::{AuthStatus, ChatReq, EventRx, Model, Provider, StreamEvent};
use parzi_runtime::handler::{AgentRun, ProviderSlot, RunEvent};
use parzi_runtime::inter::{self, InterKind};
use parzi_runtime::lease_tools::{LeaseCtx, LeaseHub};
use parzi_runtime::mcp::McpManager;
use parzi_runtime::tools::{Approval, ApprovalMode, Approver, ToolCallInfo, ToolExecutor};
use tokio::sync::mpsc;

/// Hermetic home for this test binary: no test ever touches the real ~/.parzi.
static TEST_HOME_INIT: std::sync::Once = std::sync::Once::new();

fn test_home() {
    TEST_HOME_INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("parzi-test-leases-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
    });
}

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

/// One lane as the test drives it: its executor, its run id, and the events
/// the handler would have shown a person.
struct Lane {
    tools: ToolExecutor,
    run: String,
    notices: mpsc::UnboundedReceiver<RunEvent>,
}

impl Lane {
    async fn call(&self, name: &str, args: serde_json::Value) -> (bool, String) {
        self.tools.execute(name, &args).await
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

/// A workspace with one repo, a project with one `critical:` glob, and a hub
/// whose "no answer" timeout is short enough to test.
fn world(tag: &str, critical: &[&str]) -> World {
    test_home();
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
        hub: Arc::new(LeaseHub::new().with_answer_timeout(Duration::from_millis(250))),
        store: SessionStore::open().unwrap(),
        workspace: name,
        slug: "checkout".into(),
        cwd,
    }
}

impl World {
    /// Register one lane on the hub, exactly as `project_flow` does before it
    /// dispatches a coder run.
    async fn lane(&self, lane: &str, approver: Option<Arc<dyn Approver>>) -> Lane {
        let run = self
            .store
            .create(&self.slug, lane, "", "scripted/m")
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
                Some(tx),
                approver,
                Some(self.store.clone()),
            )
            .await;
        self.hub
            .bind_project(&run, &self.workspace, &self.slug)
            .await;
        self.hub.bind_repo(&run, "api").await;
        Lane {
            tools: ToolExecutor {
                cwd: self.cwd.to_string_lossy().to_string(),
                mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
                allowed: vec!["*".into()],
                leases: Some(LeaseCtx::new(self.hub.clone(), &run)),
            },
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
    serde_json::json!({"task": task, "paths": paths})
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
    std::fs::write(w.cwd.join("src/routes.rs"), "fn main() {}").unwrap();

    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    let (ok, msg) = web
        .call(
            "fs.write",
            serde_json::json!({"path": "src/routes.rs", "content": "mine now"}),
        )
        .await;
    assert!(!ok, "a write into another lane's file must fail");
    assert!(msg.contains("lane api"), "name the holder: {msg}");
    assert_eq!(
        std::fs::read_to_string(w.cwd.join("src/routes.rs")).unwrap(),
        "fn main() {}",
        "the file must be untouched"
    );

    let (ok, _) = web
        .call("fs.read", serde_json::json!({"path": "src/routes.rs"}))
        .await;
    assert!(ok, "reads are never blocked by a lease");
}

#[tokio::test]
async fn a_shell_write_into_a_held_file_is_refused_and_undone() {
    let w = world("shell-gate", &[]);
    let api = w.lane("api", None).await;
    let web = w.lane("web", None).await;
    std::fs::write(w.cwd.join("src/routes.rs"), "fn main() {}").unwrap();
    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    let cmd = if cfg!(windows) {
        "echo mine>src\\routes.rs"
    } else {
        "printf mine > src/routes.rs"
    };
    let (ok, msg) = web
        .call("shell.exec", serde_json::json!({"cmd": cmd}))
        .await;
    assert!(
        !ok,
        "shell.exec must not keep a write into a held file: {msg}"
    );
    assert!(
        msg.contains("lane api") && msg.contains("lease_violation"),
        "the refusal must name the holder: {msg}"
    );
    assert_eq!(
        std::fs::read_to_string(w.cwd.join("src/routes.rs")).unwrap(),
        "fn main() {}",
        "the held file must be restored"
    );
}

#[tokio::test]
async fn a_scope_notice_reaches_the_host_bus() {
    test_home();
    let (bus, _) = tokio::sync::broadcast::channel(16);
    let mut rx = bus.subscribe();
    let w = {
        let name = format!("ws-bus-{}", std::process::id());
        let cwd = std::env::temp_dir().join(format!("parzi-lease-bus-{}", std::process::id()));
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
            why: "so a person can pay".into(),
            ..Project::default()
        };
        project::save(&project).unwrap();
        World {
            hub: Arc::new(
                LeaseHub::new()
                    .with_answer_timeout(Duration::from_millis(250))
                    .with_bus(bus),
            ),
            store: SessionStore::open().unwrap(),
            workspace: name,
            slug: "checkout".into(),
            cwd,
        }
    };
    let api = w.lane("api", None).await;
    std::fs::write(w.cwd.join("src/page.rs"), "page").unwrap();
    std::fs::write(w.cwd.join("src/other.rs"), "other").unwrap();
    api.call("lease.claim", claim("TSK-8", &["src/page.rs"]))
        .await;
    let (ok, _) = api
        .call(
            "fs.write",
            serde_json::json!({"path": "src/other.rs", "content": "creep"}),
        )
        .await;
    assert!(ok, "scope creep is allowed (and noticed) unless strict");
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

    let (ok, _) = api
        .call(
            "fs.write",
            serde_json::json!({"path": "src/elsewhere.rs", "content": "// stray"}),
        )
        .await;
    assert!(ok, "creep is allowed — this is a notice, not a wall");
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

    let (ok, msg) = api
        .call(
            "fs.write",
            serde_json::json!({"path": "src/elsewhere.rs", "content": "// stray"}),
        )
        .await;
    assert!(!ok, "strict workspaces refuse: {msg}");
    assert!(msg.contains("strict"), "say which rule bit: {msg}");
    assert!(!w.cwd.join("src/elsewhere.rs").exists());
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

/// The scripted provider for the end-to-end case: claim a file another lane
/// holds, then stop. One turn per tool result.
struct ScriptedCoder {
    turns: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl Provider for ScriptedCoder {
    fn id(&self) -> &'static str {
        "scripted"
    }
    async fn models(&self) -> Result<Vec<Model>, parzi_core::error::ParziError> {
        Ok(vec![])
    }
    async fn chat_stream(&self, _req: ChatReq) -> Result<EventRx, parzi_core::error::ParziError> {
        let (tx, rx) = mpsc::unbounded_channel();
        if self.turns.fetch_add(1, Ordering::SeqCst) == 0 {
            tx.send(Ok(StreamEvent::ToolCall {
                id: "c1".into(),
                name: "lease.claim".into(),
                args: claim("TSK-9", &["src/routes.rs"]),
            }))
            .unwrap();
        } else {
            tx.send(Ok(StreamEvent::Text("blocked, asking instead".into())))
                .unwrap();
        }
        Ok(rx)
    }
    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Ok
    }
}

#[tokio::test]
async fn a_real_run_sees_the_lease_tools_and_the_collision() {
    let w = world("run", &[]);
    let api = w.lane("api", None).await;
    let web = w.lane("web", None).await;
    api.call("lease.claim", claim("TSK-8", &["src/routes.rs"]))
        .await;

    let tools = Arc::new(web.tools);
    assert!(
        tools.defs().iter().any(|d| d.name == "lease.claim")
            && tools.defs().iter().any(|d| d.name == "board.handoff"),
        "a project lane is offered the lease and board tools"
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let run = AgentRun::new(
        web.run.clone(),
        vec![ProviderSlot {
            provider_id: "scripted".into(),
            provider: Box::new(ScriptedCoder {
                turns: Arc::new(AtomicUsize::new(0)),
            }),
            model_id: "m".into(),
            price_in: 0.0,
            price_out: 0.0,
            reason: "test",
        }],
        vec![],
        "web".into(),
        ApprovalMode::Auto,
        4,
        4096,
        vec![],
        "low".into(),
        w.store.clone(),
        tools,
        Arc::new(parzi_runtime::tools::AutoApprover),
        tx,
        tokio_util::sync::CancellationToken::new(),
    );
    run.run("do TSK-9").await.unwrap();

    let refused = w.store.events(&web.run).unwrap().into_iter().any(|e| {
        matches!(e, Event::ToolResult { name, ok, output, .. }
            if name == "lease.claim" && !ok && output.contains("lane api"))
    });
    assert!(
        refused,
        "the run was told, in its own transcript, who holds it"
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
