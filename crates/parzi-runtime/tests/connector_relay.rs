mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use common::*;
use parzi_core::config::{McpServerCfg, ParziConfig};
use parzi_providers::TurnEnd;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};
use serde_json::{json, Value};

const FAKE: &str = r#"
const readline = require('readline');
const send = (o) => process.stdout.write(JSON.stringify(o) + '\n');
const delay = Number(process.env.START_DELAY_MS || 0);
readline.createInterface({ input: process.stdin }).on('line', (line) => {
  let m; try { m = JSON.parse(line); } catch (e) { return; }
  if (m.method === 'initialize') {
    setTimeout(() => send({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: m.params.protocolVersion,
      capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1' } } }), delay);
  } else if (m.method === 'tools/list') {
    send({ jsonrpc: '2.0', id: m.id, result: { tools: [
      { name: 'echo', title: 'Echo', description: 'echo back',
        inputSchema: { $schema: 'http://json-schema.org/draft-07/schema#', type: 'object',
          properties: { x: { type: 'number' } } },
        annotations: { readOnlyHint: true } },
      { name: 'pic', description: 'a picture', inputSchema: { type: 'object' } },
      { name: 'fail', description: 'always fails', inputSchema: { type: 'object' } },
      { name: 'a.dotted tool', description: 'odd name', inputSchema: { type: 'object' } }] } });
  } else if (m.method === 'tools/call') {
    const n = m.params.name;
    let result;
    if (n === 'pic') result = { content: [{ type: 'text', text: 'here' }, { type: 'image', data: 'AAAA', mimeType: 'image/png' }] };
    else if (n === 'fail') result = { isError: true, content: [{ type: 'text', text: 'it broke' }] };
    else result = { content: [{ type: 'text', text: n + ':' + JSON.stringify(m.params.arguments) }] };
    send({ jsonrpc: '2.0', id: m.id, result });
  }
});
"#;

fn have_node() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn connector(delay_ms: u64) -> McpServerCfg {
    McpServerCfg {
        command: "node".into(),
        args: vec!["-e".into(), FAKE.into()],
        env: HashMap::from([("START_DELAY_MS".to_string(), delay_ms.to_string())]),
        timeout_ms: 10_000,
        enabled: true,
        ..Default::default()
    }
}

fn config(name: &str, srv: McpServerCfg) -> ParziConfig {
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "auto".into();
    cfg.mcp.servers = HashMap::from([(name.to_string(), srv)]);
    cfg
}

async fn call(a: &Agent, name: &str, args: Value) -> Value {
    mcp(
        &a.spec,
        "tools/call",
        json!({"name": name, "arguments": args}),
    )
    .await["result"]
        .clone()
}

#[tokio::test]
async fn an_agent_lists_and_calls_a_connector_through_parzi() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    home("relay");
    let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
    let keep = seen.clone();
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let keep = keep.clone();
            async move {
                let listed = mcp(&a.spec, "tools/list", json!({})).await;
                let tools = listed["result"]["tools"].as_array().cloned().unwrap();
                let echo = tools.iter().find(|t| t["name"] == "fake__echo").cloned();
                let names: Vec<String> = tools
                    .iter()
                    .filter_map(|t| t["name"].as_str().map(str::to_string))
                    .collect();
                let out = json!({
                    "names": names,
                    "echo": echo,
                    "call": call(&a, "fake__echo", json!({"x": 1})).await,
                    "pic": call(&a, "fake__pic", json!({})).await,
                    "fail": call(&a, "fake__fail", json!({})).await,
                    "dotted": call(&a, "fake__a_dotted_tool", json!({})).await,
                });
                keep.lock().unwrap().push(out);
                a.say("done");
                Ok(TurnEnd::Completed)
            }
        }),
    );
    let (orch, store) = orch_with(config("fake", connector(0)), &[fake]);
    let (meta, _rx) = orch
        .spawn("t", "claude", "go", None, "", "low", vec![], None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let got = seen.lock().unwrap().pop().expect("the agent ran");
    let names: Vec<&str> = got["names"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for want in [
        "brain_search",
        "fake__echo",
        "fake__pic",
        "fake__fail",
        "fake__a_dotted_tool",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    let echo = &got["echo"];
    assert_eq!(echo["title"], "Echo");
    assert_eq!(echo["annotations"]["readOnlyHint"], true);
    assert_eq!(echo["inputSchema"]["type"], "object");
    assert!(echo["inputSchema"].get("$schema").is_none(), "{echo}");
    assert_eq!(got["call"]["isError"], false, "{}", got["call"]);
    assert_eq!(got["call"]["content"][0]["text"], r#"echo:{"x":1}"#);
    assert_eq!(got["pic"]["content"][1]["type"], "image");
    assert_eq!(got["pic"]["content"][1]["data"], "AAAA");
    assert_eq!(got["pic"]["content"][1]["mimeType"], "image/png");
    assert_eq!(got["fail"]["isError"], true);
    assert_eq!(got["fail"]["content"][0]["text"], "it broke");
    assert_eq!(
        got["dotted"]["content"][0]["text"], "a.dotted tool:{}",
        "the real name reaches the connector"
    );
    orch.mcp().shutdown().await;
}

struct Person {
    seen: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl Approver for Person {
    async fn approve(&self, call: &ToolCallInfo) -> Approval {
        self.seen.lock().unwrap().push(call.name.clone());
        Approval::Allow
    }
}

#[tokio::test]
async fn each_tool_keeps_its_own_choice() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    home("relay");
    let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
    let keep = seen.clone();
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let keep = keep.clone();
            async move {
                let names = a.tool_names().await;
                let out = json!({
                    "names": names,
                    "echo": call(&a, "modes__echo", json!({})).await,
                    "fail": call(&a, "modes__fail", json!({})).await,
                    "pic": call(&a, "modes__pic", json!({})).await,
                });
                keep.lock().unwrap().push(out);
                Ok(TurnEnd::Completed)
            }
        }),
    );
    let mut srv = connector(0);
    srv.deny = vec!["pic".into()];
    srv.tool_modes = HashMap::from([
        ("echo".to_string(), "deny".to_string()),
        ("fail".to_string(), "ask".to_string()),
    ]);
    let person = Arc::new(Person {
        seen: Mutex::default(),
    });
    let (orch, store) = orch_with(config("modes", srv), &[fake]);
    let (meta, _rx) = orch
        .spawn(
            "t",
            "claude",
            "go",
            Some(person.clone() as Arc<dyn Approver>),
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let got = seen.lock().unwrap().pop().expect("the agent ran");
    let names = got["names"].to_string();
    assert!(names.contains("modes__fail"), "{names}");
    assert!(
        !names.contains("modes__echo"),
        "blocked tools are not offered"
    );
    assert!(
        !names.contains("modes__pic"),
        "tools switched off are not offered"
    );
    assert_eq!(got["echo"]["isError"], true);
    assert_eq!(got["pic"]["isError"], true);
    assert_eq!(
        got["fail"]["content"][0]["text"], "it broke",
        "an ask tool runs once the person allows it"
    );
    assert_eq!(*person.seen.lock().unwrap(), ["modes.fail"]);
    orch.mcp().shutdown().await;
}

#[tokio::test]
async fn a_slow_connector_waits_for_the_next_turn_not_this_one() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    home("relay");
    let seen: Arc<Mutex<Vec<(u128, bool)>>> = Arc::default();
    let keep = seen.clone();
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let keep = keep.clone();
            async move {
                let t = std::time::Instant::now();
                let names = a.tool_names().await;
                let has = names.iter().any(|n| n == "slow__echo");
                keep.lock().unwrap().push((t.elapsed().as_millis(), has));
                Ok(TurnEnd::Completed)
            }
        }),
    );
    let (orch, store) = orch_with(config("slow", connector(3_500)), &[fake]);
    let (meta, _rx) = orch
        .spawn("t", "claude", "go", None, "", "low", vec![], None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    // Wait for the background listing to land rather than a fixed nap: a
    // Node child starts much slower on a Windows runner than here.
    let listed = async {
        loop {
            let tools = orch.mcp().session_tools(std::time::Duration::ZERO).await;
            if tools.iter().any(|t| t.server == "slow" && t.name == "echo") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(30), listed)
        .await
        .expect("the connector lists in the background");
    orch.send_to(&meta.id, "again", None, "", "low", vec![], None, None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let turns = seen.lock().unwrap().clone();
    assert_eq!(turns.len(), 2, "{turns:?}");
    let (first_ms, first_has) = turns[0];
    assert!(!first_has, "it was still starting");
    assert!(
        (1_500..3_200).contains(&first_ms),
        "the first list waits about two seconds, not for the connector: {first_ms} ms"
    );
    let (second_ms, second_has) = turns[1];
    assert!(second_has, "the next turn has it");
    assert!(second_ms < 1_000, "{second_ms} ms");
    assert_eq!(
        orch.mcp().live_starts(),
        1,
        "listed once, in the background"
    );
    orch.mcp().shutdown().await;
}
