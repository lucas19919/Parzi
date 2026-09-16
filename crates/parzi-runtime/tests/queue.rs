//! Queue mechanics: busy spawns park as Queued, pump starts them headless.

use std::sync::Arc;
use std::time::Duration;

use parzi_core::config::ParziConfig;
use parzi_core::store::{SessionStatus, SessionStore};
use parzi_runtime::Orchestrator;

/// Never emits, never closes: the run stays Active until killed.
struct HangProvider;

#[async_trait::async_trait]
impl parzi_providers::Provider for HangProvider {
    fn id(&self) -> &'static str {
        "hang"
    }
    async fn models(&self) -> parzi_core::error::Result<Vec<parzi_providers::Model>> {
        Ok(vec![])
    }
    async fn chat_stream(
        &self,
        _req: parzi_providers::ChatReq,
    ) -> parzi_core::error::Result<parzi_providers::EventRx> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        std::mem::forget(tx);
        Ok(rx)
    }
    fn auth_status(&self) -> parzi_providers::AuthStatus {
        parzi_providers::AuthStatus::Ok
    }
}

fn hang_factory(
    _id: &str,
    _cfg: &ParziConfig,
) -> parzi_core::error::Result<Box<dyn parzi_providers::Provider>> {
    Ok(Box::new(HangProvider))
}

fn test_orch(max: usize) -> (Arc<Orchestrator>, SessionStore) {
    test_home();
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = max;
    cfg.orchestrator.queue_when_busy = true;
    let store = SessionStore::open().unwrap();
    let orch = Arc::new(Orchestrator::new(cfg, store.clone()).with_factory(Arc::new(hang_factory)));
    (orch, store)
}

/// Hermetic home for this test binary (see approval_gate.rs).
fn test_home() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("parzi-test-queue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
    });
}

/// Session dirs registered here are removed on Drop — including test
/// failure by panic (assert) — so a flaky timing assert can never orphan
/// sessions into a real home again.
struct CleanupGuard {
    ids: Vec<String>,
}

impl CleanupGuard {
    fn add(&mut self, id: &str) {
        self.ids.push(id.to_string());
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if let Ok(dir) = parzi_core::paths::sessions_dir() {
            for id in &self.ids {
                let _ = std::fs::remove_dir_all(dir.join(id));
            }
        }
    }
}

async fn wait_status(store: &SessionStore, id: &str, want: SessionStatus) -> bool {
    for _ in 0..100 {
        if let Ok(m) = store.get(id) {
            if m.status == want {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn busy_spawn_queues_then_pump_starts_it() {
    let (orch, store) = test_orch(1);
    let mut guard = CleanupGuard { ids: vec![] };
    let pump = orch.clone();
    tokio::spawn(async move { pump.pump_loop().await });
    // Abandon the loop at test end via abort (test process exits anyway).
    let (first, _rx) = orch
        .spawn("t", "", "hang/model", "first", None, "", "med", vec![])
        .await
        .unwrap();
    guard.add(&first.id);
    assert!(wait_status(&store, &first.id, SessionStatus::Active).await);
    let (second, _rx2) = orch
        .spawn("t", "", "hang/model", "second", None, "", "med", vec![])
        .await
        .unwrap();
    guard.add(&second.id);
    assert!(wait_status(&store, &second.id, SessionStatus::Queued).await);
    orch.kill(&first.id).await.unwrap();
    assert!(
        wait_status(&store, &second.id, SessionStatus::Active).await,
        "pump should start the queued run after kill"
    );
    orch.kill(&second.id).await.unwrap();
}

#[tokio::test]
async fn queue_reject_mode_errors_loudly() {
    test_home();
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = 1;
    cfg.orchestrator.queue_when_busy = false;
    let store = SessionStore::open().unwrap();
    let orch = Orchestrator::new(cfg, store).with_factory(Arc::new(hang_factory));
    let mut guard = CleanupGuard { ids: vec![] };
    let (first, _rx) = orch
        .spawn("t", "", "hang/model", "first", None, "", "med", vec![])
        .await
        .unwrap();
    guard.add(&first.id);
    let err = orch
        .spawn("t", "", "hang/model", "second", None, "", "med", vec![])
        .await
        .unwrap_err();
    assert!(err.to_string().contains("busy"), "{err}");
    orch.kill(&first.id).await.unwrap();
}

/// R-5: a queued run's prompt used to live only in memory, so a restart lost
/// it and the session sat `Queued` forever. It is persisted at enqueue (with
/// its opening turn already in the transcript) and re-enqueued on boot.
#[tokio::test]
async fn queued_run_survives_a_restart() {
    let (orch, store) = test_orch(1);
    let mut guard = CleanupGuard { ids: vec![] };
    let (first, _rx) = orch
        .spawn("t", "", "hang/model", "first", None, "", "med", vec![])
        .await
        .unwrap();
    guard.add(&first.id);
    assert!(wait_status(&store, &first.id, SessionStatus::Active).await);
    let (second, _rx2) = orch
        .spawn(
            "t",
            "",
            "hang/model",
            "queued work",
            None,
            "",
            "med",
            vec![],
        )
        .await
        .unwrap();
    guard.add(&second.id);
    assert!(wait_status(&store, &second.id, SessionStatus::Queued).await);
    // The turn the run still owes is in the transcript and on disk.
    let events = store.events(&second.id).unwrap();
    assert!(
        events.iter().any(|e| matches!(
            e,
            parzi_core::store::Event::User { text } if text == "queued work"
        )),
        "the queued prompt must be recorded at enqueue: {events:?}"
    );

    // Restart: a fresh orchestrator over the same store, nothing in memory.
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = 1;
    cfg.orchestrator.queue_when_busy = true;
    let restarted =
        Arc::new(Orchestrator::new(cfg, store.clone()).with_factory(Arc::new(hang_factory)));
    // At least this one (sibling tests in this binary share the home and may
    // have runs parked at the same moment).
    assert!(restarted.recover_queue().await.unwrap() >= 1);
    let pump = restarted.clone();
    tokio::spawn(async move { pump.pump_loop().await });
    assert!(
        wait_status(&store, &second.id, SessionStatus::Active).await,
        "a re-enqueued run must start after the restart"
    );
    // And it did not say its opening turn twice.
    let users = store
        .events(&second.id)
        .unwrap()
        .iter()
        .filter(|e| matches!(e, parzi_core::store::Event::User { text } if text == "queued work"))
        .count();
    assert_eq!(
        users, 1,
        "the re-enqueued prompt must not be appended twice"
    );

    orch.kill(&first.id).await.unwrap();
    restarted.kill(&second.id).await.unwrap();
}

/// R-5: a launch that cannot build its provider must park the session (Idle)
/// and say so — it used to leave it `Queued` with nobody to tell.
#[tokio::test]
async fn launch_failure_parks_the_session_and_reports_on_the_bus() {
    test_home();
    fn broken_factory(
        _id: &str,
        _cfg: &ParziConfig,
    ) -> parzi_core::error::Result<Box<dyn parzi_providers::Provider>> {
        Err(parzi_core::error::ParziError::Store(
            "no such provider".into(),
        ))
    }
    let store = SessionStore::open().unwrap();
    let orch = Orchestrator::new(ParziConfig::default(), store.clone())
        .with_factory(Arc::new(broken_factory));
    let mut bus = orch.subscribe();
    let err = orch
        .spawn("t", "", "broken/model", "work", None, "", "med", vec![])
        .await
        .unwrap_err();
    assert!(
        !err.to_string().is_empty(),
        "the caller must hear the error"
    );
    let (sid, ev) = tokio::time::timeout(Duration::from_secs(5), bus.recv())
        .await
        .expect("the host bus must hear about a failed launch")
        .unwrap();
    assert!(
        matches!(ev, parzi_runtime::handler::RunEvent::Error(_)),
        "{ev:?}"
    );
    assert_eq!(store.get(&sid).unwrap().status, SessionStatus::Idle);
    let mut guard = CleanupGuard { ids: vec![] };
    guard.add(&sid);
}

/// R-1: a kill must land inside the stream, not after it — and `finish()`
/// must never paint `Done` over a stop the human asked for.
#[tokio::test]
async fn kill_mid_stream_stops_within_one_event_and_stays_killed() {
    test_home();
    /// Streams a chunk every 20 ms, forever.
    struct DripProvider;
    #[async_trait::async_trait]
    impl parzi_providers::Provider for DripProvider {
        fn id(&self) -> &'static str {
            "drip"
        }
        async fn models(&self) -> parzi_core::error::Result<Vec<parzi_providers::Model>> {
            Ok(vec![])
        }
        async fn chat_stream(
            &self,
            _req: parzi_providers::ChatReq,
        ) -> parzi_core::error::Result<parzi_providers::EventRx> {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                loop {
                    if tx
                        .send(Ok(parzi_providers::StreamEvent::Text("tick ".into())))
                        .is_err()
                    {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            });
            Ok(rx)
        }
        fn auth_status(&self) -> parzi_providers::AuthStatus {
            parzi_providers::AuthStatus::Ok
        }
    }
    fn drip_factory(
        _id: &str,
        _cfg: &ParziConfig,
    ) -> parzi_core::error::Result<Box<dyn parzi_providers::Provider>> {
        Ok(Box::new(DripProvider))
    }
    let store = SessionStore::open().unwrap();
    let orch = Arc::new(
        Orchestrator::new(ParziConfig::default(), store.clone())
            .with_factory(Arc::new(drip_factory)),
    );
    let mut guard = CleanupGuard { ids: vec![] };
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = counter.clone();
    let mut bus = orch.subscribe();
    tokio::spawn(async move {
        while let Ok((_id, ev)) = bus.recv().await {
            if matches!(ev, parzi_runtime::handler::RunEvent::Text(_)) {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    });
    let (meta, _rx) = orch
        .spawn("t", "", "drip/model", "stream on", None, "", "med", vec![])
        .await
        .unwrap();
    guard.add(&meta.id);
    assert!(wait_status(&store, &meta.id, SessionStatus::Active).await);
    // Let a few chunks through, then stop it.
    tokio::time::sleep(Duration::from_millis(200)).await;
    orch.kill(&meta.id).await.unwrap();
    let at_kill = counter.load(std::sync::atomic::Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(400)).await;
    let after = counter.load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        after <= at_kill + 1,
        "stream must stop within one event: {at_kill} → {after}"
    );
    assert_eq!(
        store.get(&meta.id).unwrap().status,
        SessionStatus::Killed,
        "a killed run must stay killed"
    );
}
