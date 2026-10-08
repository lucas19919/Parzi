mod common;

use std::collections::HashMap;
use std::sync::Arc;

use common::*;
use parzi_core::config::ParziConfig;
use parzi_core::store::{Event, SessionStore};
use parzi_providers::{PermissionDecision, TurnEnd};
use parzi_runtime::handler::RunSink;
use parzi_runtime::inter::{InterKind, InterSessionMessage};
use parzi_runtime::mcp::McpManager;
use parzi_runtime::toolhost::{ToolHost, ToolHostParts};
use parzi_runtime::tools::{
    is_session_tool, Approval, ApprovalMode, Approver, AskRequest, Asker, ToolCallInfo,
    ToolExecutor,
};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

struct Allow;
#[async_trait::async_trait]
impl Approver for Allow {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

struct DenyAll;
#[async_trait::async_trait]
impl Approver for DenyAll {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Deny
    }
}

fn answer() -> Arc<Fake> {
    Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.say("teamwork reply");
            Ok(TurnEnd::Completed)
        }),
    )
}

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
fn session_tools_advertised_iff_allowed() {
    for t in [
        "session.spawn",
        "session.send_message",
        "session.read_session",
        "session.list_sessions",
    ] {
        assert!(is_session_tool(t));
    }
    assert!(!is_session_tool("fs.read"));
    let open = ToolExecutor {
        cwd: String::new(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: vec!["*".into()],
    };
    assert!(open.defs().iter().any(|d| d.name == "browser.open"));
    assert!(open.defs().iter().any(|d| d.name == "session.spawn"));
    let shut = ToolExecutor {
        cwd: String::new(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: vec![],
    };
    assert!(shut.defs().iter().all(|d| d.name != "session.spawn"));
}

#[test]
fn makers_and_readers_are_advertised() {
    let open = ToolExecutor {
        cwd: String::new(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: vec!["*".into()],
    };
    for t in ["image.generate", "doc.read", "models.list", "browser.shot"] {
        assert!(
            open.defs().iter().any(|d| d.name == *t),
            "{t} must be advertised"
        );
    }
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
            None,
            None,
        )
        .await
        .unwrap(),
    );
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
            None,
            None,
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
            None,
            None,
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

#[tokio::test]
async fn a_code_run_can_spawn_a_crew() {
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
    assert!(reply.contains("ok=true"), "{reply}");
    assert!(reply.contains("child found x"), "{reply}");
    let children = store.list_children(&meta.id).unwrap();
    assert!(!children.is_empty(), "a spawn creates a child");
}

#[tokio::test]
async fn full_access_reaches_the_children() {
    home("teamwork-prop");
    let dir = std::env::temp_dir().join(format!("parzi-prop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("child.txt");
    let want = target.display().to_string();
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let want = want.clone();
            async move {
                if a.spec.prompt.contains("child writes") {
                    let input = json!({"file_path": want, "content": "hello"});
                    let decision = a.ask("Write", input, &[&want]).await;
                    if decision == PermissionDecision::Allow {
                        std::fs::write(&want, "hello").unwrap();
                        a.say("child wrote it");
                    } else {
                        a.say("child was denied");
                    }
                    return Ok(TurnEnd::Completed);
                }
                let (ok, out) = a
                    .parzi(
                        "session.spawn",
                        json!({"title": "writer", "prompt": format!("child writes {want}"), "wait": true}),
                    )
                    .await;
                a.say(&format!("parent heard: ok={ok} {out}"));
                Ok(TurnEnd::Completed)
            }
        }),
    );
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = 4;
    let (orch, store) = orch_with(cfg, &[fake]);
    let cwd = dir.display().to_string();
    let (meta, _rx) = orch
        .spawn(
            "t",
            "",
            "claude",
            "delegate the writing",
            Some(Arc::new(DenyAll)),
            &cwd,
            "low",
            vec![],
            Some("full".into()),
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    assert_eq!(
        std::fs::read_to_string(&target).unwrap_or_default(),
        "hello",
        "a Full parent's child writes without any approver involved"
    );
    let reply = last_reply(&store, &meta.id);
    assert!(reply.contains("ok=true"), "{reply}");
    let _ = std::fs::remove_dir_all(&dir);
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
            None,
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
    let evs = events(&store, &target.id);
    assert!(evs.iter().any(|e| matches!(
        e,
        Event::System { text } if InterSessionMessage::decode(text).is_some_and(|m| m.body.contains("follow up please"))
    )));
    assert!(!evs
        .iter()
        .any(|e| matches!(e, Event::User { text } if text.contains("follow up please"))));
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
            None,
            None,
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

#[tokio::test]
async fn spawn_wait_degrades_to_queued_when_slots_full() {
    let (orch, store) = team(1, hang());
    let pump = orch.clone();
    tokio::spawn(async move { pump.pump_loop().await });
    let parent = store.create("boss", "t", "", "claude/model").unwrap();
    let h = orch.harness();
    let first_id = id_of(
        &h.spawn_session(
            &parent.id,
            "one",
            "first work",
            true,
            None,
            None,
            false,
            None,
            None,
        )
        .await
        .unwrap(),
    );
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let second = h
        .spawn_session(
            &parent.id,
            "two",
            "second work",
            true,
            None,
            None,
            true,
            None,
            None,
        )
        .await
        .unwrap();
    let v: Value = serde_json::from_str(&second).unwrap();
    assert_eq!(v["status"], "queued", "{second}");
    orch.kill(&first_id).await.ok();
    orch.kill(v["session_id"].as_str().unwrap()).await.ok();
}

struct Echo(String);

#[async_trait::async_trait]
impl Asker for Echo {
    async fn ask(&self, _req: &AskRequest) -> String {
        self.0.clone()
    }
}

fn build_host(asker: Option<Arc<dyn Asker>>) -> (ToolHost, SessionStore, String) {
    home("teamwork-tools");
    let store = SessionStore::open().unwrap();
    let sid = store.create("tools", "", "", "claude/m").unwrap().id;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let folder = std::env::temp_dir()
        .join(format!("parzi-tools-{}", std::process::id()))
        .display()
        .to_string();
    std::fs::create_dir_all(&folder).unwrap();
    let host = ToolHost::new(ToolHostParts {
        session_id: sid.clone(),
        lane: "build".into(),
        mode: ApprovalMode::Auto,
        edits_auto: false,
        full: false,
        cfg: ParziConfig::default(),
        store: store.clone(),
        tools: Arc::new(ToolExecutor {
            cwd: folder,
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["*".into()],
        }),
        approver: Arc::new(Allow),
        asker,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test")),
        harness: None,
        sink: RunSink::new(&sid, tx, None),
        cancel: CancellationToken::new(),
    });
    (host, store, sid)
}

#[tokio::test]
async fn plan_write_then_read_roundtrips() {
    let (host, _store, _sid) = build_host(None);
    let (ok, out) = host
        .call(
            "plan.write",
            &json!({
                "goal": "ship v1",
                "decisions": [{"decision": "sqlite", "why": "single file"}],
                "steps": [{"title": "migrate", "status": "doing"}],
            }),
        )
        .await;
    assert!(ok, "{out}");
    assert!(out.contains("1 steps"), "{out}");
    let (ok, back) = host.call("plan.read", &json!({})).await;
    assert!(ok, "{back}");
    assert!(
        back.contains("ship v1") && back.contains("sqlite"),
        "{back}"
    );
}

#[tokio::test]
async fn ask_user_returns_the_answer_or_says_so() {
    let (host, _store, _sid) = build_host(Some(Arc::new(Echo("blue".into()))));
    let (ok, out) = host
        .call(
            "ask.user",
            &json!({"question": "which?", "options": ["blue"]}),
        )
        .await;
    assert!(ok && out == "blue", "{out}");

    let (lonely, _store, _sid) = build_host(None);
    let (ok, out) = lonely
        .call("ask.user", &json!({"question": "which?"}))
        .await;
    assert!(ok && out.contains("no one to ask"), "{out}");
}

#[tokio::test]
async fn project_create_makes_a_home() {
    let (host, store, sid) = build_host(None);
    let folder = std::env::temp_dir()
        .join(format!("parzi-proj-{}", std::process::id()))
        .join("demo");
    let (ok, out) = host
        .call(
            "project.create",
            &json!({"title": "Demo", "folder": folder.display().to_string()}),
        )
        .await;
    assert!(ok, "{out}");
    assert!(folder.is_dir(), "the folder exists");
    let _ = std::fs::remove_dir_all(folder.parent().unwrap());
    let meta = store.get(&sid).unwrap();
    assert_eq!(
        meta.cwd,
        folder.display().to_string(),
        "session moved there"
    );
}
