//! B1: Ask mode waits for the human — a slow approver that answers Allow
//! lets the action happen exactly once, never an instant deny. B4: finished
//! runs release their slot. H-5: Parzi's session tools obey the lane
//! allowlist. And the agent's own actions pass the same gate.

mod common;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use parzi_core::config::ParziConfig;
use parzi_core::store::{Event, SessionStatus};
use parzi_providers::{PermissionDecision, PermissionGate, PermissionRequest, TurnEnd};
use parzi_runtime::handler::RunSink;
use parzi_runtime::mcp::McpManager;
use parzi_runtime::toolhost::{ToolHost, ToolHostParts};
use parzi_runtime::tools::{Approval, ApprovalMode, Approver, ToolCallInfo, ToolExecutor};
use serde_json::json;

struct SlowAllow(AtomicUsize);
#[async_trait::async_trait]
impl Approver for SlowAllow {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        self.0.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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

struct Allow;
#[async_trait::async_trait]
impl Approver for Allow {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

/// An agent that asks to write `gate.txt`, writes it only when allowed,
/// and says what happened.
fn writer() -> Arc<Fake> {
    Fake::new(
        "claude",
        script(|a: Agent| async move {
            let path = a.spec.cwd.join("gate.txt");
            let input = json!({"file_path": path.display().to_string(), "content": "hello"});
            let decision = a
                .ask("Write", input.clone(), &[&path.display().to_string()])
                .await;
            let (ok, _) = a
                .own_tool("w1", "Write", input, || async {
                    match decision {
                        PermissionDecision::Allow | PermissionDecision::AllowAlways => {
                            std::fs::write(&path, "hello").unwrap();
                            (true, "written".to_string())
                        }
                        PermissionDecision::Deny(why) => (false, format!("denied: {why}")),
                    }
                })
                .await;
            a.say(if ok { "done" } else { "could not write" });
            Ok(TurnEnd::Completed)
        }),
    )
}

#[tokio::test]
async fn ask_mode_waits_for_a_slow_approver_and_deny_denies() {
    home("approval");
    let dir = std::env::temp_dir().join(format!("parzi-gate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cwd = dir.display().to_string();
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "ask".into();
    let (orch, store) = orch_with(cfg, &[writer()]);

    let slow = Arc::new(SlowAllow(AtomicUsize::new(0)));
    let t0 = std::time::Instant::now();
    let (meta, _rx) = orch
        .spawn(
            "t",
            "",
            "claude",
            "write the file",
            Some(slow.clone()),
            &cwd,
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    let done = settle(&store, &meta.id).await;
    assert_eq!(done.status, SessionStatus::Done);
    assert!(
        t0.elapsed() >= std::time::Duration::from_millis(400),
        "asked instantly"
    );
    assert_eq!(
        slow.0.load(Ordering::SeqCst),
        1,
        "the person was asked once"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("gate.txt")).unwrap(),
        "hello"
    );

    std::fs::remove_file(dir.join("gate.txt")).unwrap();
    let (meta, _rx) = orch
        .spawn(
            "t",
            "",
            "claude",
            "write the file",
            Some(Arc::new(DenyAll)),
            &cwd,
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    assert!(
        !dir.join("gate.txt").exists(),
        "a denial must stop the write"
    );
    let denied = events(&store, &meta.id).iter().any(
        |e| matches!(e, Event::ToolResult { ok: false, output, .. } if output.contains("denied")),
    );
    assert!(denied, "the transcript shows the denial");
    let _ = std::fs::remove_dir_all(&dir);
}

/// B4: sequential finished runs never queue, and a finished thread takes a
/// second message.
#[tokio::test]
async fn sequential_completed_runs_release_slots() {
    home("approval");
    let answer = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.say("hi");
            Ok(TurnEnd::Completed)
        }),
    );
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = 1;
    let (orch, store) = orch_with(cfg, &[answer]);
    let p = orch.clone();
    tokio::spawn(async move { p.pump_loop().await });
    let (m1, _) = orch
        .spawn(
            "t",
            "",
            "claude",
            "first",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    assert_ne!(settle(&store, &m1.id).await.status, SessionStatus::Queued);
    let (m2, _) = orch
        .spawn(
            "t",
            "",
            "claude",
            "second",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    assert_eq!(settle(&store, &m2.id).await.status, SessionStatus::Done);
    let again = orch
        .send_to(
            &m1.id,
            "follow up",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
            None,
        )
        .await;
    assert!(
        again.is_ok(),
        "a finished thread takes a second message: {again:?}"
    );
}

fn host(allowed: Vec<String>, mode: ApprovalMode) -> ToolHost {
    let store = parzi_core::store::SessionStore::open().unwrap();
    let meta = store.create("gate", "t", "", "claude").unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: meta.id.clone(),
        lane: "test".into(),
        mode,
        edits_auto: false,
        store,
        tools: Arc::new(ToolExecutor {
            cwd: std::env::temp_dir().display().to_string(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed,
            leases: None,
        }),
        approver: Arc::new(Allow),
        harness: Some(Arc::new(NoHarness)),
        role: None,
        sink: RunSink::new(&meta.id, tx, None),
        cancel: tokio_util::sync::CancellationToken::new(),
    })
}

struct NoHarness;
#[async_trait::async_trait]
impl parzi_runtime::handler::HarnessBridge for NoHarness {
    async fn spawn_session(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: bool,
        _: Option<String>,
        _: Option<String>,
        _: bool,
    ) -> parzi_core::error::Result<String> {
        Ok("spawned".into())
    }
    async fn send_message(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: parzi_core::context::InterKind,
        _: bool,
    ) -> parzi_core::error::Result<String> {
        Ok("sent".into())
    }
    async fn read_session(
        &self,
        _: &str,
        _: &str,
        _: Option<usize>,
    ) -> parzi_core::error::Result<String> {
        Ok("read".into())
    }
    async fn list_sessions(&self, _: &str, _: bool) -> parzi_core::error::Result<String> {
        Ok("[]".into())
    }
}

/// H-5: a lane without `session.*` cannot list other sessions.
#[tokio::test]
async fn session_tools_obey_the_lane_allowlist() {
    home("approval");
    let without = host(vec!["plan.read".into()], ApprovalMode::Auto);
    let (ok, out) = without.call("session.list_sessions", &json!({})).await;
    assert!(!ok, "not in the allowlist, not run: {out}");
    let with = host(vec!["session.*".into()], ApprovalMode::Auto);
    let (ok, out) = with.call("session.list_sessions", &json!({})).await;
    assert!(ok, "{out}");
}

fn request(tool: &str) -> PermissionRequest {
    PermissionRequest {
        id: "r1".into(),
        tool: tool.into(),
        title: tool.into(),
        input: json!({"command": "ls"}),
        paths: vec![],
    }
}

/// The agent's own actions: a lane that lists file and shell kinds refuses
/// the kinds it leaves out; a lane that lists none leaves them to the mode;
/// lockdown lets reads through and nothing else.
#[tokio::test]
async fn the_agents_own_actions_pass_the_lane_and_the_mode() {
    home("approval");
    let reads_only = host(
        vec!["fs.read".into(), "ui.show_widget".into()],
        ApprovalMode::Auto,
    );
    assert!(matches!(
        reads_only.decide(request("Bash")).await,
        PermissionDecision::Deny(_)
    ));
    assert_eq!(
        reads_only.decide(request("Grep")).await,
        PermissionDecision::Allow
    );
    let no_kinds = host(vec!["session.*".into()], ApprovalMode::Auto);
    assert_eq!(
        no_kinds.decide(request("Bash")).await,
        PermissionDecision::Allow
    );
    let locked = host(vec!["*".into()], ApprovalMode::Deny);
    assert!(matches!(
        locked.decide(request("Edit")).await,
        PermissionDecision::Deny(_)
    ));
    assert_eq!(
        locked.decide(request("Read")).await,
        PermissionDecision::Allow
    );
    // Parzi's own tools pass Parzi's gate when they run, so the vendor's
    // permission ask for them is waved through, even locked down.
    assert_eq!(
        locked.decide(request("mcp__parzi__ui_show_widget")).await,
        PermissionDecision::Allow
    );
}

/// Markdown with an ASCII-box diagram renders as a dead console window: the
/// render tool refuses it and names the tools that draw it properly.
#[tokio::test]
async fn markdown_with_an_ascii_diagram_is_refused() {
    home("approval");
    let h = host(vec![], ApprovalMode::Auto);
    let boxes = "```ascii\n+------+------+\n|  api |  web |\n+------+------+\n```";
    let (ok, out) = h
        .call("ui.show_markdown", &json!({"markdown": boxes}))
        .await;
    assert!(!ok && out.contains("ui.show_diagram"), "{out}");
    let (ok, out) = h
        .call("ui.show_markdown", &json!({"markdown": "Plain **text**."}))
        .await;
    assert!(ok, "{out}");
}
