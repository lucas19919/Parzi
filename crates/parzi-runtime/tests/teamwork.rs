//! Teamwork: `session.*` — spawn (subsession vs full session), messages
//! across sessions, inspection — through the bridge and, end to end, from
//! an agent calling Parzi's tools over MCP.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use common::*;
use parzi_core::config::ParziConfig;
use parzi_core::store::Event;
use parzi_providers::TurnEnd;
use parzi_runtime::inter::{InterKind, InterSessionMessage};
use parzi_runtime::mcp::McpManager;
use parzi_runtime::tools::{
    is_session_tool, session_defs, Approval, Approver, ToolCallInfo, ToolExecutor,
};
use serde_json::{json, Value};

struct Allow;
#[async_trait::async_trait]
impl Approver for Allow {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

/// Answers every turn with a fixed reply.
fn answer() -> Arc<Fake> {
    Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.say("teamwork reply");
            Ok(TurnEnd::Completed)
        }),
    )
}

/// Holds its run slot until killed.
fn hang() -> Arc<Fake> {
    Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.cancel.cancelled().await;
            Ok(TurnEnd::Interrupted)
        }),
    )
}

fn team(
    max: usize,
    fake: Arc<Fake>,
) -> (
    Arc<parzi_runtime::Orchestrator>,
    parzi_core::store::SessionStore,
) {
    home("teamwork");
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = max;
    cfg.lanes.default_mode = "auto".into();
    orch_with(cfg, &[fake])
}

fn id_of(json: &str) -> String {
    serde_json::from_str::<Value>(json).unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string()
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
    let e = ToolExecutor {
        cwd: String::new(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: vec!["*".into()],
        leases: None,
    };
    assert!(e.defs().iter().any(|d| d.name == "session.spawn"));
}

#[tokio::test]
async fn spawn_subsession_nests_but_full_session_stays_top_level() {
    let (orch, store) = team(4, answer());
    let parent = store.create("boss", "t", "", "claude/model").unwrap();
    let h = orch.harness();
    let sub_id = id_of(
        &h.spawn_session(
            &parent.id,
            "research",
            "look into x",
            true,
            None,
            None,
            false,
        )
        .await
        .unwrap(),
    );
    // Default model: the parent's.
    assert_eq!(store.get(&sub_id).unwrap().model, "claude/model");
    assert_eq!(
        store.get(&sub_id).unwrap().parent_id.as_deref(),
        Some(parent.id.as_str())
    );
    assert!(store
        .list_children(&parent.id)
        .unwrap()
        .iter()
        .any(|m| m.id == sub_id));
    let full_id = id_of(
        &h.spawn_session(
            &parent.id,
            "side quest",
            "do y",
            false,
            Some("claude/other".into()),
            None,
            false,
        )
        .await
        .unwrap(),
    );
    assert_eq!(store.get(&full_id).unwrap().parent_id, None);
    orch.kill(&sub_id).await.ok();
    orch.kill(&full_id).await.ok();
}

#[tokio::test]
async fn spawn_wait_collects_child_reply() {
    let (orch, store) = team(4, answer());
    let parent = store.create("boss", "t", "", "claude/model").unwrap();
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        orch.harness().spawn_session(
            &parent.id,
            "research",
            "look into x",
            true,
            None,
            None,
            true,
        ),
    )
    .await
    .expect("spawn wait timed out")
    .unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["status"], "done");
    assert!(
        v["response"].as_str().unwrap().contains("teamwork reply"),
        "{out}"
    );
}

/// End to end: an agent delegates with `session.spawn` over MCP, waits,
/// and gets the child's answer back as the tool result.
#[tokio::test]
async fn an_agent_delegates_over_mcp_and_gets_the_answer() {
    home("teamwork");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            if a.spec.prompt.contains("look into x") {
                a.say("child found x");
                return Ok(TurnEnd::Completed);
            }
            let (ok, out) = a
                .parzi(
                    "session.spawn",
                    json!({"title": "research", "prompt": "look into x", "wait": true}),
                )
                .await;
            a.say(&format!("parent heard: ok={ok} {out}"));
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(&[fake]);
    let (meta, _rx) = orch
        .spawn(
            "t",
            "",
            "claude",
            "delegate the research",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let reply = last_reply(&store, &meta.id);
    assert!(
        reply.contains("ok=true") && reply.contains("child found x"),
        "{reply}"
    );
    let children = store.list_children(&meta.id).unwrap();
    assert_eq!(children.len(), 1, "the child nests under the parent");
    // The call shows in the parent's thread under Parzi's own name.
    assert!(events(&store, &meta.id)
        .iter()
        .any(|e| matches!(e, Event::ToolCall { name, .. } if name == "session.spawn")));
}

#[tokio::test]
async fn send_message_continues_target_and_can_wait() {
    let fake = answer();
    let (orch, store) = team(4, fake.clone());
    let parent = store.create("boss", "t", "", "claude/model").unwrap();
    let target = store.create("worker", "t", "", "claude/model").unwrap();
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        orch.harness().send_message(
            &parent.id,
            &target.id,
            "follow up please",
            InterKind::Text,
            true,
        ),
    )
    .await
    .expect("send_message wait timed out")
    .unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert!(
        v["response"].as_str().unwrap().contains("teamwork reply"),
        "{out}"
    );
    // H-5: in the transcript as typed data, never as a user turn…
    let evs = events(&store, &target.id);
    assert!(evs.iter().any(|e| matches!(
        e,
        Event::System { text } if InterSessionMessage::decode(text).is_some_and(|m| m.body.contains("follow up please"))
    )));
    assert!(!evs
        .iter()
        .any(|e| matches!(e, Event::User { text } if text.contains("follow up please"))));
    // …and to the agent in its untrusted wrapping.
    let prompt = fake.seen().last().unwrap().prompt.clone();
    assert!(
        prompt.contains("follow up please") && prompt.contains("untrusted"),
        "{prompt}"
    );
}

#[tokio::test]
async fn read_and_list_inspect_sessions() {
    let (orch, store) = team(4, answer());
    let parent = store.create("boss", "t", "", "claude/model").unwrap();
    let h = orch.harness();
    let sub_id = id_of(
        &h.spawn_session(
            &parent.id,
            "research",
            "look into x",
            true,
            None,
            None,
            false,
        )
        .await
        .unwrap(),
    );
    let read = h.read_session(&parent.id, &sub_id, Some(10)).await.unwrap();
    assert!(read.contains("research"), "{read}");
    assert!(h
        .list_sessions(&parent.id, true)
        .await
        .unwrap()
        .contains(&sub_id));
    assert!(h
        .list_sessions(&parent.id, false)
        .await
        .unwrap()
        .contains(&parent.id));
    orch.kill(&sub_id).await.ok();
}

/// max=1 with a hanging child holding the only slot: a blocking wait would
/// deadlock, so the bridge answers `queued` instead.
#[tokio::test]
async fn spawn_wait_degrades_to_queued_when_slots_full() {
    let (orch, store) = team(1, hang());
    let pump = orch.clone();
    tokio::spawn(async move { pump.pump_loop().await });
    let parent = store.create("boss", "t", "", "claude/model").unwrap();
    let h = orch.harness();
    let first_id = id_of(
        &h.spawn_session(&parent.id, "one", "first work", true, None, None, false)
            .await
            .unwrap(),
    );
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let second = h
        .spawn_session(&parent.id, "two", "second work", true, None, None, true)
        .await
        .unwrap();
    let v: Value = serde_json::from_str(&second).unwrap();
    assert_eq!(v["status"], "queued", "{second}");
    orch.kill(&first_id).await.ok();
    orch.kill(v["session_id"].as_str().unwrap()).await.ok();
}
