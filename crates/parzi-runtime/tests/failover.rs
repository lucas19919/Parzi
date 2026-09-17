//! Failover: an explicit pick that 429s hops to the next slot with a
//! `RouteTransition` signal (live event + persisted transcript event).
//!
//! Hermetic by construction: slots are handed to `AgentRun` directly, so the
//! hop logic never depends on ambient sign-ins (the router chain, which does
//! need live credentials, is covered by the catalog/router unit tests).

use std::sync::Arc;

use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Event, SessionStore};
use parzi_providers::{AuthStatus, ChatReq, EventRx, Model, Provider, StreamEvent};
use parzi_runtime::handler::{AgentRun, ProviderSlot, RunEvent};
use parzi_runtime::mcp::McpManager;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo, ToolExecutor};

/// Hermetic home for this test binary (see approval_gate.rs): real sign-ins
/// must never leak into routing, and test sessions must never land in the
/// user's sidebar.
static TEST_HOME_INIT: std::sync::Once = std::sync::Once::new();

fn test_home() {
    TEST_HOME_INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("parzi-test-failover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
    });
}

struct DenyAll;
#[async_trait::async_trait]
impl Approver for DenyAll {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Deny
    }
}

/// First slot: always 429s.
struct FlakyProvider;

#[async_trait::async_trait]
impl Provider for FlakyProvider {
    fn id(&self) -> &'static str {
        "flaky"
    }
    async fn models(&self) -> Result<Vec<Model>> {
        Ok(vec![])
    }
    async fn chat_stream(&self, _req: ChatReq) -> Result<EventRx> {
        Err(ParziError::Provider(
            "flaky".into(),
            "http 429: rate limit, retry after 1".into(),
        ))
    }
    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Ok
    }
}

/// Every other slot: answers with one text chunk, then ends the turn.
struct GoodProvider {
    id: &'static str,
}

#[async_trait::async_trait]
impl Provider for GoodProvider {
    fn id(&self) -> &'static str {
        self.id
    }
    async fn models(&self) -> Result<Vec<Model>> {
        Ok(vec![])
    }
    async fn chat_stream(&self, _req: ChatReq) -> Result<EventRx> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let _ = tx.send(Ok(StreamEvent::Text("hello from fallback".into())));
        Ok(rx)
    }
    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Ok
    }
}

#[tokio::test]
async fn explicit_pick_429_hops_with_route_transition() {
    test_home();
    let store = SessionStore::open().unwrap();
    let meta = store.create("hop", "t", "", "flaky/any-model").unwrap();
    let sid = meta.id.clone();

    let tools = Arc::new(ToolExecutor {
        cwd: String::new(),
        mcp: Arc::new(McpManager::new(std::collections::HashMap::new(), 60)),
        allowed: vec![],
        leases: None,
    });
    let slots = vec![
        ProviderSlot {
            provider_id: "flaky".into(),
            provider: Box::new(FlakyProvider),
            model_id: "any-model".into(),
            price_in: 0.0,
            price_out: 0.0,
            reason: "explicit pick",
        },
        ProviderSlot {
            provider_id: "good".into(),
            provider: Box::new(GoodProvider { id: "good" }),
            model_id: "any-model".into(),
            price_in: 0.0,
            price_out: 0.0,
            reason: "fallback",
        },
    ];
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let run = AgentRun::new(
        sid.clone(),
        slots,
        vec![],
        "test".into(),
        parzi_runtime::tools::ApprovalMode::Auto,
        false,
        4,
        4096,
        vec![],
        "low".into(),
        store.clone(),
        tools,
        Arc::new(DenyAll),
        tx,
        tokio_util::sync::CancellationToken::new(),
    );

    let run_handle = tokio::spawn(async move {
        run.run("hello").await.unwrap();
    });
    let mut hop: Option<(String, String, String)> = None;
    let mut done = false;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        while let Some(ev) = rx.recv().await {
            match ev {
                RunEvent::RouteTransition {
                    from_provider,
                    to_provider,
                    reason,
                    ..
                } => {
                    hop = Some((from_provider, to_provider, reason));
                }
                RunEvent::Done { .. } => {
                    done = true;
                    break;
                }
                RunEvent::Error(e) => panic!("run should hop, not die: {e}"),
                _ => {}
            }
        }
    })
    .await;
    let _ = run_handle.await;
    assert!(outcome.is_ok(), "run did not finish within 60s");
    assert!(done, "expected Done after failover hop");

    let (from, to, reason) = hop.expect("expected a RouteTransition live event");
    assert_eq!(from, "flaky");
    assert_ne!(to, "flaky");
    assert!(reason.contains("rate limit"), "unexpected reason: {reason}");

    // The hop is also persisted in the transcript for audit.
    let events = store.events(&meta.id).unwrap();
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::RouteTransition { from_provider, .. } if from_provider == "flaky"
        )),
        "transcript missing RouteTransition"
    );

    if let Ok(dir) = parzi_core::paths::sessions_dir() {
        let _ = std::fs::remove_dir_all(dir.join(&meta.id));
    }
}

#[tokio::test]
async fn strict_mode_single_slot_halts_on_429() {
    test_home();
    let store = SessionStore::open().unwrap();
    let meta = store.create("strict", "t", "", "flaky/any-model").unwrap();
    let sid = meta.id.clone();

    let tools = Arc::new(ToolExecutor {
        cwd: String::new(),
        mcp: Arc::new(McpManager::new(std::collections::HashMap::new(), 60)),
        allowed: vec![],
        leases: None,
    });
    let slots = vec![ProviderSlot {
        provider_id: "flaky".into(),
        provider: Box::new(FlakyProvider),
        model_id: "any-model".into(),
        price_in: 0.0,
        price_out: 0.0,
        reason: "explicit pick",
    }];
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let run = AgentRun::new(
        sid.clone(),
        slots,
        vec![],
        "test".into(),
        parzi_runtime::tools::ApprovalMode::Auto,
        false,
        4,
        4096,
        vec![],
        "low".into(),
        store.clone(),
        tools,
        Arc::new(DenyAll),
        tx,
        tokio_util::sync::CancellationToken::new(),
    );

    let run_handle = tokio::spawn(async move {
        let _ = run.run("hello").await;
    });
    let mut saw_hop = false;
    let mut saw_error = false;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        while let Some(ev) = rx.recv().await {
            match ev {
                RunEvent::RouteTransition { .. } => saw_hop = true,
                RunEvent::Error(_) => {
                    saw_error = true;
                    break;
                }
                RunEvent::Done { .. } => break,
                _ => {}
            }
        }
    })
    .await;
    let _ = run_handle.await;
    assert!(outcome.is_ok(), "run did not finish within 60s");
    assert!(!saw_hop, "strict mode must not hop providers");
    assert!(saw_error, "single-slot 429 must surface an error");

    if let Ok(dir) = parzi_core::paths::sessions_dir() {
        let _ = std::fs::remove_dir_all(dir.join(&meta.id));
    }
}
