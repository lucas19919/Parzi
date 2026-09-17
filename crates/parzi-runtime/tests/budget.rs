//! R-4: `budget:` is a promise the runtime keeps. A run that reaches its
//! token or cost cap pauses — loudly (a Notice and a line in the thread), with
//! `budget_exceeded` on the session — and never silently continues spending.

use std::collections::HashMap;
use std::sync::Arc;

use parzi_core::config::{Budget, ParziConfig};
use parzi_core::project::{Project, Roster, Status};
use parzi_core::store::{SessionStatus, SessionStore};
use parzi_providers::{AuthStatus, ChatReq, EventRx, Model, Provider, StreamEvent};
use parzi_runtime::handler::{AgentRun, ProviderSlot, RunEvent};
use parzi_runtime::mcp::McpManager;
use parzi_runtime::orchestrator::{run_note, BUDGET_NOTE};
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo, ToolExecutor};
use parzi_runtime::Orchestrator;
use tokio::sync::mpsc;

fn test_home() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("parzi-test-budget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
    });
}

/// Reports 1 000 tokens per turn and answers with text, forever.
struct SpendProvider;

#[async_trait::async_trait]
impl Provider for SpendProvider {
    fn id(&self) -> &'static str {
        "spend"
    }
    async fn models(&self) -> parzi_core::error::Result<Vec<Model>> {
        Ok(vec![])
    }
    async fn chat_stream(&self, _req: ChatReq) -> parzi_core::error::Result<EventRx> {
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = tx.send(Ok(StreamEvent::Text("spending".into())));
        let _ = tx.send(Ok(StreamEvent::Usage {
            tokens_in: 600,
            tokens_out: 400,
        }));
        Ok(rx)
    }
    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Ok
    }
}

fn spend_factory(_id: &str, _cfg: &ParziConfig) -> parzi_core::error::Result<Box<dyn Provider>> {
    Ok(Box::new(SpendProvider))
}

struct Allow;
#[async_trait::async_trait]
impl Approver for Allow {
    async fn approve(&self, _c: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

fn tools() -> Arc<ToolExecutor> {
    Arc::new(ToolExecutor {
        cwd: String::new(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: vec![],
        leases: None,
    })
}

#[test]
fn tightest_budget_wins_and_reports_the_reason() {
    let config = Budget {
        max_cost_usd: Some(10.0),
        max_tokens: None,
    };
    let project = Budget {
        max_cost_usd: Some(2.0),
        max_tokens: Some(5_000),
    };
    let both = config.tightest(project);
    assert_eq!(both.max_cost_usd, Some(2.0));
    assert_eq!(both.max_tokens, Some(5_000));
    assert!(Budget::default().is_unlimited());
    assert!(Budget::default().exceeded(1_000_000, 500.0).is_none());
    let reason = both.exceeded(5_000, 0.0).expect("token cap reached");
    assert!(reason.contains("token budget"), "{reason}");
    let reason = both.exceeded(0, 2.5).expect("cost cap reached");
    assert!(reason.contains("cost budget"), "{reason}");
}

/// The run stops at the cap, says why, and stays available (Idle) instead of
/// being killed: §15.6 — "lanes pause, the header explains".
#[tokio::test]
async fn token_budget_pauses_the_run_and_marks_it() {
    test_home();
    let store = SessionStore::open().unwrap();
    let sid = store.create("spender", "t", "", "spend/m").unwrap().id;
    // Unbounded sink: the run never blocks on it, so the events are read
    // after the run instead of racing a drain task.
    let (tx, mut rx) = mpsc::unbounded_channel::<RunEvent>();
    let run = AgentRun::new(
        sid.clone(),
        vec![ProviderSlot {
            provider_id: "spend".into(),
            provider: Box::new(SpendProvider),
            model_id: "m".into(),
            price_in: 0.0,
            price_out: 0.0,
            reason: "test",
        }],
        vec![],
        "test".into(),
        parzi_runtime::tools::ApprovalMode::Auto,
        false,
        // 32 steps would be plenty to spend past the cap without it.
        32,
        4096,
        vec![],
        "low".into(),
        store.clone(),
        tools(),
        Arc::new(Allow),
        tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .with_budget(Budget {
        max_cost_usd: None,
        max_tokens: Some(1_000),
    });
    run.run("spend it").await.unwrap();

    assert_eq!(store.get(&sid).unwrap().status, SessionStatus::Idle);
    assert_eq!(run_note(&sid).as_deref(), Some(BUDGET_NOTE));
    let mut notices: Vec<String> = vec![];
    while let Ok(ev) = rx.try_recv() {
        if let RunEvent::Notice { text } = ev {
            notices.push(text);
        }
    }
    assert!(
        notices.iter().any(|n| n.contains("token budget")),
        "the pause must be announced: {notices:?}"
    );
    let said = store.events(&sid).unwrap().iter().any(|e| {
        matches!(
            e,
            parzi_core::store::Event::System { text } if text.contains("Paused")
        )
    });
    assert!(said, "the thread must say why it stopped");

    if let Ok(d) = parzi_core::paths::sessions_dir() {
        let _ = std::fs::remove_dir_all(d.join(&sid));
    }
}

/// A run dispatched for a project honours that project's `budget:` line — the
/// cap in PROJECT.md, not just the machine's config.
#[tokio::test]
async fn project_budget_is_read_from_project_md() {
    test_home();
    let project = Project {
        slug: "spent".into(),
        title: "Spent".into(),
        workspace: "budgets".into(),
        repos: vec![],
        roster: Roster {
            header: "spend/m".into(),
            orchestrator: "spend/m".into(),
            coder: "spend/m".into(),
        },
        // Nothing left to spend: the next turn must not be bought.
        budget_usd: Some(0.0),
        status: Status::Running,
        critical: vec![],
        why: "prove the cap".into(),
        what: vec![],
        constraints: vec![],
    };
    std::fs::create_dir_all(parzi_core::project::dir("budgets", "spent")).unwrap();
    parzi_core::project::save(&project).unwrap();

    let store = SessionStore::open().unwrap();
    let orch = Arc::new(
        Orchestrator::new(ParziConfig::default(), store.clone())
            .with_factory(Arc::new(spend_factory)),
    );
    let (meta, mut rx) = orch
        .spawn_in_project(
            Some(("budgets".into(), "spent".into())),
            "t",
            "",
            "spend/m",
            "do the work",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    let mut paused = false;
    while let Some(ev) = rx.recv().await {
        if let RunEvent::Notice { text } = ev {
            if text.contains("cost budget") {
                paused = true;
            }
        }
    }
    assert!(paused, "a project with no budget left must pause its lane");
    assert_eq!(store.get(&meta.id).unwrap().status, SessionStatus::Idle);
    assert_eq!(run_note(&meta.id).as_deref(), Some(BUDGET_NOTE));

    if let Ok(d) = parzi_core::paths::sessions_dir() {
        let _ = std::fs::remove_dir_all(d.join(&meta.id));
    }
}
