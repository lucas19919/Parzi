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
                        PermissionDecision::Allow => {
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
    host_with(allowed, mode, ParziConfig::default())
}

fn host_with(allowed: Vec<String>, mode: ApprovalMode, cfg: ParziConfig) -> ToolHost {
    let store = parzi_core::store::SessionStore::open().unwrap();
    let meta = store.create("gate", "t", "claude").unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: meta.id.clone(),
        mode,
        edits_auto: false,
        full: false,
        cfg,
        store,
        tools: Arc::new(ToolExecutor {
            cwd: std::env::temp_dir().display().to_string(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed,
        }),
        approver: Arc::new(Allow),
        asker: None,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test")),
        harness: Some(Arc::new(NoHarness)),
        sink: RunSink::new(&meta.id, tx, None),
        cancel: tokio_util::sync::CancellationToken::new(),
    })
}

struct NoHarness;
#[async_trait::async_trait]
impl parzi_runtime::handler::HarnessBridge for NoHarness {
    async fn spawn(
        &self,
        _: &str,
        _: parzi_runtime::handler::SpawnSpec,
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
        _: Option<String>,
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

#[tokio::test]
async fn session_tools_obey_the_allowlist() {
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
        name: String::new(),
        id: "r1".into(),
        tool: tool.into(),
        title: tool.into(),
        input: json!({"command": "ls"}),
        paths: vec![],
    }
}

#[tokio::test]
async fn the_agents_own_actions_pass_the_allowlist_and_the_mode() {
    home("approval");
    let reads_only = host(
        vec!["fs.read".into(), "ui.show_artifact".into()],
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
    let mut off = ParziConfig::default();
    off.lanes.no_shell = true;
    let no_shell = host_with(vec!["session.*".into()], ApprovalMode::Auto, off);
    assert!(matches!(
        no_shell.decide(request("Bash")).await,
        PermissionDecision::Deny(_)
    ));
    assert_eq!(
        no_shell.decide(request("Grep")).await,
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
    assert_eq!(
        locked.decide(request("mcp__parzi__ui_show_artifact")).await,
        PermissionDecision::Allow
    );
}

fn auto_host(
    cwd: &str,
    approver: Arc<dyn Approver>,
    harness: Arc<dyn parzi_runtime::handler::HarnessBridge>,
) -> ToolHost {
    let store = parzi_core::store::SessionStore::open().unwrap();
    let meta = store.create("auto", "t", "claude").unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: meta.id.clone(),
        mode: ApprovalMode::Auto,
        edits_auto: false,
        full: false,
        cfg: parzi_core::config::ParziConfig::default(),
        store,
        tools: Arc::new(ToolExecutor {
            cwd: cwd.to_string(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["*".into()],
        }),
        approver,
        asker: None,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test")),
        harness: Some(harness),
        sink: RunSink::new(&meta.id, tx, None),
        cancel: tokio_util::sync::CancellationToken::new(),
    })
}

#[tokio::test]
async fn project_create_outside_the_folder_asks() {
    home("approval");
    let base = std::env::temp_dir().join(format!("parzi-proj-{}", std::process::id()));
    let (cwd, away) = (base.join("here"), base.join("away").join("paper"));
    std::fs::create_dir_all(&cwd).unwrap();
    let projects = base.join("projects");
    std::env::set_var("PARZI_PROJECTS", &projects);
    let cwd_s = cwd.display().to_string();
    let host = auto_host(&cwd_s, Arc::new(DenyAll), Arc::new(NoHarness));
    let (ok, out) = host
        .call(
            "project.create",
            &json!({"title": "Away", "folder": away.display().to_string()}),
        )
        .await;
    assert!(!ok, "{out}");
    assert!(out.contains("declined"), "{out}");
    assert!(!away.exists(), "nothing is created without a yes");
    let (ok, out) = host
        .call(
            "project.create",
            &json!({"title": "Paper", "folder": "papers/./one/../paper"}),
        )
        .await;
    assert!(ok, "{out}");
    assert!(
        projects.join("papers").join("paper").is_dir(),
        "a bare name lands beside the other projects, not in the session's folder: {out}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[tokio::test]
async fn moving_to_another_project_needs_a_yes() {
    home("approval");
    let base = std::env::temp_dir().join(format!("parzi-switch-{}", std::process::id()));
    let elsewhere = base.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let folder = elsewhere.display().to_string();
    let project = parzi_core::brain::project_upsert(None, "Elsewhere", &folder).unwrap();
    let here = base.display().to_string();
    let store = parzi_core::store::SessionStore::open().unwrap();
    let in_project = |store: &parzi_core::store::SessionStore| {
        store
            .list()
            .unwrap()
            .into_iter()
            .filter(|m| m.project == project.slug && m.cwd == folder)
            .count()
    };

    let refused = auto_host(&here, Arc::new(DenyAll), Arc::new(NoHarness));
    let args = json!({"slug": project.slug});
    let (ok, out) = refused.call("project.switch", &args).await;
    assert!(!ok && out.contains("declined"), "{out}");
    assert_eq!(in_project(&store), 0, "nothing moves without a yes");

    let unknown = auto_host(&here, Arc::new(Allow), Arc::new(NoHarness));
    let (ok, out) = unknown
        .call("project.switch", &json!({"slug": "no-such-project"}))
        .await;
    assert!(!ok && out.contains("no project"), "{out}");

    let allowed = auto_host(&here, Arc::new(Allow), Arc::new(NoHarness));
    let (ok, out) = allowed.call("project.switch", &args).await;
    assert!(ok, "{out}");
    assert_eq!(in_project(&store), 1, "the session moved: {out}");
    let _ = std::fs::remove_dir_all(&base);
}

#[tokio::test]
async fn grok_reaches_parzis_tools_through_its_use_tool() {
    home("approval");
    let ask = host(vec!["*".into()], ApprovalMode::Ask);
    let wrapped = |inner: &str| PermissionRequest {
        name: String::new(),
        id: "r2".into(),
        tool: "other".into(),
        title: "use_tool".into(),
        input: json!({"tool_name": inner, "tool_input": {}}),
        paths: vec![],
    };
    assert_eq!(
        ask.decide(wrapped("parzi__ui_propose_task")).await,
        PermissionDecision::Allow
    );
    let direct = PermissionRequest {
        name: String::new(),
        title: "parzi__todo_write".into(),
        input: json!({}),
        ..wrapped("")
    };
    assert_eq!(ask.decide(direct).await, PermissionDecision::Allow);
    let locked = host(vec!["*".into()], ApprovalMode::Deny);
    assert!(matches!(
        locked.decide(wrapped("github__create_issue")).await,
        PermissionDecision::Deny(_)
    ));
}

#[tokio::test]
async fn its_own_scratch_folder_is_the_sessions_to_write_in() {
    home("approval");
    let store = parzi_core::store::SessionStore::open().unwrap();
    let meta = store.create("scratch", "t", "claude").unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let work = ToolHost::new(ToolHostParts {
        session_id: meta.id.clone(),
        mode: ApprovalMode::Ask,
        edits_auto: false,
        full: false,
        cfg: ParziConfig::default(),
        store,
        tools: Arc::new(ToolExecutor {
            cwd: std::env::temp_dir().display().to_string(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["*".into()],
        }),
        approver: Arc::new(DenyAll),
        asker: None,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test")),
        harness: Some(Arc::new(NoHarness)),
        sink: RunSink::new(&meta.id, tx, None),
        cancel: tokio_util::sync::CancellationToken::new(),
    });
    let scratch = parzi_core::paths::scratch_dir(&meta.id).unwrap();
    std::fs::create_dir_all(&scratch).unwrap();
    let write = |path: String| PermissionRequest {
        name: String::new(),
        id: "w".into(),
        tool: "Write".into(),
        title: "Write".into(),
        input: json!({"file_path": path}),
        paths: vec![],
    };
    let inside = scratch.join("notes.md").display().to_string();
    assert_eq!(work.decide(write(inside)).await, PermissionDecision::Allow);
    let elsewhere = std::env::temp_dir()
        .join("elsewhere.md")
        .display()
        .to_string();
    assert!(matches!(
        work.decide(write(elsewhere)).await,
        PermissionDecision::Deny(_)
    ));
    assert!(matches!(
        work.decide(write("notes.md".into())).await,
        PermissionDecision::Deny(_)
    ));
}
