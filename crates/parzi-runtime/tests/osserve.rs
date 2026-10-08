use std::sync::Arc;
use std::time::Duration;

use parzi_core::config::ParziConfig;
use parzi_core::store::SessionStore;
use parzi_runtime::osserve;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};
use parzi_runtime::Orchestrator;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

fn isolated() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("parzi-osserve-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("PARZI_HOME", &dir);
    dir
}

async fn rpc(port: u16, token: &str, op: &str, mut body: Value) -> Value {
    body["op"] = json!(op);
    body["token"] = json!(token);
    let mut line = serde_json::to_string(&body).unwrap();
    line.push('\n');
    let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let (read, mut write) = stream.into_split();
    write.write_all(line.as_bytes()).await.unwrap();
    write.shutdown().await.unwrap();
    let mut reader = BufReader::new(read);
    let mut buf = String::new();
    reader.read_line(&mut buf).await.unwrap();
    serde_json::from_str(&buf).unwrap()
}

#[tokio::test]
async fn serve_stays_on_loopback_and_parks_approvals() {
    let dir = isolated();

    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let desk_port = listener.local_addr().unwrap().port();
    std::fs::write(
        dir.join("gui.json"),
        json!({ "port": desk_port, "token": "desk-token" }).to_string(),
    )
    .unwrap();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let (read, mut write) = sock.into_split();
        let mut line = String::new();
        let _ = BufReader::new(read).read_line(&mut line).await;
        let _ = write.write_all(b"{\"ok\":true,\"sessions\":[]}\n").await;
    });
    let blocked = osserve::start(Arc::new(Orchestrator::new(
        ParziConfig::default(),
        SessionStore::open().unwrap(),
    )))
    .await;
    let message = match blocked {
        Ok(_) => panic!("desk must block serve"),
        Err(message) => message,
    };
    assert!(message.contains("desk is open"), "{message}");
    std::fs::remove_file(dir.join("gui.json")).unwrap();

    let store = SessionStore::open().unwrap();
    let daemon = osserve::start(Arc::new(Orchestrator::new(
        ParziConfig::default(),
        store.clone(),
    )))
    .await
    .expect("serve starts");
    let meta: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("serve.json")).unwrap()).unwrap();
    let token = meta["token"].as_str().unwrap().to_string();
    assert_eq!(meta["port"].as_u64().unwrap(), u64::from(daemon.port));

    let denied = rpc(daemon.port, "nope", "health", json!({})).await;
    assert_eq!(denied["ok"], json!(false));
    assert_eq!(denied["error"], json!("bad token"));

    let health = rpc(daemon.port, &token, "health", json!({})).await;
    assert_eq!(health["ok"], json!(true));
    assert_eq!(health["serve"], json!("parzi-os"));

    let listed = rpc(daemon.port, &token, "session.list", json!({})).await;
    assert!(listed["sessions"].as_array().unwrap().is_empty());

    let second = osserve::start(Arc::new(Orchestrator::new(
        ParziConfig::default(),
        SessionStore::open().unwrap(),
    )))
    .await;
    let second = match second {
        Ok(_) => panic!("one serve"),
        Err(message) => message,
    };
    assert!(second.contains("already running"), "{second}");

    let session = store.create("note", "default", "", "claude/test").unwrap();
    let approver = daemon.approver.clone();
    let sid = session.id.clone();
    let waiting = tokio::spawn(async move {
        approver
            .approve(&ToolCallInfo {
                id: "call-1".into(),
                name: "shell.exec".into(),
                args: json!({ "cmd": "echo hi" }),
                lane: String::new(),
                session: sid,
            })
            .await
    });
    let key = loop {
        let page = rpc(daemon.port, &token, "approval.list", json!({})).await;
        let rows = page["approvals"].as_array().cloned().unwrap_or_default();
        if let Some(row) = rows.first() {
            break row["key"].as_str().unwrap().to_string();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let answered = rpc(
        daemon.port,
        &token,
        "approval.answer",
        json!({ "key": key, "allow": true }),
    )
    .await;
    assert_eq!(answered["ok"], json!(true));
    assert_eq!(waiting.await.unwrap(), Approval::Allow);
    let events = store.events(&session.id).unwrap();
    assert!(events.iter().any(|e| matches!(
        e,
        parzi_core::store::Event::System { text } if text.contains("waiting for approval")
    )));

    drop(daemon);
    assert!(!dir.join("serve.json").exists());
    let restarted = osserve::start(Arc::new(Orchestrator::new(
        ParziConfig::default(),
        SessionStore::open().unwrap(),
    )))
    .await
    .expect("serve restarts after the lock is released");
    drop(restarted);
    let _ = std::fs::remove_dir_all(&dir);
}
