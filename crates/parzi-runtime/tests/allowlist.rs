mod common;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use parzi_core::store::SessionStore;
use parzi_providers::{PermissionDecision, PermissionGate, PermissionRequest};
use parzi_runtime::handler::RunSink;
use parzi_runtime::mcp::McpManager;
use parzi_runtime::toolhost::{ToolHost, ToolHostParts};
use parzi_runtime::tools::{Approval, ApprovalMode, Approver, ToolCallInfo, ToolExecutor};
use tokio_util::sync::CancellationToken;

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
    let (ok, _) = e.execute("evil.tool", &serde_json::json!({})).await;
    assert!(!ok);
}

struct Person {
    answer: Approval,
    seen: Mutex<Vec<ToolCallInfo>>,
}

#[async_trait::async_trait]
impl Approver for Person {
    async fn approve(&self, call: &ToolCallInfo) -> Approval {
        self.seen.lock().unwrap().push(call.clone());
        self.answer
    }
}

fn gate(folder: &Path, mode: ApprovalMode, edits_auto: bool, person: Arc<Person>) -> ToolHost {
    common::home("allowlist");
    let store = SessionStore::open().unwrap();
    let sid = store.create("gate", "", "", "claude/m").unwrap().id;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: sid.clone(),
        lane: String::new(),
        mode,
        edits_auto,
        store,
        tools: Arc::new(ToolExecutor {
            cwd: folder.display().to_string(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["*".into()],
        }),
        approver: person,
        harness: None,
        sink: RunSink::new(&sid, tx, None),
        cancel: CancellationToken::new(),
    })
}

async fn ask(host: &ToolHost, tool: &str, paths: &[&str]) -> PermissionDecision {
    host.decide(PermissionRequest {
        id: format!("req-{}", uuid::Uuid::new_v4()),
        tool: tool.into(),
        title: tool.into(),
        input: serde_json::json!({}),
        paths: paths.iter().map(|p| (*p).to_string()).collect(),
    })
    .await
}

fn nobody() -> Arc<Person> {
    Arc::new(Person {
        answer: Approval::Deny,
        seen: Mutex::new(vec![]),
    })
}

#[tokio::test]
async fn writes_outside_the_folder_need_a_person() {
    let folder = std::env::temp_dir().join(format!("parzi-fence-{}", std::process::id()));
    std::fs::create_dir_all(folder.join("src")).unwrap();
    let inside = folder.join("src").join("a.rs");
    let inside = inside.to_str().unwrap();
    let host = gate(&folder, ApprovalMode::Auto, false, nobody());

    assert_eq!(
        ask(&host, "Write", &[inside]).await,
        PermissionDecision::Allow
    );
    assert_eq!(
        ask(&host, "Edit", &["src/a.rs"]).await,
        PermissionDecision::Allow
    );
    let elsewhere = std::env::temp_dir().join("elsewhere.rs");
    for evil in ["../../secret", "src/../../x", elsewhere.to_str().unwrap()] {
        assert!(
            matches!(
                ask(&host, "Write", &[evil]).await,
                PermissionDecision::Deny(_)
            ),
            "{evil} must not be written on the lane's say-so"
        );
    }
    assert!(
        matches!(ask(&host, "edit", &[]).await, PermissionDecision::Deny(_)),
        "an edit that names no file is not approved blind"
    );
    assert_eq!(
        ask(&host, "Read", &[elsewhere.to_str().unwrap()]).await,
        PermissionDecision::Allow,
        "reads are not fenced"
    );

    let host = gate(&folder, ApprovalMode::Ask, true, nobody());
    assert_eq!(
        ask(&host, "Edit", &["src/a.rs"]).await,
        PermissionDecision::Allow
    );
    assert!(matches!(
        ask(&host, "Edit", &["../x"]).await,
        PermissionDecision::Deny(_)
    ));

    let person = Arc::new(Person {
        answer: Approval::Allow,
        seen: Mutex::new(vec![]),
    });
    let host = gate(&folder, ApprovalMode::Auto, false, person.clone());
    assert_eq!(
        ask(&host, "Write", &["../x"]).await,
        PermissionDecision::Allow
    );
    let cards = person.seen.lock().unwrap().clone();
    assert_eq!(cards.len(), 1, "one card, for the write outside");
    assert!(
        cards[0].args["title"]
            .as_str()
            .unwrap_or("")
            .contains("outside"),
        "{:?}",
        cards[0].args
    );
    let _ = std::fs::remove_dir_all(&folder);
}
