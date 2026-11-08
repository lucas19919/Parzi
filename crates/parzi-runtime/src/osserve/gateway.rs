//! The gateway: a WebSocket door into `parzi serve` for phones and other
//! devices, off unless turned on. It speaks what the SSH relay speaks, so one
//! client works over either:
//!
//! - The client's first message is `{"op":"hello","token":"pzd_…"}`, a paired
//!   device's token (see `devices.rs`). A wrong token closes the socket.
//! - The engine answers `{"rpc":"parzi","version":…,"you":{id,name,scope}}`.
//! - Then each message is a request, `{"op":…,"rid":N,…}`, answered by one
//!   message with the same `rid`; requests run side by side.
//! - `{"op":"subscribe"}` starts the events, each sent as `{"event":{…}}`.
//!
//! Each op needs a level: reading needs `read`, acting needs `operate`, and
//! the rest (approving every tool at once, deleting, settings, stopping the
//! engine) needs `admin`. The settings live in `~/.parzi/gateway.json`, on
//! this machine only.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

use super::{dispatch, EventFeed, State, MAX_REQUEST};
use crate::devices::{self, Device, Scope};

/// The port the gateway listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 7337;
const MAX_DEVICES_CONNECTED: usize = 32;
const HELLO_WAIT: Duration = Duration::from_secs(10);
/// A wrong token waits this long before the socket closes, to slow guessing.
const REFUSE_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Where to listen. The default, on this machine only, suits Tailscale
    /// Serve or an SSH tunnel in front; `0.0.0.0:7337` opens it to the local
    /// network, unencrypted.
    #[serde(default = "default_listen")]
    pub listen: String,
}

fn default_listen() -> String {
    format!("127.0.0.1:{DEFAULT_PORT}")
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: default_listen(),
        }
    }
}

fn config_path() -> Result<std::path::PathBuf, String> {
    parzi_core::paths::ensure_dirs()
        .map(|root| root.join("gateway.json"))
        .map_err(|e| e.to_string())
}

impl GatewayConfig {
    #[must_use]
    pub fn load() -> Self {
        config_path()
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        self.address()?;
        let body = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        super::write_private(&config_path()?, &body)
    }

    /// The address to listen on, checked.
    pub fn address(&self) -> Result<SocketAddr, String> {
        self.listen.trim().parse().map_err(|_| {
            format!(
                "listen `{}` is an address and port, like 127.0.0.1:{DEFAULT_PORT}",
                self.listen
            )
        })
    }
}

/// The level an op needs over the gateway.
fn needs(op: &str, req: &Value) -> Scope {
    match op {
        "health" | "subscribe" | "session.list" | "session.show" | "session.export"
        | "session.events" | "approval.list" | "providers" | "project.list" | "brain.manifest"
        | "brain.read" | "settings.get" | "mirror.list" | "mirror.events" => Scope::Read,
        // Approving every tool of a run at once is the person's call alone.
        "session.send" if req.get("yes").and_then(Value::as_bool) == Some(true) => Scope::Admin,
        "session.send" | "session.kill" | "session.fork" | "session.rename" | "session.compact"
        | "approval.answer" | "question.answer" | "mirror.adopt" | "project.folder" => {
            Scope::Operate
        }
        _ => Scope::Admin,
    }
}

pub(super) struct Gateway {
    pub addr: SocketAddr,
    pub task: JoinHandle<()>,
}

/// Starts listening as `cfg` says. The engine runs on without it if the
/// address is taken.
pub(super) async fn start(state: Arc<State>, cfg: &GatewayConfig) -> Result<Gateway, String> {
    let want = cfg.address()?;
    let listener = TcpListener::bind(want)
        .await
        .map_err(|e| format!("gateway could not listen on {want}: {e}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let open = !addr.ip().is_loopback();
    if open {
        tracing::warn!(
            "the gateway listens on {addr}, open to the network and unencrypted; prefer Tailscale Serve in front of 127.0.0.1"
        );
    }
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_DEVICES_CONNECTED));
    let stop = state.stop.clone();
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                () = stop.cancelled() => break,
                incoming = listener.accept() => {
                    let Ok((sock, peer)) = incoming else { continue };
                    if !open && !peer.ip().is_loopback() {
                        continue;
                    }
                    let Ok(slot) = slots.clone().try_acquire_owned() else { continue };
                    let state = state.clone();
                    tokio::spawn(async move {
                        if let Err(e) = connection(sock, state).await {
                            tracing::debug!("gateway connection from {peer} ended: {e}");
                        }
                        drop(slot);
                    });
                }
            }
        }
    });
    Ok(Gateway { addr, task })
}

fn text(v: &Value) -> Message {
    Message::Text(v.to_string())
}

async fn connection(sock: TcpStream, state: Arc<State>) -> Result<(), String> {
    let limits = WebSocketConfig {
        max_message_size: Some(usize::try_from(MAX_REQUEST).unwrap_or(usize::MAX)),
        max_frame_size: Some(usize::try_from(MAX_REQUEST).unwrap_or(usize::MAX)),
        ..WebSocketConfig::default()
    };
    let ws = tokio::time::timeout(
        HELLO_WAIT,
        tokio_tungstenite::accept_async_with_config(sock, Some(limits)),
    )
    .await
    .map_err(|_| "handshake timed out".to_string())?
    .map_err(|e| e.to_string())?;
    let (mut sink, mut source) = ws.split();

    let Ok(Some(device)) = tokio::time::timeout(HELLO_WAIT, hello(&mut source)).await else {
        tokio::time::sleep(REFUSE_DELAY).await;
        let _ = sink
            .send(text(&json!({ "ok": false, "error": "bad token" })))
            .await;
        let _ = sink.close().await;
        return Err("refused".into());
    };
    sink.send(text(&json!({
        "rpc": "parzi",
        "version": env!("CARGO_PKG_VERSION"),
        "device": parzi_core::device::this(),
        "you": { "id": device.id, "name": device.name, "scope": device.scope.as_str() },
    })))
    .await
    .map_err(|e| e.to_string())?;

    // One writer, so replies and events never interleave inside a message.
    let (tx, mut rx) = mpsc::channel::<Value>(256);
    let writer = tokio::spawn(async move {
        while let Some(v) = rx.recv().await {
            if sink.send(text(&v)).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let mut follower: Option<JoinHandle<()>> = None;
    let mut inflight = tokio::task::JoinSet::new();

    while let Some(msg) = source.next().await {
        let msg = msg.map_err(|e| e.to_string())?;
        let raw = match msg {
            Message::Text(t) => t,
            Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
            Message::Close(_) => break,
            _ => continue,
        };
        while inflight.try_join_next().is_some() {}
        let Ok(Value::Object(mut req)) = serde_json::from_str::<Value>(&raw) else {
            let _ = tx
                .send(json!({ "ok": false, "error": "not a JSON object" }))
                .await;
            continue;
        };
        let rid = req.remove("rid").unwrap_or(Value::Null);
        req.remove("token");
        let op = req
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let req = Value::Object(req);
        let need = needs(&op, &req);
        if device.scope < need {
            let _ = tx
                .send(json!({
                    "rid": rid,
                    "ok": false,
                    "error": format!("`{op}` needs the {} level; this device has {}", need.as_str(), device.scope.as_str()),
                }))
                .await;
            continue;
        }
        if op == "subscribe" {
            if follower.as_ref().is_none_or(JoinHandle::is_finished) {
                let mut feed = EventFeed::new(&state);
                let tx = tx.clone();
                follower = Some(tokio::spawn(async move {
                    while let Some(ev) = feed.next().await {
                        if tx.send(json!({ "event": ev })).await.is_err() {
                            break;
                        }
                    }
                }));
            }
            let _ = tx
                .send(json!({ "rid": rid, "ok": true, "stream": true }))
                .await;
            continue;
        }
        // A device that was unpaired while connected stops here.
        if devices::verify_id(&device.id).is_none() {
            let _ = tx
                .send(json!({ "rid": rid, "ok": false, "error": "this device was unpaired" }))
                .await;
            break;
        }
        let state = state.clone();
        let tx = tx.clone();
        inflight.spawn(async move {
            let wait = crate::provision::op_wait(&op);
            let mut reply = match tokio::time::timeout(wait, dispatch(&state, &req)).await {
                Ok(v) => v,
                Err(_) => json!({ "ok": false, "error": format!("`{op}` took longer than {}s", wait.as_secs()) }),
            };
            reply["rid"] = rid;
            let _ = tx.send(reply).await;
        });
    }
    if let Some(f) = follower {
        f.abort();
    }
    inflight.abort_all();
    drop(tx);
    let _ = writer.await;
    Ok(())
}

/// Reads the hello and returns the paired device it names.
async fn hello<S>(source: &mut S) -> Option<Device>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match source.next().await?.ok()? {
            Message::Text(t) => {
                let v: Value = serde_json::from_str(&t).ok()?;
                if v.get("op").and_then(Value::as_str) != Some("hello") {
                    return None;
                }
                return devices::verify(v.get("token").and_then(Value::as_str).unwrap_or(""));
            }
            Message::Ping(_) | Message::Pong(_) => {}
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_op_needs_the_least_level_that_is_safe() {
        assert_eq!(needs("session.list", &json!({})), Scope::Read);
        assert_eq!(needs("subscribe", &json!({})), Scope::Read);
        assert_eq!(needs("session.send", &json!({})), Scope::Operate);
        assert_eq!(needs("approval.answer", &json!({})), Scope::Operate);
        assert_eq!(needs("session.send", &json!({ "yes": true })), Scope::Admin);
        for op in [
            "session.delete",
            "settings.put",
            "brain.apply",
            "shutdown",
            "mirror.put",
            "doctor",
            "made.up",
        ] {
            assert_eq!(needs(op, &json!({})), Scope::Admin, "{op}");
        }
    }

    #[test]
    fn the_default_stays_on_this_machine() {
        let cfg = GatewayConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.address().unwrap().ip().is_loopback());
        assert!(GatewayConfig {
            enabled: true,
            listen: "nowhere".into()
        }
        .address()
        .is_err());
    }
}
