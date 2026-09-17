//! R-2 / R-3 against a real stdio server: a small node script that speaks
//! JSON-RPC, interleaves notifications and stale answers, and can refuse to
//! answer at all. Skipped (not failed) where node is absent.

use std::collections::HashMap;

use parzi_core::config::McpServerCfg;
use parzi_runtime::mcp::McpManager;

const FAKE_SERVER: &str = r#"
const readline = require('readline');
const fs = require('fs');
if (process.env.PID_FILE) fs.writeFileSync(process.env.PID_FILE, String(process.pid));
const send = (o) => process.stdout.write(JSON.stringify(o) + '\n');
readline.createInterface({ input: process.stdin }).on('line', (line) => {
  let m; try { m = JSON.parse(line); } catch (e) { return; }
  if (m.method === 'initialize') {
    // A notification before the answer: a one-line-per-request reader eats it
    // and treats it as the response (R-3).
    send({ jsonrpc: '2.0', method: 'notifications/progress', params: {} });
    send({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: '2024-11-05', capabilities: {},
      serverInfo: { name: 'fake', version: '1' } } });
  } else if (m.method === 'tools/list') {
    send({ jsonrpc: '2.0', method: 'notifications/message', params: {} });
    send({ jsonrpc: '2.0', id: 999, result: { stale: true } });
    send({ jsonrpc: '2.0', id: m.id, result: { tools: [
      { name: 'echo', description: 'echo back', inputSchema: { type: 'object' } },
      { name: 'hang', description: 'never answers', inputSchema: { type: 'object' } }] } });
  } else if (m.method === 'tools/call') {
    if (m.params.name === 'hang') return;
    send({ jsonrpc: '2.0', id: m.id, result: { content: [
      { type: 'text', text: 'echo:' + JSON.stringify(m.params.arguments) }] } });
  }
});
"#;

fn have_node() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        // Not `/proc`: macOS has none, so that check answered "dead" for every
        // pid and quietly turned both liveness assertions into no-ops. `ps`
        // exists on both; a zombie is counted as gone, since the child is
        // already killed and only waiting to be reaped.
        std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "stat="])
            .output()
            .map(|o| {
                let stat = String::from_utf8_lossy(&o.stdout);
                let stat = stat.trim();
                !stat.is_empty() && !stat.starts_with('Z')
            })
            .unwrap_or(false)
    }
}

/// Manager with one fake server; `PID_FILE` comes through the per-server env
/// (the parent environment is scrubbed).
fn manager(tag: &str, timeout_ms: u64, idle_secs: u64) -> (McpManager, std::path::PathBuf) {
    let pid_file = std::env::temp_dir().join(format!("parzi-mcp-{tag}-{}.pid", std::process::id()));
    let _ = std::fs::remove_file(&pid_file);
    let mut env = HashMap::new();
    env.insert(
        "PID_FILE".to_string(),
        pid_file.to_string_lossy().to_string(),
    );
    let cfg = McpServerCfg {
        command: "node".into(),
        args: vec!["-e".into(), FAKE_SERVER.into()],
        env,
        allow: vec![],
        deny: vec![],
        tool_modes: HashMap::new(),
        timeout_ms,
        enabled: true,
    };
    let mut servers = HashMap::new();
    servers.insert("fake".to_string(), cfg);
    (McpManager::new(servers, idle_secs), pid_file)
}

fn pid_of(pid_file: &std::path::Path) -> u32 {
    std::fs::read_to_string(pid_file)
        .unwrap_or_default()
        .trim()
        .parse()
        .expect("server wrote its pid")
}

/// R-3: notifications and a stale answer to an abandoned request must not be
/// mistaken for our response.
#[tokio::test]
async fn notifications_and_stale_ids_do_not_desync() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let (mgr, _pid_file) = manager("desync", 5_000, 60);
    let tools = mgr.exposed_tools("fake").await.expect("tools/list");
    assert_eq!(tools.len(), 2, "got {tools:?}");
    assert_eq!(tools[0].qualified(), "fake.echo");
    let (ok, text) = mgr
        .call_tool("fake", "echo", serde_json::json!({"x": 1}))
        .await;
    assert!(ok, "echo failed: {text}");
    assert!(text.contains("\"x\":1"), "{text}");
    mgr.shutdown().await;
}

/// R-3: a call that never gets an answer must time out, drop the connection
/// and let the next call work against a fresh server.
#[tokio::test]
async fn read_timeout_respawns_the_server() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let (mgr, pid_file) = manager("timeout", 1_200, 60);
    mgr.exposed_tools("fake").await.expect("tools/list");
    let first = pid_of(&pid_file);
    let (ok, err) = mgr.call_tool("fake", "hang", serde_json::json!({})).await;
    assert!(!ok, "a silent server must not report success");
    assert!(err.contains("timed out"), "{err}");
    let (ok, text) = mgr
        .call_tool("fake", "echo", serde_json::json!({"y": 2}))
        .await;
    assert!(ok, "next call after a timeout must work: {text}");
    assert!(text.contains("\"y\":2"), "{text}");
    let second = pid_of(&pid_file);
    assert_ne!(first, second, "the dead connection must be respawned");
    mgr.shutdown().await;
    assert!(!alive(second), "shutdown left pid {second} running");
}

/// E8: an idle server dies on its own timer, not on the next MCP call (which
/// may never come). The floor is 10 s, so this test waits in real time.
#[tokio::test]
async fn idle_server_is_reaped_on_a_timer() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let (mgr, pid_file) = manager("reap", 5_000, 10);
    mgr.exposed_tools("fake").await.expect("tools/list");
    let pid = pid_of(&pid_file);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
    while alive(pid) && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(!alive(pid), "idle server {pid} was never reaped");
    mgr.shutdown().await;
}

/// R-2 / E8: nothing survives `shutdown()`.
#[tokio::test]
async fn shutdown_kills_the_server() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let (mgr, pid_file) = manager("shutdown", 5_000, 60);
    mgr.exposed_tools("fake").await.expect("tools/list");
    let pid = pid_of(&pid_file);
    assert!(alive(pid), "server should be running before shutdown");
    mgr.shutdown().await;
    assert!(!alive(pid), "shutdown left pid {pid} running");
}
