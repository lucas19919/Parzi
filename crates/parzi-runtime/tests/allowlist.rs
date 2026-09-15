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
    let (ok, _) = e
        .execute("fs.read", &serde_json::json!({"path": "x"}))
        .await;
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

fn exec_in(cwd: &str, allowed: &[&str]) -> ToolExecutor {
    ToolExecutor {
        cwd: cwd.to_string(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: allowed.iter().map(|s| s.to_string()).collect(),
    }
}

/// B3 sandbox table: absolute, rooted, drive-relative, UNC, verbatim,
/// deep `..` climbs and empty cwd must all fail closed.
#[tokio::test]
async fn sandbox_table_rejects_escapes() {
    let dir = std::env::temp_dir().join(format!("parzi-sandbox-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let cwd = dir.to_string_lossy().to_string();
    let e = exec_in(&cwd, &["fs.read", "fs.write", "fs.list"]);
    for evil in [
        "C:\\Windows\\System32\\x",
        "\\foo",
        "/etc/passwd",
        "C:foo",
        "\\\\server\\share\\x",
        "//server/share/x",
        "\\\\?\\C:\\x",
        "../../secret",
        "sub/../../..",
        "a/../../../../etc",
    ] {
        let (ok, _) = e
            .execute("fs.read", &serde_json::json!({"path": evil}))
            .await;
        assert!(!ok, "must reject {evil}");
        let (ok, _) = e
            .execute(
                "fs.write",
                &serde_json::json!({"path": evil, "content": "x"}),
            )
            .await;
        assert!(!ok, "must reject write {evil}");
    }
    // Empty cwd refuses (R-7): no silent fallback to the process cwd.
    let no_cwd = exec(&["fs.read"]);
    let (ok, _) = no_cwd
        .execute("fs.read", &serde_json::json!({"path": "x"}))
        .await;
    assert!(!ok);
    let no_cwd_shell = exec(&["shell.exec"]);
    let (ok, _) = no_cwd_shell
        .execute("shell.exec", &serde_json::json!({"cmd": "echo hi"}))
        .await;
    // shell.exec without cwd fails closed too.
    assert!(!ok);
    // Legit relative paths still work.
    std::fs::write(dir.join("sub").join("ok.txt"), "hello").unwrap();
    let (ok, out) = e
        .execute("fs.read", &serde_json::json!({"path": "sub/ok.txt"}))
        .await;
    assert!(ok, "{out}");
    let _ = std::fs::remove_dir_all(&dir);
}
