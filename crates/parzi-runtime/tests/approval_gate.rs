//! B1: Ask mode must wait for the human. A slow approver (500ms) that
//! answers Allow must still let the tool run exactly once — not instant-deny.
//! B4: sequential completed runs must release their slot.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use parzi_core::config::ParziConfig;
use parzi_core::store::SessionStore;
use parzi_providers::{AuthStatus, ChatReq, EventRx, Model, Provider, StreamEvent};
use parzi_runtime::handler::{AgentRun, ProviderSlot, RunEvent};
use parzi_runtime::mcp::McpManager;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo, ToolExecutor};
use parzi_runtime::Orchestrator;
use tokio::sync::mpsc;

/// Hermetic home for this test binary: no test session ever touches the
/// real ~/.parzi (and the sidebar) again. Called first in every test; the
/// Once makes parallel tests share one temp home safely.
static TEST_HOME_INIT: std::sync::Once = std::sync::Once::new();

fn test_home() {
    TEST_HOME_INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("parzi-test-approval-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
    });
}

/// Provider that emits one fs.write tool call, then ends.
struct ToolCallProvider;

#[async_trait::async_trait]
impl Provider for ToolCallProvider {
    fn id(&self) -> &'static str {
        "toolcall"
    }
    async fn models(&self) -> Result<Vec<Model>, parzi_core::error::ParziError> {
        Ok(vec![])
    }
    async fn chat_stream(&self, req: ChatReq) -> Result<EventRx, parzi_core::error::ParziError> {
        let (tx, rx) = mpsc::unbounded_channel();
        // First turn: one tool call. Second turn (after result): plain text.
        let calls_so_far = req
            .messages
            .iter()
            .filter(|m| m.content.contains("[tool:"))
            .count();
        if calls_so_far == 0 && !req.tools.is_empty() {
            let _ = tx.send(Ok(StreamEvent::ToolCall {
                id: "t1".into(),
                name: "fs.write".into(),
                args: serde_json::json!({"path": "gate.txt", "content": "hello"}),
            }));
        } else {
            let _ = tx.send(Ok(StreamEvent::Text("done".into())));
        }
        Ok(rx)
    }
    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Ok
    }
}

/// Slow human: answers Allow after 500ms.
struct SlowAllow;
#[async_trait::async_trait]
impl Approver for SlowAllow {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        Approval::Allow
    }
}

struct DenyAll;
#[async_trait::async_trait]
impl Approver for DenyAll {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Deny
    }
}

fn tool_factory(
    _id: &str,
    _cfg: &ParziConfig,
) -> Result<Box<dyn Provider>, parzi_core::error::ParziError> {
    Ok(Box::new(ToolCallProvider))
}

#[tokio::test]
async fn ask_mode_waits_for_slow_approver() {
    test_home();
    let dir = std::env::temp_dir().join(format!("parzi-gate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cwd = dir.to_string_lossy().to_string();

    let store = SessionStore::open().unwrap();
    let meta = store.create("gate", "t", "", "toolcall/m").unwrap();
    let sid = meta.id.clone();

    let tools = Arc::new(ToolExecutor {
        cwd: cwd.clone(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: vec!["fs.write".into()],
    });
    let slots = vec![ProviderSlot {
        provider_id: "toolcall".into(),
        provider: Box::new(ToolCallProvider),
        model_id: "m".into(),
        price_in: 0.0,
        price_out: 0.0,
        reason: "test",
    }];
    let (tx, mut rx) = mpsc::unbounded_channel();
    // Drain events so emit() never blocks.
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let run = AgentRun::new(
        sid.clone(),
        slots,
        vec![],
        "test".into(),
        parzi_runtime::tools::ApprovalMode::Ask,
        4,
        4096,
        vec![],
        "low".into(),
        store.clone(),
        tools,
        Arc::new(SlowAllow),
        tx,
        tokio_util::sync::CancellationToken::new(),
    );
    let t0 = std::time::Instant::now();
    run.run("write the file").await.unwrap();
    let elapsed = t0.elapsed();
    // Must have waited ~500ms for the human, then run the tool.
    assert!(
        elapsed >= std::time::Duration::from_millis(400),
        "ask returned instantly: {elapsed:?}"
    );
    let written = std::fs::read_to_string(dir.join("gate.txt")).unwrap_or_default();
    assert_eq!(written, "hello", "slow Allow must still execute the tool");

    // Deny path still denies.
    let sid2 = store.create("gate2", "t", "", "toolcall/m").unwrap().id;
    let tools2 = Arc::new(ToolExecutor {
        cwd: cwd.clone(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: vec!["fs.write".into()],
    });
    let slots2 = vec![ProviderSlot {
        provider_id: "toolcall".into(),
        provider: Box::new(ToolCallProvider),
        model_id: "m".into(),
        price_in: 0.0,
        price_out: 0.0,
        reason: "test",
    }];
    let (tx2, mut rx2) = mpsc::unbounded_channel::<RunEvent>();
    tokio::spawn(async move { while rx2.recv().await.is_some() {} });
    let run2 = AgentRun::new(
        sid2.clone(),
        slots2,
        vec![],
        "test".into(),
        parzi_runtime::tools::ApprovalMode::Ask,
        4,
        4096,
        vec![],
        "low".into(),
        store.clone(),
        tools2,
        Arc::new(DenyAll),
        tx2,
        tokio_util::sync::CancellationToken::new(),
    );
    run2.run("write the file").await.unwrap();
    // Second run wrote the same path again only if denied it would NOT overwrite with new content;
    // instead check transcript has a denial ToolResult.
    let events = store.events(&sid2).unwrap();
    let denied = events.iter().any(|e| matches!(
        e,
        parzi_core::store::Event::ToolResult { ok: false, output, .. } if output.contains("denied")
    ));
    assert!(
        denied,
        "Deny approver must produce a denial, got {events:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
    if let Ok(d) = parzi_core::paths::sessions_dir() {
        let _ = std::fs::remove_dir_all(d.join(&sid));
        let _ = std::fs::remove_dir_all(d.join(&sid2));
    }
}

/// B4: max_concurrent sequential completed runs → none queues; a second
/// message to a finished thread succeeds.
#[tokio::test]
async fn sequential_completed_runs_release_slots() {
    test_home();
    struct Answer;
    #[async_trait::async_trait]
    impl Provider for Answer {
        fn id(&self) -> &'static str {
            "answer"
        }
        async fn models(&self) -> Result<Vec<Model>, parzi_core::error::ParziError> {
            Ok(vec![])
        }
        async fn chat_stream(
            &self,
            _req: ChatReq,
        ) -> Result<EventRx, parzi_core::error::ParziError> {
            let (tx, rx) = mpsc::unbounded_channel();
            let _ = tx.send(Ok(StreamEvent::Text("hi".into())));
            Ok(rx)
        }
        fn auth_status(&self) -> AuthStatus {
            AuthStatus::Ok
        }
    }
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let factory = Arc::new(
        |_id: &str,
         _cfg: &ParziConfig|
         -> Result<Box<dyn Provider>, parzi_core::error::ParziError> {
            COUNT.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(Answer))
        },
    );
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = 1;
    cfg.orchestrator.queue_when_busy = true;
    let store = SessionStore::open().unwrap();
    let orch = Orchestrator::new(cfg, store.clone()).with_factory(factory);
    let pump = std::sync::Arc::new(orch);
    let p2 = pump.clone();
    tokio::spawn(async move { p2.pump_loop().await });

    struct Allow;
    #[async_trait::async_trait]
    impl Approver for Allow {
        async fn approve(&self, _c: &ToolCallInfo) -> Approval {
            Approval::Allow
        }
    }
    // Two sequential top-level runs: neither may queue.
    let (m1, rx1) = pump
        .spawn(
            "t",
            "",
            "answer/m",
            "first",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
        )
        .await
        .unwrap();
    drop(rx1);
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_ne!(
        format!("{:?}", store.get(&m1.id).unwrap().status),
        "Queued",
        "first sequential run must not queue"
    );
    let (m2, rx2) = pump
        .spawn(
            "t",
            "",
            "answer/m",
            "second",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
        )
        .await
        .unwrap();
    drop(rx2);
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert_ne!(
        format!("{:?}", store.get(&m2.id).unwrap().status),
        "Queued",
        "second sequential run must not queue after first finished"
    );
    // Second message to the finished first thread must succeed (not "is active").
    let rx = pump
        .send_to(
            &m1.id,
            "follow up",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
        )
        .await;
    assert!(
        rx.is_ok(),
        "second message to finished thread must succeed: {rx:?}"
    );

    if let Ok(d) = parzi_core::paths::sessions_dir() {
        let _ = std::fs::remove_dir_all(d.join(&m1.id));
        let _ = std::fs::remove_dir_all(d.join(&m2.id));
    }
}
