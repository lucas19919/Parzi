// Its own test binary (and so its own PARZI_HOME): `parzi serve` holds a
// lock per home.
//
// A desk that quotes a plan item into a server session passes the item
// along with `session.send`; the server links the session to it, and the
// link reaches the desk with the session list.
mod common;

use std::sync::Arc;

use common::*;
use parzi_core::config::ParziConfig;
use parzi_providers::TurnEnd;
use parzi_runtime::osserve;
use parzi_runtime::status::StatusBoard;
use parzi_runtime::Orchestrator;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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
async fn a_quoted_item_links_the_server_session() {
    let dir = home("serve-plan-item");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.say("on it");
            Ok(TurnEnd::Completed)
        }),
    );
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "auto".into();
    let store = parzi_core::store::SessionStore::open().unwrap();
    let served = Arc::new(
        Orchestrator::new(cfg, store.clone())
            .with_source(source(&[fake]))
            .with_status(Arc::new(StatusBoard::in_memory())),
    );
    let daemon = osserve::start(served).await.expect("serve starts");
    let meta: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("serve.json")).unwrap()).unwrap();
    let token = meta["token"].as_str().unwrap().to_string();
    let sent = rpc(
        daemon.port,
        &token,
        "session.send",
        json!({
            "target": "new",
            "message": "start this\n\n<plan-item project=\"parzi\" id=\"names\" title=\"Session names\">\nState: planned. Needs: none.\n</plan-item>",
            "model": "claude",
            "plan_item": "names",
            "yes": true,
        }),
    )
    .await;
    assert_eq!(sent["ok"], json!(true), "{sent}");
    let id = sent["id"].as_str().unwrap().to_string();
    settle(&store, &id).await;
    let m = store.get(&id).unwrap();
    assert_eq!(m.plan_item.as_deref(), Some("names"));
    assert_eq!(m.title, "Start this", "named from the text, not the block");

    let again = rpc(
        daemon.port,
        &token,
        "session.send",
        json!({ "target": id, "message": "and go on", "model": "claude", "yes": true }),
    )
    .await;
    assert_eq!(again["ok"], json!(true), "{again}");
    settle(&store, &id).await;
    let listed = rpc(daemon.port, &token, "session.list", json!({})).await;
    let row = listed["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == json!(id))
        .cloned()
        .unwrap();
    assert_eq!(
        row["plan_item"],
        json!("names"),
        "a send without a quote keeps the link"
    );
    assert_eq!(row["effort"], json!("medium"));
    drop(daemon);
}
