//! Live-engine bridge between the egui UI thread and the agent runtime.
//!
//! Phase 2 (`docs/native-ui/PLAN.md` M2): a background tokio `Runtime` (two
//! workers) owns an `Arc<Orchestrator>`. Each run's `RunEvent` stream is
//! forwarded onto a crossbeam channel the UI thread drains once per frame,
//! with token deltas coalesced over [`COALESCE_WINDOW`]. After each forwarded
//! batch the bridge invokes a repaint callback built from an `egui::Context`
//! ([`repaint_callback`], safe from any thread). This module never touches
//! `eframe`: app wiring lives in `app.rs` (follow-up).
//!
//! Approvals are a SINGLE path (AUDIT B1): [`UiApprover`] implements the
//! runtime [`Approver`](parzi_runtime::tools::Approver) trait by pushing the
//! call plus its oneshot responder into the approvals queue and awaiting the
//! UI's answer with a deny-on-timeout. There is deliberately no second
//! channel: while the card is open, the agent task blocks here.

use std::sync::Arc;
use std::time::Duration;

use parzi_runtime::handler::RunEvent;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};
use parzi_runtime::Orchestrator;

/// Deny-by-default approval timeout: an unanswered card must never stall a
/// run forever. Matches the contract the CLI-side approvers rely on.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);
/// Token deltas arriving within one window merge into a single UI event, so a
/// fast stream wakes the UI at ~30 fps instead of once per token.
pub const COALESCE_WINDOW: Duration = Duration::from_millis(30);

/// Thread-safe repaint hook. Built on the UI thread from its context:
/// `DeskBridge::new(orch, repaint_callback(&cc.egui_ctx))`.
pub type RepaintCallback = Box<dyn Fn() + Send + Sync>;

/// Build a [`RepaintCallback`] from an egui context. `request_repaint` is
/// documented as safe to call from any thread.
pub fn repaint_callback(ctx: &egui::Context) -> RepaintCallback {
    let ctx = ctx.clone();
    Box::new(move || ctx.request_repaint())
}

/// Key of a pending approval: the tool-call id, unique within its run.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApprovalKey(pub String);

/// One event for the UI thread, tagged with the session it belongs to.
#[derive(Debug)]
pub struct UiEvent {
    pub session_id: String,
    pub event: RunEvent,
}

/// The UI side of the single approval path: what the approval card renders
/// and answers. Dropping it without answering denies (fail closed).
pub struct PendingApproval {
    pub key: ApprovalKey,
    pub call: ToolCallInfo,
    responder: Option<tokio::sync::oneshot::Sender<Approval>>,
}

impl PendingApproval {
    /// Answer the waiting agent task. Consumes the card: each tool call runs
    /// exactly once, after exactly one answer (AUDIT B1).
    pub fn answer(mut self, decision: Approval) {
        if let Some(tx) = self.responder.take() {
            let _ = tx.send(decision);
        }
    }
}

impl std::fmt::Debug for PendingApproval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingApproval")
            .field("key", &self.key)
            .field("call", &self.call)
            .finish()
    }
}

/// Coalesces consecutive token deltas into one buffer. Pure and unit-tested;
/// the forwarder below is a thin async shell around it.
#[derive(Debug, Default)]
pub struct TextCoalescer {
    buf: String,
}

impl TextCoalescer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, delta: String) {
        self.buf.push_str(&delta);
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Take the buffered text, if any, leaving an empty buffer behind.
    pub fn take(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.buf))
        }
    }
}

/// The runtime's single approval gate, backed by the UI card queue.
///
/// `approve` pushes a [`PendingApproval`] into the queue the UI thread drains
/// and awaits the oneshot inside it: `Allow`/`Deny` from the card, `Deny`
/// when the queue is gone (UI tearing down), the card is dropped, or
/// [`APPROVAL_TIMEOUT`] elapses. The timeout is configurable for tests via
/// [`UiApprover::with_timeout`]; production always uses 120 s.
#[derive(Clone)]
pub struct UiApprover {
    queue: crossbeam_channel::Sender<PendingApproval>,
    timeout: Duration,
}

impl UiApprover {
    pub fn new(queue: crossbeam_channel::Sender<PendingApproval>) -> Self {
        Self {
            queue,
            timeout: APPROVAL_TIMEOUT,
        }
    }

    pub fn with_timeout(
        queue: crossbeam_channel::Sender<PendingApproval>,
        timeout: Duration,
    ) -> Self {
        Self { queue, timeout }
    }
}

impl Approver for UiApprover {
    // Hand-written future: `parzi-desk` carries no `async-trait` dep, so the
    // impl mirrors the macro expansion's early-bound lifetimes exactly.
    fn approve<'life0, 'life1, 'async_trait>(
        &'life0 self,
        call: &'life1 ToolCallInfo,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Approval> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let call = call.clone();
        let queue = self.queue.clone();
        let timeout = self.timeout;
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let pending = PendingApproval {
                key: ApprovalKey(call.id.clone()),
                call,
                responder: Some(tx),
            };
            // UI gone: fail closed, never hang the agent task.
            if queue.send(pending).is_err() {
                return Approval::Deny;
            }
            match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(decision)) => decision,
                // Timeout elapsed, or the card was dropped unanswered.
                _ => Approval::Deny,
            }
        })
    }
}

/// Owns the background runtime plus both UI queues. Constructed once by
/// `app.rs`; shared onwards via the handles it hands out.
pub struct DeskBridge {
    rt: tokio::runtime::Runtime,
    orchestrator: Arc<Orchestrator>,
    events_tx: crossbeam_channel::Sender<UiEvent>,
    events_rx: crossbeam_channel::Receiver<UiEvent>,
    approvals_tx: crossbeam_channel::Sender<PendingApproval>,
    approvals_rx: crossbeam_channel::Receiver<PendingApproval>,
    repaint: Arc<RepaintCallback>,
}

impl DeskBridge {
    /// Two workers host the orchestrator, exactly as `src-tauri` does today.
    pub fn new(
        orchestrator: Arc<Orchestrator>,
        repaint: RepaintCallback,
    ) -> std::io::Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let (events_tx, events_rx) = crossbeam_channel::unbounded();
        let (approvals_tx, approvals_rx) = crossbeam_channel::unbounded();
        Ok(Self {
            rt,
            orchestrator,
            events_tx,
            events_rx,
            approvals_tx,
            approvals_rx,
            repaint: repaint.into(),
        })
    }

    pub fn orchestrator(&self) -> &Arc<Orchestrator> {
        &self.orchestrator
    }

    /// Approver to hand to `Orchestrator::spawn` / `send_to` for each run.
    pub fn approver(&self) -> UiApprover {
        UiApprover::new(self.approvals_tx.clone())
    }

    /// Drain forwarded run events. Called once per frame by `app.rs`.
    pub fn drain_events(&self) -> Vec<UiEvent> {
        self.events_rx.try_iter().collect()
    }

    /// Drain cards waiting for an answer. Each needs exactly one
    /// [`PendingApproval::answer`]: the agent task blocks until then (or the
    /// 120 s deny-timeout fires).
    pub fn drain_approvals(&self) -> Vec<PendingApproval> {
        self.approvals_rx.try_iter().collect()
    }

    /// Forward one run's live channel into the UI queue. Token deltas
    /// coalesce over [`COALESCE_WINDOW`]; any other event flushes pending
    /// text first (approximate order) and wakes the UI via the repaint hook.
    /// A closed channel flushes the tail so the transcript loses nothing.
    pub fn track_run(
        &self,
        session_id: String,
        rx: tokio::sync::mpsc::UnboundedReceiver<RunEvent>,
    ) {
        let tx = self.events_tx.clone();
        let repaint = self.repaint.clone();
        self.rt.spawn(async move {
            let mut rx = rx;
            let mut tick = tokio::time::interval(COALESCE_WINDOW);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut coalescer = TextCoalescer::new();
            loop {
                tokio::select! {
                    biased;
                    msg = rx.recv() => {
                        let Some(ev) = msg else { break };
                        if let RunEvent::Text(delta) = ev {
                            coalescer.push(delta);
                        } else {
                            if let Some(text) = coalescer.take() {
                                let _ = tx.send(UiEvent {
                                    session_id: session_id.clone(),
                                    event: RunEvent::Text(text),
                                });
                            }
                            let _ = tx.send(UiEvent {
                                session_id: session_id.clone(),
                                event: ev,
                            });
                            repaint();
                        }
                    }
                    _ = tick.tick() => {
                        if let Some(text) = coalescer.take() {
                            let _ = tx.send(UiEvent {
                                session_id: session_id.clone(),
                                event: RunEvent::Text(text),
                            });
                            repaint();
                        }
                    }
                }
            }
            if let Some(text) = coalescer.take() {
                let _ = tx.send(UiEvent {
                    session_id: session_id.clone(),
                    event: RunEvent::Text(text),
                });
            }
            repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_call(id: &str) -> ToolCallInfo {
        ToolCallInfo {
            id: id.into(),
            name: "shell.exec".into(),
            // Inferred `serde_json::Value` without naming the crate, which
            // `parzi-desk` does not depend on directly.
            args: Default::default(),
            lane: "build".into(),
        }
    }

    // No `tokio::test` macro: `parzi-desk` enables only
    // `rt-multi-thread/sync/time`, so tests drive a local runtime instead.
    fn block_on<F>(fut: F) -> F::Output
    where
        F: std::future::Future,
    {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(fut)
    }

    #[test]
    fn coalescer_merges_deltas_and_tracks_emptiness() {
        let mut c = TextCoalescer::new();
        assert!(c.is_empty());
        assert_eq!(c.take(), None);
        c.push("Hello, ".to_string());
        c.push("world".to_string());
        assert!(!c.is_empty());
        assert_eq!(c.take().as_deref(), Some("Hello, world"));
        assert!(c.is_empty());
        assert_eq!(c.take(), None);
    }

    #[test]
    fn approver_denies_when_the_ui_is_gone() {
        let (tx, rx) = crossbeam_channel::unbounded();
        drop(rx); // UI thread torn down: send fails, fail closed.
        let approver = UiApprover::with_timeout(tx, Duration::from_millis(10));
        assert!(matches!(
            block_on(approver.approve(&test_call("t1"))),
            Approval::Deny
        ));
    }

    #[test]
    fn approver_denies_on_timeout_and_queues_exactly_one_card() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let approver = UiApprover::with_timeout(tx, Duration::from_millis(20));
        // Receiver alive (send succeeds) but nobody answers.
        assert!(matches!(
            block_on(approver.approve(&test_call("t2"))),
            Approval::Deny
        ));
        // Single path: exactly one card, keyed by the tool-call id.
        let cards: Vec<_> = rx.try_iter().collect();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].key, ApprovalKey("t2".into()));
    }

    #[test]
    fn approver_forwards_the_cards_answer() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let approver = UiApprover::with_timeout(tx, Duration::from_secs(10));
        let agent = std::thread::spawn(move || {
            let call = test_call("t3");
            block_on(approver.approve(&call))
        });
        let card = rx.recv_timeout(Duration::from_secs(5)).expect("card queued");
        assert_eq!(card.key, ApprovalKey("t3".into()));
        card.answer(Approval::Allow);
        assert!(matches!(agent.join().expect("agent task"), Approval::Allow));
    }

    #[test]
    fn dropped_card_denies() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let approver = UiApprover::with_timeout(tx, Duration::from_secs(10));
        let agent = std::thread::spawn(move || {
            let call = test_call("t4");
            block_on(approver.approve(&call))
        });
        let card = rx.recv_timeout(Duration::from_secs(5)).expect("card queued");
        drop(card); // dismissed without answering: fail closed.
        assert!(matches!(agent.join().expect("agent task"), Approval::Deny));
    }
}
