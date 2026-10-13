// Its own test binary (and so its own PARZI_HOME): `parzi serve` holds a
// lock per home and re-enqueues every queued session on disk.
//
// A run queued with someone to ask comes back after a restart under
// `parzi serve`, and asks through serve's ParkedApprover (a client answers
// on the socket) instead of being denied every gated call.
mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use parzi_core::config::ParziConfig;
use parzi_core::store::SessionStatus;
use parzi_providers::{PermissionDecision, TurnEnd};
use parzi_runtime::osserve;
use parzi_runtime::status::StatusBoard;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};
use parzi_runtime::Orchestrator;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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

async fn rpc(port: u16, token: &str, op: &str, mut body: Value) -> Value {
    body["op"] = json!(op);
    body["token"] = json!(token);
    let mut line = serde_json::to_string(&body).unwrap();
    line.push('\n');
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let (read, mut write) = stream.into_split();
    write.write_all(line.as_bytes()).await.unwrap();
    write.shutdown().await.unwrap();
    let mut reader = BufReader::new(read);
    let mut buf = String::new();
    reader.read_line(&mut buf).await.unwrap();
    serde_json::from_str(&buf).unwrap()
}

#[tokio::test]
async fn a_restored_run_under_serve_asks_through_the_socket() {
    let dir = home("serve-restore");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            if a.spec.prompt.contains("hold the slot") {
                a.cancel.cancelled().await;
                return Ok(TurnEnd::Interrupted);
            }
            let input = json!({"file_path": "notes.txt", "content": "hi"});
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
            "",
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
            "",
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
    // The desk closes for good: the first run goes, the queue stays on disk.
    orch.kill(&first.id).await.unwrap();
    drop(orch);

    // `parzi serve` starts on the same home and picks the queue up.
    let served = Arc::new(
        Orchestrator::new(asking(8), store.clone())
            .with_source(source(&[fake]))
            .with_status(Arc::new(StatusBoard::in_memory())),
    );
    let daemon = osserve::start(served).await.expect("serve starts");
    let meta: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("serve.json")).unwrap()).unwrap();
    let token = meta["token"].as_str().unwrap().to_string();
    let key = {
        let mut found = None;
        for _ in 0..400 {
            let page = rpc(daemon.port, &token, "approval.list", json!({})).await;
            let rows = page["approvals"].as_array().cloned().unwrap_or_default();
            if let Some(row) = rows
                .iter()
                .find(|r| r["session"].as_str() == Some(second.id.as_str()))
            {
                found = row["key"].as_str().map(str::to_string);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        found.expect("the restored run parked its approval on the socket")
    };
    let answered = rpc(
        daemon.port,
        &token,
        "approval.answer",
        json!({ "key": key, "allow": true }),
    )
    .await;
    assert_eq!(answered["ok"], json!(true));
    settle(&store, &second.id).await;
    assert_eq!(
        last_reply(&store, &second.id),
        "write allowed",
        "the restored run asked a serve client instead of failing closed"
    );
    assert_eq!(
        before.load(Ordering::SeqCst),
        0,
        "the old desk approver is gone"
    );
    drop(daemon);
}
