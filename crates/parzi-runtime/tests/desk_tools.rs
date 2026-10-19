mod common;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use parzi_providers::{PermissionDecision, PermissionGate, PermissionRequest};
use parzi_runtime::handler::RunSink;
use parzi_runtime::mcp::McpManager;
use parzi_runtime::toolhost::{ToolHost, ToolHostParts};
use parzi_runtime::tools::{Approval, ApprovalMode, Approver, ToolCallInfo, ToolExecutor};
use serde_json::json;

/// Says no, and counts how often it was asked.
#[derive(Default)]
struct Counter(AtomicUsize);
#[async_trait::async_trait]
impl Approver for Counter {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        self.0.fetch_add(1, Ordering::SeqCst);
        Approval::Deny
    }
}

fn host(mode: ApprovalMode, full: bool, approver: Arc<Counter>) -> ToolHost {
    let store = parzi_core::store::SessionStore::open().unwrap();
    let meta = store.create("desk", "t", "claude").unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: meta.id.clone(),
        mode,
        edits_auto: false,
        full,
        cfg: parzi_core::config::ParziConfig::default(),
        store,
        tools: Arc::new(ToolExecutor {
            cwd: std::env::temp_dir().display().to_string(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["*".into()],
        }),
        approver,
        asker: None,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test")),
        harness: None,
        sink: RunSink::new(&meta.id, tx, None),
        cancel: tokio_util::sync::CancellationToken::new(),
    })
}

fn read(tool: &str, input: serde_json::Value) -> PermissionRequest {
    PermissionRequest {
        name: String::new(),
        id: "r1".into(),
        tool: tool.into(),
        title: tool.into(),
        input,
        paths: vec![],
    }
}

fn denied_as_secret(d: &PermissionDecision) -> bool {
    matches!(d, PermissionDecision::Deny(why) if why.contains("sign-in tokens"))
}

#[tokio::test]
async fn token_files_stay_shut_in_every_mode() {
    let home = home("desktools");
    let gui = home.join("gui.json").display().to_string();
    for (mode, full) in [
        (ApprovalMode::Auto, false),
        (ApprovalMode::Auto, true),
        (ApprovalMode::Deny, false),
    ] {
        let h = host(mode, full, Arc::default());
        let d = h.decide(read("Read", json!({ "file_path": gui }))).await;
        assert!(denied_as_secret(&d), "{mode:?} full={full}: {d:?}");
        let d = h
            .decide(read(
                "Grep",
                json!({ "pattern": "token", "path": home.display().to_string() }),
            ))
            .await;
        assert!(denied_as_secret(&d), "a search inside ~/.parzi: {d:?}");
        let d = h
            .decide(read("Bash", json!({ "command": "cat ~/.parzi/gui.json" })))
            .await;
        assert!(denied_as_secret(&d), "{d:?}");
        let fine = std::env::temp_dir().join("notes.txt").display().to_string();
        let d = h.decide(read("Read", json!({ "file_path": fine }))).await;
        assert_eq!(d, PermissionDecision::Allow, "{mode:?}: an ordinary read");
    }
}

#[tokio::test]
async fn parzi_tools_refuse_token_files_before_asking() {
    let home = home("desktools");
    let asked = Arc::new(Counter::default());
    let h = host(ApprovalMode::Ask, false, asked.clone());
    let gui = home.join("serve.json").display().to_string();
    let (ok, out) = h.call("doc.read", &json!({ "source": gui })).await;
    assert!(!ok && out.contains("sign-in tokens"), "{out}");
    let (ok, out) = h
        .call(
            "shell.exec",
            &json!({ "cmd": "Get-Content $env:USERPROFILE\\.parzi\\gui.json" }),
        )
        .await;
    assert!(!ok && out.contains("sign-in tokens"), "{out}");
    assert_eq!(asked.0.load(Ordering::SeqCst), 0, "refused, never asked");
}

#[tokio::test]
async fn page_tools_without_a_window_say_so() {
    home("desktools");
    let asked = Arc::new(Counter::default());
    let h = host(ApprovalMode::Auto, false, asked.clone());
    for tool in ["browser.read", "browser.click", "desk.state"] {
        let (ok, out) = h.call(tool, &json!({})).await;
        assert!(!ok, "{tool}: {out}");
        assert!(out.contains("needs the Parzi window"), "{tool}: {out}");
    }
    assert_eq!(asked.0.load(Ordering::SeqCst), 0);
}
