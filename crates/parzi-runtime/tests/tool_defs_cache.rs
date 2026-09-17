//! E6: the tool list is built once per run, not once per turn. A run that
//! takes four turns must reach the MCP servers exactly once — before this the
//! cost (up to 12 s per unreachable server, plus a half-started child) was
//! paid on every turn.

use std::collections::HashMap;
use std::sync::Arc;

use parzi_core::config::McpServerCfg;
use parzi_core::store::SessionStore;
use parzi_providers::{AuthStatus, ChatReq, EventRx, Model, Provider, StreamEvent};
use parzi_runtime::handler::{AgentRun, ProviderSlot};
use parzi_runtime::mcp::McpManager;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo, ToolExecutor};
use tokio::sync::mpsc;

/// Hermetic home for this test binary (see approval_gate.rs).
static TEST_HOME_INIT: std::sync::Once = std::sync::Once::new();

fn test_home() {
    TEST_HOME_INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("parzi-test-toolcache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
    });
}

/// Emits a tool call on the first three turns, then plain text: four turns,
/// four `tool_defs` calls.
struct ThreeToolCalls;

#[async_trait::async_trait]
impl Provider for ThreeToolCalls {
    fn id(&self) -> &'static str {
        "cachetest"
    }
    async fn models(&self) -> Result<Vec<Model>, parzi_core::error::ParziError> {
        Ok(vec![])
    }
    async fn chat_stream(&self, req: ChatReq) -> Result<EventRx, parzi_core::error::ParziError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let done = req
            .messages
            .iter()
            .filter(|m| m.content.contains("[tool:"))
            .count();
        if done < 3 {
            let _ = tx.send(Ok(StreamEvent::ToolCall {
                id: format!("t{done}"),
                name: "fs.write".into(),
                args: serde_json::json!({"path": format!("c{done}.txt"), "content": "x"}),
            }));
        } else {
            let _ = tx.send(Ok(StreamEvent::Text("done".into())));
        }
        Ok(rx)
    }
    fn auth_status(&self) -> AuthStatus {
        AuthStatus::Ok
    }
}

struct AllowAll;
#[async_trait::async_trait]
impl Approver for AllowAll {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

/// An unreachable server: the worst case the audit measured (R-2). It must be
/// attempted once per run, never once per turn.
fn broken_server() -> HashMap<String, McpServerCfg> {
    let mut servers = HashMap::new();
    servers.insert(
        "broken".to_string(),
        McpServerCfg {
            command: "parzi-no-such-mcp-binary".into(),
            args: vec![],
            env: HashMap::new(),
            allow: vec![],
            deny: vec![],
            tool_modes: HashMap::new(),
            timeout_ms: 1_000,
            enabled: true,
        },
    );
    servers
}

#[tokio::test]
async fn tool_defs_are_built_once_per_run() {
    test_home();
    let dir = std::env::temp_dir().join(format!("parzi-toolcache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let store = SessionStore::open().unwrap();
    let meta = store.create("cache", "t", "", "cachetest/m").unwrap();
    let sid = meta.id.clone();

    let mcp = Arc::new(McpManager::new(broken_server(), 60));
    let tools = Arc::new(ToolExecutor {
        cwd: dir.to_string_lossy().to_string(),
        mcp: mcp.clone(),
        allowed: vec!["fs.write".into()],
        leases: None,
    });
    let slots = vec![ProviderSlot {
        provider_id: "cachetest".into(),
        provider: Box::new(ThreeToolCalls),
        model_id: "m".into(),
        price_in: 0.0,
        price_out: 0.0,
        reason: "test",
    }];
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let run = AgentRun::new(
        sid.clone(),
        slots,
        vec![],
        "test".into(),
        parzi_runtime::tools::ApprovalMode::Auto,
        false,
        8,
        4096,
        vec![],
        "low".into(),
        store.clone(),
        tools,
        Arc::new(AllowAll),
        tx,
        tokio_util::sync::CancellationToken::new(),
    );
    run.run("go").await.unwrap();

    let turns = store
        .events(&sid)
        .unwrap()
        .iter()
        .filter(|e| matches!(e, parzi_core::store::Event::ToolCall { .. }))
        .count();
    assert_eq!(turns, 3, "provider should have driven four turns");
    assert_eq!(
        mcp.exposed_tools_calls(),
        1,
        "tool defs must be built once per run, not once per turn"
    );

    // A Settings save invalidates the cache: the next turn rebuilds.
    mcp.set_configs(broken_server()).await;
    let store2 = store.clone();
    let sid2 = store2.create("cache2", "t", "", "cachetest/m").unwrap().id;
    let tools2 = Arc::new(ToolExecutor {
        cwd: dir.to_string_lossy().to_string(),
        mcp: mcp.clone(),
        allowed: vec!["fs.write".into()],
        leases: None,
    });
    let (tx2, mut rx2) = mpsc::unbounded_channel();
    tokio::spawn(async move { while rx2.recv().await.is_some() {} });
    let run2 = AgentRun::new(
        sid2.clone(),
        vec![ProviderSlot {
            provider_id: "cachetest".into(),
            provider: Box::new(ThreeToolCalls),
            model_id: "m".into(),
            price_in: 0.0,
            price_out: 0.0,
            reason: "test",
        }],
        vec![],
        "test".into(),
        parzi_runtime::tools::ApprovalMode::Auto,
        false,
        8,
        4096,
        vec![],
        "low".into(),
        store2.clone(),
        tools2,
        Arc::new(AllowAll),
        tx2,
        tokio_util::sync::CancellationToken::new(),
    );
    run2.run("go").await.unwrap();
    assert_eq!(
        mcp.exposed_tools_calls(),
        2,
        "a second run rebuilds its own list, still once"
    );

    let _ = std::fs::remove_dir_all(&dir);
    if let Ok(d) = parzi_core::paths::sessions_dir() {
        let _ = std::fs::remove_dir_all(d.join(&sid));
        let _ = std::fs::remove_dir_all(d.join(&sid2));
    }
}
