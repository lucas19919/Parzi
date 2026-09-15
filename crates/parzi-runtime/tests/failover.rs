//! Failover: an explicit pick that 429s hops to the next slot with a
//! `RouteTransition` signal (live event + persisted transcript event).

use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Event, SessionStore};
use parzi_providers::{AuthStatus, ChatReq, EventRx, Model, Provider, StreamEvent};
use parzi_runtime::handler::RunEvent;
use parzi_runtime::Orchestrator;

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

fn failover_factory(id: &str, _cfg: &ParziConfig) -> Result<Box<dyn Provider>> {
    if id == "flaky" {
        Ok(Box::new(FlakyProvider))
    } else {
        // Leak the id string: the factory seam is sync and the provider
        // only needs a stable `id()` for the duration of the run.
        let id: &'static str = Box::leak(id.to_string().into_boxed_str());
        Ok(Box::new(GoodProvider { id }))
    }
}

#[tokio::test]
async fn explicit_pick_429_hops_with_route_transition() {
    let mut cfg = ParziConfig::default();
    cfg.routing.auto_failover = true;
    cfg.routing.auto_order = vec![];
    let store = SessionStore::open().unwrap();
    let orch = Orchestrator::new(cfg, store.clone()).with_factory(Arc::new(failover_factory));

    let (meta, mut rx) = orch
        .spawn("t", "", "flaky/any-model", "hello", None, "", "med", vec![])
        .await
        .unwrap();

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
    let mut cfg = ParziConfig::default();
    cfg.routing.auto_failover = false;
    let store = SessionStore::open().unwrap();
    let orch = Orchestrator::new(cfg, store.clone()).with_factory(Arc::new(failover_factory));

    let (meta, mut rx) = orch
        .spawn("t", "", "flaky/any-model", "hello", None, "", "med", vec![])
        .await
        .unwrap();

    let mut saw_hop = false;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        while let Some(ev) = rx.recv().await {
            match ev {
                RunEvent::RouteTransition { .. } => saw_hop = true,
                RunEvent::Done { .. } | RunEvent::Error(_) => break,
                _ => {}
            }
        }
    })
    .await;
    assert!(outcome.is_ok(), "run did not finish within 60s");
    assert!(!saw_hop, "strict mode must not hop providers");

    if let Ok(dir) = parzi_core::paths::sessions_dir() {
        let _ = std::fs::remove_dir_all(dir.join(&meta.id));
    }
}
