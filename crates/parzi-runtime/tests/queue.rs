//! Over the concurrency cap, runs queue (or refuse loudly), survive a
//! restart, park on a failed launch, and stop within one event when killed.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use parzi_core::config::ParziConfig;
use parzi_core::store::{Event, SessionStatus};
use parzi_providers::{Provider, TurnEnd};
use parzi_runtime::handler::RunEvent;
use parzi_runtime::status::StatusBoard;
use parzi_runtime::Orchestrator;

/// Holds its slot until killed.
fn hang() -> Arc<Fake> {
    Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.cancel.cancelled().await;
            Ok(TurnEnd::Interrupted)
        }),
    )
}

fn capped(max: usize, queue: bool) -> ParziConfig {
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = max;
    cfg.orchestrator.queue_when_busy = queue;
    cfg
}

#[tokio::test]
async fn busy_spawn_queues_then_pump_starts_it() {
    home("queue");
    let (orch, store) = orch_with(capped(1, true), &[hang()]);
    let pump = orch.clone();
    tokio::spawn(async move { pump.pump_loop().await });
    let (first, _rx) = orch.spawn("t", "", "claude", "first", None, "", "medium", vec![], None).await.unwrap();
    assert!(wait_status(&store, &first.id, SessionStatus::Active).await);
    let (second, _rx2) = orch.spawn("t", "", "claude", "second", None, "", "medium", vec![], None).await.unwrap();
    assert!(wait_status(&store, &second.id, SessionStatus::Queued).await);
    orch.kill(&first.id).await.unwrap();
    assert!(
        wait_status(&store, &second.id, SessionStatus::Active).await,
        "the pump starts the queued run once a slot frees"
    );
    orch.kill(&second.id).await.unwrap();
}

#[tokio::test]
async fn queue_reject_mode_errors_loudly() {
    home("queue");
    let (orch, _store) = orch_with(capped(1, false), &[hang()]);
    let (first, _rx) = orch.spawn("t", "", "claude", "first", None, "", "medium", vec![], None).await.unwrap();
    let err = orch
        .spawn("t", "", "claude", "second", None, "", "medium", vec![], None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("busy"), "{err}");
    orch.kill(&first.id).await.unwrap();
}

/// R-5: a queued prompt is on disk with its opening turn, and a restarted
/// orchestrator starts it without writing that turn twice.
#[tokio::test]
async fn queued_run_survives_a_restart() {
    home("queue");
    let fake = hang();
    let (orch, store) = orch_with(capped(1, true), &[fake.clone()]);
    let (first, _rx) = orch.spawn("t", "", "claude", "first", None, "", "medium", vec![], None).await.unwrap();
    assert!(wait_status(&store, &first.id, SessionStatus::Active).await);
    let (second, _rx2) = orch
        .spawn("t", "", "claude", "queued work", None, "", "medium", vec![], None)
        .await
        .unwrap();
    assert!(wait_status(&store, &second.id, SessionStatus::Queued).await);
    assert!(events(&store, &second.id)
        .iter()
        .any(|e| matches!(e, Event::User { text } if text == "queued work")));

    // Sibling tests share this home and may have runs parked too: the
    // restarted orchestrator gets room for all of them.
    let restarted = Arc::new(
        Orchestrator::new(capped(8, true), store.clone())
            .with_source(source(&[fake]))
            .with_status(Arc::new(StatusBoard::in_memory())),
    );
    assert!(restarted.recover_queue().await.unwrap() >= 1);
    let pump = restarted.clone();
    tokio::spawn(async move { pump.pump_loop().await });
    orch.kill(&first.id).await.unwrap();
    assert!(
        wait_status(&store, &second.id, SessionStatus::Active).await,
        "a re-enqueued run starts after the restart"
    );
    let users = events(&store, &second.id)
        .iter()
        .filter(|e| matches!(e, Event::User { text } if text == "queued work"))
        .count();
    assert_eq!(users, 1, "the opening turn is written once");
    restarted.kill(&second.id).await.unwrap();
}

/// R-5: a launch that cannot get its provider parks the session (Idle) and
/// says so on the host bus.
#[tokio::test]
async fn launch_failure_parks_the_session_and_reports_on_the_bus() {
    home("queue");
    let (orch, store) = orch_with(ParziConfig::default(), &[]);
    let mut bus = orch.subscribe();
    let err = orch
        .spawn("t", "", "claude", "work", None, "", "medium", vec![], None)
        .await
        .unwrap_err();
    assert!(!err.to_string().is_empty());
    let (sid, ev) = tokio::time::timeout(Duration::from_secs(5), bus.recv())
        .await
        .expect("the host bus hears about a failed launch")
        .unwrap();
    assert!(matches!(ev, RunEvent::Error(_)), "{ev:?}");
    assert_eq!(store.get(&sid).unwrap().status, SessionStatus::Idle);
}

/// R-1: a kill lands inside the stream, and a killed run stays killed.
#[tokio::test]
async fn kill_mid_stream_stops_within_one_event_and_stays_killed() {
    home("queue");
    let drip = Fake::new(
        "claude",
        script(|a: Agent| async move {
            loop {
                if a.cancel.is_cancelled() {
                    return Ok(TurnEnd::Interrupted);
                }
                let _ = a.events.send(parzi_providers::ProviderEvent::TextDelta("tick ".into()));
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }),
    );
    let (orch, store) = orch(&[drip]);
    let counter = Arc::new(AtomicUsize::new(0));
    let seen = counter.clone();
    let mut bus = orch.subscribe();
    tokio::spawn(async move {
        while let Ok((_id, ev)) = bus.recv().await {
            if matches!(ev, RunEvent::Text(_)) {
                seen.fetch_add(1, Ordering::SeqCst);
            }
        }
    });
    let (meta, _rx) = orch.spawn("t", "", "claude", "stream on", None, "", "medium", vec![], None).await.unwrap();
    assert!(wait_status(&store, &meta.id, SessionStatus::Active).await);
    tokio::time::sleep(Duration::from_millis(200)).await;
    orch.kill(&meta.id).await.unwrap();
    let at_kill = counter.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(400)).await;
    let after = counter.load(Ordering::SeqCst);
    assert!(after <= at_kill + 1, "the stream stops within one event: {at_kill} → {after}");
    assert_eq!(store.get(&meta.id).unwrap().status, SessionStatus::Killed);
    // What streamed before the stop is kept.
    assert!(events(&store, &meta.id)
        .iter()
        .any(|e| matches!(e, Event::Assistant { text, done: false } if text.contains("tick"))));
}

/// A provider the roster knows but this build cannot start is a launch
/// failure, not a hang.
#[test]
fn an_empty_source_has_no_providers() {
    let src = source(&[]);
    assert!(src("claude", &ParziConfig::default()).map(|p: Arc<dyn Provider>| p.id()).is_none());
}
