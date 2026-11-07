use std::collections::HashMap;
use std::time::{Duration, Instant};

use parzi_core::config::McpServerCfg;
use parzi_runtime::mcp::McpManager;
use serde_json::{json, Value};
use tokio::io::AsyncBufReadExt;

const SERVER: &str = r#"
const http = require('http');
const stats = { conns: 0, deletes: 0, inits: 0, notes: 0 };
const used = new Set();
let sockets = 0;
const sessions = new Set();
let n = 0;
const srv = http.createServer((req, res) => {
  if (req.url === '/stats') { stats.conns = used.size; res.end(JSON.stringify(stats)); return; }
  used.add(req.socket.tag);
  if (req.method === 'DELETE') {
    stats.deletes++; sessions.delete(req.headers['mcp-session-id']);
    res.writeHead(200); res.end(); return;
  }
  let body = '';
  req.on('data', (d) => body += d);
  req.on('end', () => {
    const m = JSON.parse(body);
    const sid = req.headers['mcp-session-id'];
    if (m.method === 'initialize') {
      stats.inits++;
      const s = 's' + (++n); sessions.add(s);
      res.writeHead(200, { 'content-type': 'application/json', 'mcp-session-id': s });
      res.end(JSON.stringify({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: m.params.protocolVersion,
        capabilities: { tools: {} }, serverInfo: { name: 'h', version: '1' } } }));
      return;
    }
    if (!sessions.has(sid)) { res.writeHead(404); res.end('no such session'); return; }
    if (!('id' in m)) { stats.notes++; res.writeHead(202); res.end(); return; }
    if (m.method === 'tools/list') {
      // Answers, then keeps the stream open a while, as some servers do.
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      res.write('event: message\ndata: ' + JSON.stringify({ jsonrpc: '2.0', method: 'notifications/message', params: {} }) + '\n\n');
      res.write('event: message\ndata: ' + JSON.stringify({ jsonrpc: '2.0', id: m.id, result: { tools: [{ name: 't', inputSchema: { type: 'object' } }, { name: 'expire' }] } }) + '\n\n');
      setTimeout(() => res.end(), 8000);
      return;
    }
    if (m.method === 'tools/call') {
      if (m.params.name === 'expire') setImmediate(() => sessions.clear());
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ jsonrpc: '2.0', id: m.id, result: { content: [{ type: 'text', text: 'ok ' + sid }] } }));
    }
  });
});
srv.on('connection', (s) => { s.tag = ++sockets; });
srv.listen(0, '127.0.0.1', () => console.log(srv.address().port));
"#;

fn have_node() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

async fn stats(port: u16) -> Value {
    reqwest::get(format!("http://127.0.0.1:{port}/stats"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn one_connection_survives_an_ended_session_and_says_goodbye() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let home = std::env::temp_dir().join(format!("parzi-mcp-http-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&home);
    std::env::set_var("PARZI_HOME", &home);
    let mut node = tokio::process::Command::new("node")
        .args(["-e", SERVER])
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut out = tokio::io::BufReader::new(node.stdout.take().unwrap()).lines();
    let port: u16 = out
        .next_line()
        .await
        .unwrap()
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let cfg = McpServerCfg {
        url: format!("http://127.0.0.1:{port}/mcp"),
        timeout_ms: 5_000,
        enabled: true,
        ..Default::default()
    };
    let mgr = McpManager::new(HashMap::from([("h".to_string(), cfg)]), 60);

    let t = Instant::now();
    let tools = mgr.exposed_tools("h").await.expect("lists");
    assert!(
        t.elapsed() < Duration::from_secs(3),
        "the list waited for the stream to close: {:?}",
        t.elapsed()
    );
    assert_eq!(tools.len(), 2, "{tools:?}");
    assert_eq!(stats(port).await["notes"], 1, "initialized was accepted");

    let before = stats(port).await["conns"].as_u64().unwrap();
    for _ in 0..3 {
        let (ok, text) = mgr.call_tool("h", "t", json!({})).await;
        assert!(ok, "{text}");
        assert_eq!(text, "ok s1");
    }
    let after = stats(port).await["conns"].as_u64().unwrap();
    assert!(
        after <= before + 1,
        "calls reuse one connection: {before} then {after}"
    );

    let (ok, _) = mgr.call_tool("h", "expire", json!({})).await;
    assert!(ok);
    let (ok, text) = mgr.call_tool("h", "t", json!({})).await;
    assert!(ok, "an ended session starts again: {text}");
    assert_eq!(text, "ok s2");
    let s = stats(port).await;
    assert_eq!(s["inits"], 2, "{s}");
    assert_eq!(s["notes"], 2, "{s}");

    mgr.shutdown().await;
    let deadline = Instant::now() + Duration::from_secs(5);
    while stats(port).await["deletes"] != 1 {
        assert!(Instant::now() < deadline, "no DELETE for the session");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = node.kill().await;
}
