// Its own test binary (and so its own PARZI_HOME): a restart re-enqueues
// every queued session on disk, which would pick up other tests' queues.
mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use parzi_core::config::ParziConfig;
use parzi_core::store::SessionStatus;
use parzi_providers::{PermissionDecision, TurnEnd};
use parzi_runtime::status::StatusBoard;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};
use parzi_runtime::Orchestrator;

struct Counting(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl Approver for Counting {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        self.0.fetch_add(1, Ordering::SeqCst);
        Approval::Allow
    }
}

fn asking(max: usize) -> ParziConfig {
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = max;
    cfg.orchestrator.queue_when_busy = true;
    cfg.lanes.default_mode = "ask".into();
    cfg
}

#[tokio::test]
async fn a_restored_run_asks_through_the_hosts_approver() {
    home("restore-approver");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            if a.spec.prompt.contains("hold the slot") {
                a.cancel.cancelled().await;
                return Ok(TurnEnd::Interrupted);
            }
            let input = serde_json::json!({"file_path": "notes.txt", "content": "hi"});
            let decision = a.ask("Write", input, &["notes.txt"]).await;
            a.say(if decision == PermissionDecision::Allow {
                "write allowed"
            } else {
                "write denied"
            });
            Ok(TurnEnd::Completed)
        }),
    );
    let before = Arc::new(AtomicUsize::new(0));
    let (orch, store) = orch_with(asking(1), std::slice::from_ref(&fake));
    let (first, _rx) = orch
        .spawn(
            "t",
            "claude",
            "hold the slot",
            Some(Arc::new(Counting(before.clone()))),
            "",
            "medium",
            vec![],
            None,
        )
        .await
        .unwrap();
    assert!(wait_status(&store, &first.id, SessionStatus::Active).await);
    let (second, _rx2) = orch
        .spawn(
            "t",
            "claude",
            "write the notes",
            Some(Arc::new(Counting(before.clone()))),
            "",
            "medium",
            vec![],
            Some("supervised".into()),
        )
        .await
        .unwrap();
    assert!(wait_status(&store, &second.id, SessionStatus::Queued).await);

    // The app restarts: the queue comes back from disk, and the desk
    // installs its approver before the pump starts.
    let asked = Arc::new(AtomicUsize::new(0));
    let restarted = Arc::new(
        Orchestrator::new(asking(8), store.clone())
            .with_source(source(&[fake]))
            .with_status(Arc::new(StatusBoard::in_memory())),
    );
    restarted.set_approver(Arc::new(Counting(asked.clone())));
    assert_eq!(restarted.recover_queue().await.unwrap(), 1);
    let pump = restarted.clone();
    tokio::spawn(async move { pump.pump_loop().await });
    orch.kill(&first.id).await.unwrap();
    settle(&store, &second.id).await;
    assert_eq!(
        last_reply(&store, &second.id),
        "write allowed",
        "the restored run asked the desk instead of failing closed"
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(before.load(Ordering::SeqCst), 0);
}
