//! A window attached to this test process: its own binary, so the flag
//! cannot leak into the tests that run without one. Page tools need the
//! Windows page engine.
#![cfg(windows)]

mod common;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
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

fn host(lane: &str, mode: ApprovalMode, full: bool, approver: Arc<Counter>) -> ToolHost {
    let store = parzi_core::store::SessionStore::open().unwrap();
    let meta = store.create("desk", "t", lane, "claude").unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: meta.id.clone(),
        lane: lane.into(),
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

#[tokio::test]
async fn work_asks_before_acting_on_a_page_and_lists_tabs_only_with_consent() {
    home("desktools-attached");
    // This test binary is its own process: pretend a window is attached.
    parzi_runtime::desk::attach();
    let asked = Arc::new(Counter::default());
    let work = host("work", ApprovalMode::Auto, false, asked.clone());
    for tool in [
        "browser.click",
        "browser.type",
        "browser.press",
        "browser.select",
    ] {
        let before = asked.0.load(Ordering::SeqCst);
        let (ok, out) = work
            .call(tool, &json!({ "text": "x", "key": "Enter", "option": "a" }))
            .await;
        assert!(!ok && out.contains("denied"), "{tool}: {out}");
        assert_eq!(
            asked.0.load(Ordering::SeqCst),
            before + 1,
            "{tool} asks in work"
        );
    }
    let before = asked.0.load(Ordering::SeqCst);
    let (ok, out) = work.call("browser.read", &json!({})).await;
    assert!(
        !ok && out.contains("not open"),
        "reads go straight to the window: {out}"
    );
    assert_eq!(asked.0.load(Ordering::SeqCst), before, "a read never asks");
    let build = host("build", ApprovalMode::Auto, false, asked.clone());
    let (ok, out) = build.call("browser.tabs", &json!({ "all": true })).await;
    assert!(!ok && out.contains("denied"), "{out}");
    assert_eq!(
        asked.0.load(Ordering::SeqCst),
        before + 1,
        "every tab asks, even in Auto"
    );
}
