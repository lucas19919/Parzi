//! Runtime gates: allowlists deny by default; ui tools always advertised.

use std::collections::HashMap;
use std::sync::Arc;

use parzi_runtime::mcp::McpManager;
use parzi_runtime::tools::{ApprovalMode, ToolExecutor};

fn exec(allowed: &[&str]) -> ToolExecutor {
    ToolExecutor {
        cwd: String::new(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: allowed.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn deny_by_default() {
    let e = exec(&[]);
    assert!(!e.is_allowed("fs.read"));
    assert!(!e.is_allowed("anything.at.all"));
}

#[test]
fn exact_prefix_and_star() {
    let e = exec(&["fs.read", "myserver.*"]);
    assert!(e.is_allowed("fs.read"));
    assert!(!e.is_allowed("fs.write"));
    assert!(e.is_allowed("myserver.tool"));
    let star = exec(&["*"]);
    assert!(star.is_allowed("whatever.goes"));
}

#[test]
fn mode_parses() {
    assert_eq!(ApprovalMode::parse("auto"), ApprovalMode::Auto);
    assert_eq!(ApprovalMode::parse("deny"), ApprovalMode::Deny);
    assert_eq!(ApprovalMode::parse("ask"), ApprovalMode::Ask);
    assert_eq!(ApprovalMode::parse("bogus"), ApprovalMode::Ask);
}

#[tokio::test]
async fn disallowed_tool_fails_closed() {
    let e = exec(&[]);
    let (ok, _) = e.execute("fs.read", &serde_json::json!({"path": "x"})).await;
    assert!(!ok);
    let (ok, _) = e.execute("evil.tool", &serde_json::json!({})).await;
    assert!(!ok);
}

#[tokio::test]
async fn path_escape_rejected() {
    let e = exec(&["fs.read"]);
    let (ok, msg) = e
        .execute("fs.read", &serde_json::json!({"path": "../../secret"}))
        .await;
    assert!(!ok, "{msg}");
}
