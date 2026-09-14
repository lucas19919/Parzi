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
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = max;
    cfg.orchestrator.queue_when_busy = true;
    // Isolate: point the store at a temp home via env is racy; instead rely
    // on unique titles and clean up our own sessions afterwards.
    let store = SessionStore::open().unwrap();
    let orch = Arc::new(
        Orchestrator::new(cfg, store.clone())
            .with_factory(Arc::new(hang_factory)),
    );
    (orch, store)
}

async fn wait_status(
    store: &SessionStore,
    id: &str,
    want: SessionStatus,
) -> bool {
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
    let pump = orch.clone();
    tokio::spawn(async move { pump.pump_loop().await });
    // Abandon the loop at test end via abort (test process exits anyway).
    let (first, _rx) = orch
        .spawn("t", "", "hang/model", "first", None, "", "med", vec![])
        .await
        .unwrap();
    assert!(wait_status(&store, &first.id, SessionStatus::Active).await);
    let (second, _rx2) = orch
        .spawn("t", "", "hang/model", "second", None, "", "med", vec![])
        .await
        .unwrap();
    assert!(wait_status(&store, &second.id, SessionStatus::Queued).await);
    orch.kill(&first.id).await.unwrap();
    assert!(
        wait_status(&store, &second.id, SessionStatus::Active).await,
        "pump should start the queued run after kill"
    );
    orch.kill(&second.id).await.unwrap();
    cleanup(&[&first.id, &second.id]);
}

fn cleanup(ids: &[&str]) {
    if let Ok(dir) = parzi_core::paths::sessions_dir() {
        for id in ids {
            let _ = std::fs::remove_dir_all(dir.join(id));
        }
    }
}

#[tokio::test]
async fn queue_reject_mode_errors_loudly() {
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = 1;
    cfg.orchestrator.queue_when_busy = false;
    let store = SessionStore::open().unwrap();
    let orch = Orchestrator::new(cfg, store).with_factory(Arc::new(hang_factory));
    let (first, _rx) = orch
        .spawn("t", "", "hang/model", "first", None, "", "med", vec![])
        .await
        .unwrap();
    let err = orch
        .spawn("t", "", "hang/model", "second", None, "", "med", vec![])
        .await
        .unwrap_err();
    assert!(err.to_string().contains("busy"), "{err}");
    orch.kill(&first.id).await.unwrap();
    cleanup(&[&first.id]);
}
