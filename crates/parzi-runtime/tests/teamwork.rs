//! Teamwork: `session.*` harness tools — spawn (subsession vs full session),
//! cross-session messaging, and inspection. Uses an answering test provider
//! so `wait: true` exercises the full child-run loop.

use std::collections::HashMap;
use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_core::error::Result;
use parzi_core::store::SessionStore;
use parzi_providers::{AuthStatus, ChatReq, EventRx, Model, Provider, StreamEvent};
use parzi_runtime::handler::HarnessBridge;
use parzi_runtime::mcp::McpManager;
use parzi_runtime::tools::{ToolExecutor, is_session_tool, session_defs};
use parzi_runtime::Orchestrator;

/// Answers every turn with one fixed text chunk, then ends the turn.
struct AnswerProvider;

#[async_trait::async_trait]
impl Provider for AnswerProvider {
    fn id(&self) -> &'static str {
        "answer"
    }
    async fn models(&self) -> Result<Vec<Model>> {
        Ok(vec![])
    }
    async fn chat_stream(&self, _req: ChatReq) -> Result<EventRx> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let _ = tx.send(Ok(StreamEvent::Text("teamwork reply".into())));
        Ok(rx)
    }
    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Ok
    }
}

fn answer_factory(
    _id: &str,
    _cfg: &ParziConfig,
) -> Result<Box<dyn Provider>> {
    Ok(Box::new(AnswerProvider))
}

fn test_orch(max: usize) -> (Arc<Orchestrator>, SessionStore) {
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = max;
    cfg.orchestrator.queue_when_busy = true;
    let store = SessionStore::open().unwrap();
    let orch = Arc::new(Orchestrator::new(cfg, store.clone()).with_factory(Arc::new(answer_factory)));
    (orch, store)
}

fn cleanup(ids: &[&str]) {
    if let Ok(dir) = parzi_core::paths::sessions_dir() {
        for id in ids {
            let _ = std::fs::remove_dir_all(dir.join(id));
        }
    }
}

#[test]
fn session_tools_advertised_and_recognized() {
    let names: Vec<String> = session_defs().into_iter().map(|d| d.name).collect();
    for t in [
        "session.spawn",
        "session.send_message",
        "session.read_session",
        "session.list_sessions",
    ] {
        assert!(names.contains(&t.to_string()), "missing def {t}");
        assert!(is_session_tool(t));
    }
    assert!(!is_session_tool("fs.read"));
    // Included in the default executor surface every provider advertises.
    let e = ToolExecutor {
        cwd: String::new(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: vec!["*".into()],
    };
    let defs = e.defs();
    assert!(defs.iter().any(|d| d.name == "session.spawn"));
}

#[tokio::test]
async fn spawn_subsession_nests_but_full_session_stays_top_level() {
    let (orch, store) = test_orch(4);
    let parent = store.create("boss", "t", "", "answer/model").unwrap();
    let h = orch.harness();

    let sub_json = h
        .spawn_session(&parent.id, "research", "look into x", true, None, None, false)
        .await
        .unwrap();
    let sub: serde_json::Value = serde_json::from_str(&sub_json).unwrap();
    let sub_id = sub["session_id"].as_str().unwrap().to_string();
    // Default model: inherits the parent's.
    assert_eq!(store.get(&sub_id).unwrap().model, "answer/model");
    assert_eq!(
        store.get(&sub_id).unwrap().parent_id.as_deref(),
        Some(parent.id.as_str())
    );
    assert!(store.list_children(&parent.id).unwrap().iter().any(|m| m.id == sub_id));

    let full_json = h
        .spawn_session(&parent.id, "side quest", "do y", false, Some("answer/other".into()), None, false)
        .await
        .unwrap();
    let full: serde_json::Value = serde_json::from_str(&full_json).unwrap();
    let full_id = full["session_id"].as_str().unwrap().to_string();
    assert_eq!(store.get(&full_id).unwrap().parent_id, None);

    orch.kill(&sub_id).await.ok();
    orch.kill(&full_id).await.ok();
    cleanup(&[&parent.id, &sub_id, &full_id]);
}

#[tokio::test]
async fn spawn_wait_collects_child_reply() {
    let (orch, store) = test_orch(4);
    let parent = store.create("boss", "t", "", "answer/model").unwrap();
    let h = orch.harness();

    let out = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        h.spawn_session(&parent.id, "research", "look into x", true, None, None, true),
    )
    .await
    .expect("spawn wait timed out")
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["status"], "done");
    assert!(
        v["response"].as_str().unwrap().contains("teamwork reply"),
        "parent should receive the child output: {out}"
    );

    let sub_id = v["session_id"].as_str().unwrap().to_string();
    cleanup(&[&parent.id, &sub_id]);
}

#[tokio::test]
async fn send_message_continues_target_and_can_wait() {
    let (orch, store) = test_orch(4);
    let parent = store.create("boss", "t", "", "answer/model").unwrap();
    let target = store.create("worker", "t", "", "answer/model").unwrap();
    let h = orch.harness();

    let out = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        h.send_message(&parent.id, &target.id, "follow up please", true),
    )
    .await
    .expect("send_message wait timed out")
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        v["response"].as_str().unwrap().contains("teamwork reply"),
        "expected the target's reply: {out}"
    );
    // The message landed in the target transcript.
    let events = store.events(&target.id).unwrap();
    assert!(events.iter().any(|e| matches!(
        e,
        parzi_core::store::Event::User { text } if text.contains("follow up please")
    )));

    cleanup(&[&parent.id, &target.id]);
}

#[tokio::test]
async fn read_and_list_inspect_sessions() {
    let (orch, store) = test_orch(4);
    let parent = store.create("boss", "t", "", "answer/model").unwrap();
    let h = orch.harness();
    let sub_json = h
        .spawn_session(&parent.id, "research", "look into x", true, None, None, false)
        .await
        .unwrap();
    let sub_id: String = serde_json::from_str::<serde_json::Value>(&sub_json)
        .unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let read = h.read_session(&sub_id, Some(10)).await.unwrap();
    assert!(read.contains("research"), "{read}");

    let kids = h.list_sessions(&parent.id, true).await.unwrap();
    assert!(kids.contains(&sub_id), "{kids}");
    let all = h.list_sessions(&parent.id, false).await.unwrap();
    assert!(all.contains(&parent.id), "{all}");

    orch.kill(&sub_id).await.ok();
    cleanup(&[&parent.id, &sub_id]);
}

#[tokio::test]
async fn spawn_wait_degrades_to_queued_when_slots_full() {
    // max=1 with the parent holding the only slot: a blocking wait would
    // deadlock, so the bridge must return `queued` instead.
    let (orch, store) = test_orch(1);
    let pump = orch.clone();
    tokio::spawn(async move { pump.pump_loop().await });
    // Occupy the single slot with a hanging run is overkill: the bridge
    // counts live handles, so spawn a first session via the harness with
    // wait=false, then a wait=true spawn must come back queued.
    let parent = store.create("boss", "t", "", "answer/model").unwrap();
    let h = orch.harness();
    let first = h
        .spawn_session(&parent.id, "one", "first work", true, None, None, false)
        .await
        .unwrap();
    let first_id: String = serde_json::from_str::<serde_json::Value>(&first)
        .unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    // Give the first child a moment to take the slot.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let second = h
        .spawn_session(&parent.id, "two", "second work", true, None, None, true)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&second).unwrap();
    assert_eq!(v["status"], "queued", "{second}");
    let second_id = v["session_id"].as_str().unwrap().to_string();

    orch.kill(&first_id).await.ok();
    orch.kill(&second_id).await.ok();
    cleanup(&[&parent.id, &first_id, &second_id]);
}
