//! R-2: an unreachable connector costs one attempt per run, never one per
//! request. Agents ask for Parzi's tool list more than once in a turn (and
//! every call resolves its name against it): the list is built once.

mod common;

use std::collections::HashMap;

use common::*;
use parzi_core::config::{McpServerCfg, ParziConfig};
use parzi_providers::TurnEnd;
use serde_json::json;

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
async fn the_tool_list_is_built_once_per_run() {
    home("toolcache");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            for _ in 0..3 {
                assert!(a.tool_names().await.iter().any(|n| n == "ui_show_widget"));
            }
            let (ok, out) = a.parzi("plan.read", json!({})).await;
            a.say(&format!("{ok} {out}"));
            Ok(TurnEnd::Completed)
        }),
    );
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "auto".into();
    cfg.mcp.servers = broken_server();
    let (orch, store) = orch_with(cfg, &[fake]);
    let (meta, _rx) = orch
        .spawn("t", "", "claude", "go", None, "", "low", vec![], None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    assert_eq!(
        orch.mcp().exposed_tools_calls(),
        1,
        "the broken connector was asked once for this run"
    );
}
