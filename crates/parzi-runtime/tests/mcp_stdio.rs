use std::collections::HashMap;
use std::time::{Duration, Instant};

use parzi_core::config::McpServerCfg;
use parzi_runtime::mcp::McpManager;

const FAKE_SERVER: &str = r#"
const readline = require('readline');
const fs = require('fs');
if (process.env.PID_FILE) fs.writeFileSync(process.env.PID_FILE, String(process.pid));
const send = (o) => process.stdout.write(JSON.stringify(o) + '\n');
const version = process.env.VERSION || '2024-11-05';
let grown = false;
let asked = 1;
const waiting = {};
const ask = (method, then) => {
  const id = 'srv-' + (asked++);
  waiting[id] = then;
  send({ jsonrpc: '2.0', id, method });
};
readline.createInterface({ input: process.stdin }).on('line', (line) => {
  let m; try { m = JSON.parse(line); } catch (e) { return; }
  if (m.method === undefined && waiting[m.id]) {
    const then = waiting[m.id]; delete waiting[m.id]; then(m); return;
  }
  if (m.method === 'notifications/cancelled') {
    if (process.env.CANCEL_FILE) fs.appendFileSync(process.env.CANCEL_FILE, JSON.stringify(m.params) + '\n');
    return;
  }
  if (m.method === 'initialize') {
    if (process.env.INIT_FILE) fs.writeFileSync(process.env.INIT_FILE, JSON.stringify(m.params));
    send({ jsonrpc: '2.0', method: 'notifications/progress', params: {} });
    send({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: version,
      capabilities: { tools: { listChanged: true } }, serverInfo: { name: 'fake', version: '1' } } });
  } else if (m.method === 'tools/list') {
    send({ jsonrpc: '2.0', method: 'notifications/message', params: { level: 'info', data: 'listing' } });
    send({ jsonrpc: '2.0', id: 999, result: { stale: true } });
    const first = [
      { name: 'echo', description: 'echo back', inputSchema: { type: 'object' } },
      { name: 'hang', description: 'never answers', inputSchema: { type: 'object' } }];
    const rest = [{ name: 'slow' }, { name: 'pinger' }, { name: 'crash' }, { name: 'grow' }]
      .concat(grown ? [{ name: 'extra' }] : []);
    if (!m.params || !m.params.cursor) send({ jsonrpc: '2.0', id: m.id, result: { tools: first, nextCursor: 'two' } });
    else send({ jsonrpc: '2.0', id: m.id, result: { tools: rest } });
  } else if (m.method === 'tools/call') {
    const n = m.params.name;
    const reply = (text) => send({ jsonrpc: '2.0', id: m.id, result: { content: [{ type: 'text', text }] } });
    if (n === 'hang') return;
    if (n === 'slow') { setTimeout(() => reply('slow done'), 1000); return; }
    if (n === 'pinger') {
      ask('ping', (p) => ask('roots/list', (r) => reply(JSON.stringify({ ping: p.result || p.error, roots: r.result || r.error }))));
      return;
    }
    if (n === 'crash') { process.stderr.write('FATAL: the disk is on fire\n'); process.exit(3); }
    if (n === 'grow') {
      grown = true; reply('grown');
      send({ jsonrpc: '2.0', method: 'notifications/tools/list_changed' });
      return;
    }
    reply('echo:' + JSON.stringify(m.params.arguments));
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

fn scratch(tag: &str, what: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("parzi-mcp-{tag}-{}.{what}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn private_home() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("parzi-mcp-stdio-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("PARZI_HOME", dir);
    });
}

struct Fake {
    mgr: McpManager,
    pid_file: std::path::PathBuf,
    cancel_file: std::path::PathBuf,
    init_file: std::path::PathBuf,
}

fn fake_with(tag: &str, timeout_ms: u64, idle_secs: u64, version: &str) -> Fake {
    private_home();
    let pid_file = scratch(tag, "pid");
    let cancel_file = scratch(tag, "cancel");
    let init_file = scratch(tag, "init");
    let env = HashMap::from([
        ("PID_FILE".to_string(), pid_file.display().to_string()),
        ("CANCEL_FILE".to_string(), cancel_file.display().to_string()),
        ("INIT_FILE".to_string(), init_file.display().to_string()),
        ("VERSION".to_string(), version.to_string()),
    ]);
    let cfg = McpServerCfg {
        command: "node".into(),
        args: vec!["-e".into(), FAKE_SERVER.into()],
        env,
        timeout_ms,
        enabled: true,
        ..Default::default()
    };
    let mut servers = HashMap::new();
    servers.insert("fake".to_string(), cfg);
    Fake {
        mgr: McpManager::new(servers, idle_secs),
        pid_file,
        cancel_file,
        init_file,
    }
}

fn fake(tag: &str, timeout_ms: u64, idle_secs: u64) -> Fake {
    fake_with(tag, timeout_ms, idle_secs, "2024-11-05")
}

fn pid_of(pid_file: &std::path::Path) -> u32 {
    std::fs::read_to_string(pid_file)
        .unwrap_or_default()
        .trim()
        .parse()
        .expect("server wrote its pid")
}

#[tokio::test]
async fn notifications_stale_ids_and_pages_do_not_desync() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let f = fake("desync", 5_000, 60);
    let tools = f.mgr.exposed_tools("fake").await.expect("tools/list");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        ["echo", "hang", "slow", "pinger", "crash", "grow"],
        "both pages"
    );
    assert_eq!(tools[0].qualified(), "fake.echo");
    let (ok, text) = f
        .mgr
        .call_tool("fake", "echo", serde_json::json!({"x": 1}))
        .await;
    assert!(ok, "echo failed: {text}");
    assert!(text.contains("\"x\":1"), "{text}");
    let init: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&f.init_file).unwrap()).unwrap();
    assert_eq!(init["protocolVersion"], "2025-06-18", "one current version");
    assert!(init["capabilities"]["roots"].is_object(), "{init}");
    f.mgr.shutdown().await;
}

#[tokio::test]
async fn a_timed_out_call_is_cancelled_and_the_server_kept() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let f = fake("timeout", 1_200, 60);
    f.mgr.exposed_tools("fake").await.expect("tools/list");
    let first = pid_of(&f.pid_file);
    let (ok, err) = f.mgr.call_tool("fake", "hang", serde_json::json!({})).await;
    assert!(!ok, "a silent server must not report success");
    assert!(err.contains("timed out"), "{err}");
    assert!(err.contains("tool `fake`"), "names the connector: {err}");
    let (ok, text) = f
        .mgr
        .call_tool("fake", "echo", serde_json::json!({"y": 2}))
        .await;
    assert!(ok, "next call after a timeout must work: {text}");
    assert!(text.contains("\"y\":2"), "{text}");
    assert_eq!(pid_of(&f.pid_file), first, "the same server answers");
    assert!(alive(first));
    let cancelled = std::fs::read_to_string(&f.cancel_file).unwrap_or_default();
    let note: serde_json::Value =
        serde_json::from_str(cancelled.lines().next().expect("told to cancel")).unwrap();
    assert!(note["requestId"].is_number(), "{note}");
    f.mgr.shutdown().await;
    assert!(!alive(first), "shutdown left pid {first} running");
}

#[tokio::test]
async fn calls_to_one_connector_overlap() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let f = fake("overlap", 5_000, 60);
    f.mgr.exposed_tools("fake").await.expect("tools/list");
    let t = Instant::now();
    let (a, b) = tokio::join!(
        f.mgr.call_tool("fake", "slow", serde_json::json!({})),
        f.mgr.call_tool("fake", "slow", serde_json::json!({}))
    );
    assert!(a.0 && b.0, "{a:?} {b:?}");
    assert!(
        t.elapsed() < Duration::from_millis(1_800),
        "two one-second calls took {:?}",
        t.elapsed()
    );
    f.mgr.shutdown().await;
}

#[tokio::test]
async fn the_server_may_ping_and_ask_for_the_folders() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let f = fake("pinger", 5_000, 60);
    let (ok, text) = f
        .mgr
        .call_tool("fake", "pinger", serde_json::json!({}))
        .await;
    assert!(ok, "{text}");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["ping"], serde_json::json!({}));
    assert_eq!(v["roots"]["code"], -32601, "no folder known yet: {v}");
    let folder = std::env::temp_dir().join("parzi project");
    f.mgr.note_folder(&folder.display().to_string());
    let (ok, text) = f
        .mgr
        .call_tool("fake", "pinger", serde_json::json!({}))
        .await;
    assert!(ok, "{text}");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let uri = v["roots"]["roots"][0]["uri"].as_str().unwrap_or_default();
    assert!(
        uri.starts_with("file://") && uri.ends_with("parzi%20project"),
        "{v}"
    );
    assert_eq!(v["roots"]["roots"][0]["name"], "parzi project");
    f.mgr.shutdown().await;
}

#[tokio::test]
async fn a_crash_names_the_connector_and_what_it_said() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let f = fake("crash", 5_000, 60);
    f.mgr.exposed_tools("fake").await.expect("tools/list");
    let first = pid_of(&f.pid_file);
    let (ok, err) = f
        .mgr
        .call_tool("fake", "crash", serde_json::json!({}))
        .await;
    assert!(!ok);
    assert!(err.starts_with("tool `fake`: "), "{err}");
    assert!(err.contains("FATAL: the disk is on fire"), "{err}");
    let (ok, text) = f
        .mgr
        .call_tool("fake", "echo", serde_json::json!({"z": 3}))
        .await;
    assert!(ok, "a stopped server starts again: {text}");
    assert_ne!(pid_of(&f.pid_file), first, "a new process");
    f.mgr.shutdown().await;
}

#[tokio::test]
async fn a_startup_failure_says_why() {
    let cfg = McpServerCfg {
        command: "node".into(),
        args: vec![
            "-e".into(),
            "process.stderr.write('booting\\nError: missing API_KEY env var\\n'); process.exit(2)"
                .into(),
        ],
        timeout_ms: 5_000,
        enabled: true,
        ..Default::default()
    };
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    private_home();
    let mgr = McpManager::new(HashMap::from([("keyless".to_string(), cfg)]), 60);
    let err = mgr.exposed_tools("keyless").await.unwrap_err().to_string();
    let first = err.lines().next().unwrap();
    assert!(first.starts_with("tool `keyless`: "), "{err}");
    assert!(
        first.ends_with("Error: missing API_KEY env var"),
        "the last word is on the first line: {err}"
    );
    assert!(err.contains("booting"), "{err}");
    mgr.shutdown().await;
}

#[tokio::test]
async fn an_unknown_protocol_version_is_refused() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let f = fake_with("version", 5_000, 60, "2099-01-01");
    let err = f.mgr.exposed_tools("fake").await.unwrap_err().to_string();
    assert!(err.contains("2099-01-01"), "{err}");
    let pid = pid_of(&f.pid_file);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!alive(pid), "a server Parzi cannot speak to is stopped");
    f.mgr.shutdown().await;
}

#[tokio::test]
async fn a_changed_tool_list_is_listed_again() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let f = fake("grow", 5_000, 60);
    let before = f.mgr.exposed_tools("fake").await.expect("tools/list");
    assert!(before.iter().all(|t| t.name != "extra"));
    let (ok, _) = f.mgr.call_tool("fake", "grow", serde_json::json!({})).await;
    assert!(ok);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let now = f.mgr.session_tools(Duration::from_millis(100)).await;
        if now.iter().any(|t| t.name == "extra") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the new tool never showed: {now:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let listed = f.mgr.exposed_tools("fake").await.unwrap();
    assert!(listed.iter().any(|t| t.name == "extra"), "{listed:?}");
    f.mgr.shutdown().await;
}

#[tokio::test]
async fn idle_server_is_reaped_on_a_timer() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let f = fake("reap", 5_000, 10);
    f.mgr.exposed_tools("fake").await.expect("tools/list");
    let pid = pid_of(&f.pid_file);
    let deadline = Instant::now() + Duration::from_secs(25);
    while alive(pid) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(!alive(pid), "idle server {pid} was never reaped");
    f.mgr.shutdown().await;
}

#[tokio::test]
async fn shutdown_kills_the_server() {
    if !have_node() {
        eprintln!("skipped: node not on PATH");
        return;
    }
    let f = fake("shutdown", 5_000, 60);
    f.mgr.exposed_tools("fake").await.expect("tools/list");
    let pid = pid_of(&f.pid_file);
    assert!(alive(pid), "server should be running before shutdown");
    f.mgr.shutdown().await;
    assert!(!alive(pid), "shutdown left pid {pid} running");
}

#[tokio::test]
#[ignore = "network: reaches a hosted MCP server through mcp-remote"]
async fn a_hosted_server_answers_through_mcp_remote() {
    let cfg = McpServerCfg {
        command: "npx".into(),
        args: vec![
            "-y".into(),
            "mcp-remote@0.14.3".into(),
            "https://mcp.deepwiki.com/mcp".into(),
        ],
        timeout_ms: 180_000,
        enabled: true,
        ..Default::default()
    };
    private_home();
    let mgr = McpManager::new(HashMap::from([("deepwiki".to_string(), cfg)]), 60);
    let tools = mgr
        .exposed_tools("deepwiki")
        .await
        .expect("lists its tools");
    assert!(!tools.is_empty(), "a hosted server offers tools");
    mgr.shutdown().await;
}
