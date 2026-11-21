mod common;

use std::collections::HashMap;
use std::sync::Arc;

use parzi_core::store::SessionStore;
use parzi_runtime::handler::RunSink;
use parzi_runtime::mcp::McpManager;
use parzi_runtime::toolhost::{project_plan, ToolHost, ToolHostParts};
use parzi_runtime::tools::{ApprovalMode, AutoApprover, ToolExecutor};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn host(project: &str) -> (ToolHost, SessionStore, String) {
    let store = SessionStore::open().unwrap();
    let sid = store.create("plans", project, "claude/m").unwrap().id;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let host = ToolHost::new(ToolHostParts {
        session_id: sid.clone(),
        mode: ApprovalMode::Auto,
        edits_auto: false,
        full: false,
        cfg: parzi_core::config::ParziConfig::default(),
        store: store.clone(),
        tools: Arc::new(ToolExecutor {
            cwd: String::new(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["project.*".into(), "session.*".into()],
        }),
        approver: Arc::new(AutoApprover),
        asker: None,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test")),
        harness: None,
        sink: RunSink::new(&sid, tx, None),
        cancel: CancellationToken::new(),
    });
    (host, store, sid)
}

#[tokio::test]
async fn agents_keep_the_project_plan_and_link_work_to_its_targets() {
    common::home("project-plan");
    let (h, store, _) = host("shop");

    let (ok, out) = h.call("project.plan", &json!({})).await;
    assert!(ok && out.contains("none yet"), "{out}");

    let (ok, out) = h
        .call(
            "project.plan",
            &json!({"goal": "Launch the shop", "targets": [{"title": "Checkout"}, {"title": "Product pages"}]}),
        )
        .await;
    assert!(ok, "{out}");
    assert!(out.contains("1. [todo] Checkout"), "{out}");

    let (ok, out) = h
        .call(
            "session.draft",
            &json!({"title": "Build checkout", "brief": "Build the checkout", "target": "checkout"}),
        )
        .await;
    assert!(ok && out.contains("linked to the target"), "{out}");

    let (ok, out) = h
        .call(
            "project.plan",
            &json!({"targets": [{"title": "Product pages"}, {"title": "Checkout", "done": true}]}),
        )
        .await;
    assert!(ok, "{out}");
    assert!(
        out.contains("1. [todo] Product pages") && out.contains("2. [done] Checkout"),
        "{out}"
    );

    let view = project_plan::view(&store, "shop").expect("the window can show it");
    assert_eq!(view["goal"], "Launch the shop");
    assert_eq!(view["targets"][1]["state"], "done");
    let doc = parzi_core::project_plan::doc_path("shop").expect("a file to open");
    let text = std::fs::read_to_string(doc).unwrap();
    assert!(text.contains("- [x] Checkout <!-- sessions: "), "{text}");

    let (bare, _, _) = host("default");
    let (ok, out) = bare.call("project.plan", &json!({})).await;
    assert!(!ok && out.contains("no project"), "{out}");
}
