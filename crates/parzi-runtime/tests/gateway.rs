mod common;

use std::time::Duration;

use common::*;
use futures_util::{SinkExt, StreamExt};
use parzi_providers::TurnEnd;
use parzi_runtime::devices::{self, Scope};
use parzi_runtime::osserve::{self, GatewayConfig};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

type Ws = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

async fn open(addr: std::net::SocketAddr) -> Ws {
    let sock = tokio::net::TcpStream::connect(addr).await.unwrap();
    let (ws, _) = tokio_tungstenite::client_async(format!("ws://{addr}/"), sock)
        .await
        .unwrap();
    ws
}

async fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::Text(v.to_string())).await.unwrap();
}

/// The next JSON message, or `None` once the socket closes.
async fn next(ws: &mut Ws) -> Option<Value> {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("an answer in time")?;
        match msg.ok()? {
            Message::Text(t) => return serde_json::from_str(&t).ok(),
            Message::Close(_) => return None,
            _ => {}
        }
    }
}

/// The reply with this rid, setting aside events that arrive first.
async fn reply(ws: &mut Ws, rid: u64, seen: &mut Vec<Value>) -> Value {
    loop {
        let v = next(ws).await.expect("the socket stays open");
        if v.get("rid") == Some(&json!(rid)) {
            return v;
        }
        seen.push(v);
    }
}

async fn signed_in(addr: std::net::SocketAddr, token: &str) -> (Ws, Value) {
    let mut ws = open(addr).await;
    send(&mut ws, json!({ "op": "hello", "token": token })).await;
    let hello = next(&mut ws).await.expect("a hello");
    (ws, hello)
}

#[tokio::test]
async fn a_paired_phone_reads_follows_and_acts_within_its_level() {
    let dir = home("gateway");
    GatewayConfig {
        enabled: true,
        listen: "127.0.0.1:0".into(),
    }
    .save()
    .unwrap();
    let (_, phone) = devices::issue("Pixel", Scope::Operate).unwrap();
    let (_, viewer) = devices::issue("Tablet", Scope::Read).unwrap();
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.say("hello from the server");
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, _store) = orch(&[fake]);
    let daemon = osserve::start(orch).await.expect("serve starts");
    let addr = daemon.gateway.expect("the gateway is on");
    assert!(addr.ip().is_loopback(), "{addr}");

    // A wrong token is turned away and the socket closes.
    let (mut ws, refused) = signed_in(addr, "pzd_not-a-real-token").await;
    assert_eq!(refused["error"], "bad token", "{refused}");
    assert!(next(&mut ws).await.is_none(), "closed after a bad token");

    // The phone signs in and is told who it is.
    let (mut ws, hello) = signed_in(addr, &phone).await;
    assert_eq!(hello["rpc"], "parzi", "{hello}");
    assert_eq!(hello["you"]["name"], "Pixel");
    assert_eq!(hello["you"]["scope"], "operate");

    let mut seen = vec![];
    send(&mut ws, json!({ "op": "session.list", "rid": 1 })).await;
    let listed = reply(&mut ws, 1, &mut seen).await;
    assert_eq!(listed["ok"], true, "{listed}");

    // Follow, then start a run and watch it to the end.
    send(&mut ws, json!({ "op": "subscribe", "rid": 2 })).await;
    assert_eq!(reply(&mut ws, 2, &mut seen).await["stream"], true);
    send(
        &mut ws,
        json!({ "op": "session.send", "rid": 3, "message": "hi", "target": "new", "model": "claude", "cwd": dir.display().to_string() }),
    )
    .await;
    let sent = reply(&mut ws, 3, &mut seen).await;
    assert_eq!(sent["ok"], true, "{sent}");
    let mut texts = String::new();
    let mut done = false;
    for v in seen.drain(..) {
        if let Some(ev) = v.get("event") {
            texts.push_str(ev["text"].as_str().unwrap_or(""));
            done |= ev["kind"] == "done";
        }
    }
    while !done {
        let v = next(&mut ws).await.expect("events until the run is done");
        let ev = &v["event"];
        texts.push_str(ev["text"].as_str().unwrap_or(""));
        done = ev["kind"] == "done";
    }
    assert!(texts.contains("hello from the server"), "{texts}");

    // Approving every tool at once is not the phone's call.
    send(
        &mut ws,
        json!({ "op": "session.send", "rid": 4, "message": "go", "yes": true, "cwd": dir.display().to_string() }),
    )
    .await;
    let blocked = reply(&mut ws, 4, &mut seen).await;
    assert!(
        blocked["error"]
            .as_str()
            .unwrap_or("")
            .contains("needs the admin level"),
        "{blocked}"
    );

    // A read-only device can look but not act.
    let (mut view, _) = signed_in(addr, &viewer).await;
    let mut other = vec![];
    send(&mut view, json!({ "op": "session.list", "rid": 1 })).await;
    assert_eq!(reply(&mut view, 1, &mut other).await["ok"], true);
    send(
        &mut view,
        json!({ "op": "session.send", "rid": 2, "message": "x" }),
    )
    .await;
    let denied = reply(&mut view, 2, &mut other).await;
    assert!(
        denied["error"]
            .as_str()
            .unwrap_or("")
            .contains("needs the operate level"),
        "{denied}"
    );

    // Unpaired while connected: the next request is refused and it closes.
    devices::revoke("Pixel").unwrap();
    send(&mut ws, json!({ "op": "session.list", "rid": 5 })).await;
    let gone = reply(&mut ws, 5, &mut seen).await;
    assert_eq!(gone["error"], "this device was unpaired", "{gone}");
    let (_, again) = signed_in(addr, &phone).await;
    assert_eq!(again["error"], "bad token", "{again}");
    drop(daemon);
}

#[tokio::test]
async fn tokens_are_kept_only_as_hashes() {
    let dir = home("gateway");
    let (d, token) = devices::issue("Laptop", Scope::Read).unwrap();
    let raw = std::fs::read_to_string(dir.join("devices.json")).unwrap();
    assert!(!raw.contains(&token), "the token itself is never written");
    assert!(raw.contains(&d.id));
    assert!(devices::verify(&token).is_some_and(|v| v.id == d.id));
    assert!(devices::verify(&token.replace("pzd_", "pzd_0")).is_none());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("devices.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "only this user can read it");
    }
    devices::revoke(&d.id).unwrap();
    assert!(devices::verify(&token).is_none());
}
