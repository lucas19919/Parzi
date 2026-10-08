mod common;

use std::collections::HashMap;
use std::sync::Arc;

use common::*;
use parzi_core::config::ParziConfig;
use parzi_core::store::SessionStore;
use parzi_providers::{PermissionDecision, PermissionGate, PermissionRequest};
use parzi_runtime::handler::RunSink;
use parzi_runtime::mcp::McpManager;
use parzi_runtime::toolhost::{ToolHost, ToolHostParts};
use parzi_runtime::tools::{Approval, ApprovalMode, Approver, ToolCallInfo, ToolExecutor};
use serde_json::json;
use tokio_util::sync::CancellationToken;

struct Allow;
#[async_trait::async_trait]
impl Approver for Allow {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

fn host(cwd: &str) -> ToolHost {
    home("shell-tools");
    let store = SessionStore::open().unwrap();
    let sid = store.create("shell", "", "", "claude/m").unwrap().id;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: sid.clone(),
        lane: "build".into(),
        mode: ApprovalMode::Auto,
        edits_auto: false,
        full: false,
        cfg: ParziConfig::default(),
        store,
        tools: Arc::new(ToolExecutor {
            cwd: cwd.to_string(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["*".into()],
        }),
        approver: Arc::new(Allow),
        asker: None,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test-shell")),
        harness: None,
        sink: RunSink::new(&sid, tx, None),
        cancel: CancellationToken::new(),
    })
}

fn workdir() -> String {
    let dir = std::env::temp_dir().join(format!("parzi-shell-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.display().to_string()
}

#[tokio::test]
async fn foreground_echo_comes_back() {
    let h = host(&workdir());
    let (ok, out) = h
        .call("shell.exec", &json!({"cmd": "echo shell-owned"}))
        .await;
    assert!(ok, "{out}");
    assert!(out.contains("shell-owned"), "{out}");
}

#[tokio::test]
async fn foreground_failure_reports_its_exit() {
    let h = host(&workdir());
    let (ok, out) = h
        .call(
            "shell.exec",
            &json!({"cmd": "exit 3", "timeout_ms": 10_000}),
        )
        .await;
    assert!(ok, "the tool ran; the command failed: {out}");
    assert!(out.contains("exit 3"), "{out}");
}

#[tokio::test]
async fn background_start_logs_kill() {
    let h = host(&workdir());
    let sleep = if cfg!(windows) {
        "Start-Sleep -Seconds 30"
    } else {
        "sleep 30"
    };
    let (ok, out) = h
        .call("shell.start", &json!({"cmd": sleep, "title": "sleeper"}))
        .await;
    assert!(ok, "{out}");
    let id = out
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    assert!(id.starts_with("sh-"), "{out}");
    let (ok, out) = h.call("shell.logs", &json!({"id": id})).await;
    assert!(ok, "{out}");
    assert!(out.contains("running"), "{out}");
    let (ok, out) = h.call("shell.kill", &json!({"id": id})).await;
    assert!(ok, "{out}");
    assert!(out.contains("killed"), "{out}");
}

#[tokio::test]
async fn workdir_outside_the_folder_is_refused() {
    let h = host(&workdir());
    let away = if cfg!(windows) {
        "C:\\Windows\\Temp"
    } else {
        "/tmp"
    };
    let (ok, out) = h
        .call("shell.exec", &json!({"cmd": "echo hi", "workdir": away}))
        .await;
    assert!(!ok, "outside the fence must not run: {out}");
    assert!(out.contains("outside"), "{out}");
}

#[tokio::test]
async fn vendor_shell_is_rerouted_not_carded() {
    let h = host(&workdir());
    for tool in ["Bash", "bash", "shell", "execute", "run_command"] {
        let d = h
            .decide(PermissionRequest {
                id: "req-1".into(),
                tool: tool.into(),
                title: tool.into(),
                input: json!({}),
                paths: vec![],
            })
            .await;
        assert!(
            matches!(d, PermissionDecision::Deny(_)),
            "{tool} must be refused"
        );
        if let PermissionDecision::Deny(why) = d {
            assert!(why.contains("shell.exec"), "{tool}: {why}");
        }
    }
}

#[tokio::test]
async fn research_offers_no_shell() {
    home("shell-research");
    let store = SessionStore::open().unwrap();
    let sid = store.create("shell", "", "", "claude/m").unwrap().id;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let host = ToolHost::new(ToolHostParts {
        session_id: sid.clone(),
        lane: "work".into(),
        mode: ApprovalMode::Ask,
        edits_auto: false,
        full: false,
        cfg: ParziConfig::default(),
        store,
        tools: Arc::new(ToolExecutor {
            cwd: workdir(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["*".into()],
        }),
        approver: Arc::new(Allow),
        asker: None,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test-shell")),
        harness: None,
        sink: RunSink::new(&sid, tx, None),
        cancel: CancellationToken::new(),
    });
    let (ok, _) = host.call("shell.exec", &json!({"cmd": "echo hi"})).await;
    assert!(!ok, "work must not run commands");
    let d = host
        .decide(PermissionRequest {
            id: "req-1".into(),
            tool: "Bash".into(),
            title: "Bash".into(),
            input: json!({}),
            paths: vec![],
        })
        .await;
    assert!(
        matches!(d, PermissionDecision::Deny(_)),
        "work denies vendor shell too"
    );
}
