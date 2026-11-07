mod common;

use std::collections::HashMap;

use common::*;
use parzi_core::config::{McpServerCfg, ParziConfig};
use parzi_providers::TurnEnd;
use parzi_runtime::mcp::McpTool;
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
            ..Default::default()
        },
    );
    servers
}

#[tokio::test]
async fn the_tool_list_is_built_once_per_run_from_the_remembered_connectors() {
    home("toolcache");
    let servers = broken_server();
    parzi_runtime::mcp_cache::save(
        "broken",
        &servers["broken"],
        &[McpTool {
            server: "broken".into(),
            name: "ping".into(),
            description: "answers pong".into(),
            input_schema: json!({"$schema": "x", "properties": {"n": {"type": "number"}}}),
            ..Default::default()
        }],
    );
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            for _ in 0..3 {
                let names = a.tool_names().await;
                assert!(names.iter().any(|n| n == "brain_search"));
                assert!(names
                    .iter()
                    .all(|n| !n.starts_with("browser_") && !n.starts_with("desk_")));
                assert!(names.iter().all(|n| n != "ui_show_widget"));
                assert!(names.iter().any(|n| n == "broken__ping"), "{names:?}");
            }
            let listed = mcp(&a.spec, "tools/list", json!({})).await;
            let ping = listed["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == "broken__ping")
                .cloned()
                .unwrap();
            assert_eq!(ping["description"], "answers pong");
            assert_eq!(ping["inputSchema"]["type"], "object");
            assert!(ping["inputSchema"].get("$schema").is_none());
            let (ok, out) = a.parzi("lane.status", json!({})).await;
            a.say(&format!("{ok} {out}"));
            Ok(TurnEnd::Completed)
        }),
    );
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "auto".into();
    cfg.mcp.servers = servers;
    let (orch, store) = orch_with(cfg, &[fake]);
    let (meta, _rx) = orch
        .spawn("t", "claude", "go", None, "", "low", vec![], None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    assert!(
        last_reply(&store, &meta.id).starts_with("false"),
        "{}",
        last_reply(&store, &meta.id)
    );
    assert_eq!(
        orch.mcp().live_starts(),
        0,
        "a remembered connector is not started to list its tools"
    );
    assert_eq!(orch.mcp().exposed_tools_calls(), 0);
}

#[tokio::test]
async fn a_connector_that_cannot_start_does_not_hold_up_the_list() {
    home("toolcache");
    let mut servers = broken_server();
    let cfg = servers.remove("broken").unwrap();
    servers.insert("missing".into(), cfg);
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            let t = std::time::Instant::now();
            let names = a.tool_names().await;
            let took = t.elapsed();
            assert!(names.iter().any(|n| n == "brain_search"));
            assert!(names.iter().all(|n| !n.starts_with("missing__")));
            let raw = mcp(
                &a.spec,
                "tools/call",
                json!({"name": "missing__ping", "arguments": {}}),
            )
            .await;
            a.say(&format!(
                "{} {}",
                took.as_millis(),
                raw["result"]["content"][0]["text"]
            ));
            Ok(TurnEnd::Completed)
        }),
    );
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "auto".into();
    cfg.mcp.servers = servers;
    let (orch, store) = orch_with(cfg, &[fake]);
    let (meta, _rx) = orch
        .spawn("t", "claude", "go", None, "", "low", vec![], None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let reply = last_reply(&store, &meta.id);
    let ms: u128 = reply.split(' ').next().unwrap().parse().unwrap();
    assert!(
        ms < 1_500,
        "the list waited {ms} ms on a connector that cannot start"
    );
    assert!(reply.contains("no tool `missing__ping`"), "{reply}");
}
