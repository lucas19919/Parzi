//! JSON-RPC 2.0 over newline-delimited stdio: the wire of Codex's app-server
//! and of every ACP agent. Requests run both ways (the vendor asks Parzi to
//! approve an action), so incoming requests surface next to notifications.
//!
//! Lines are decoded whole: a character split across two reads is never
//! mangled, because nothing is decoded before its newline arrives.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    /// The peer went away before answering.
    pub const CLOSED: i64 = -32099;
    /// No answer in time.
    pub const TIMEOUT: i64 = -32098;

    fn closed() -> Self {
        Self {
            code: Self::CLOSED,
            message: "the program exited before it answered".into(),
            data: None,
        }
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// A stdout line that is not JSON-RPC (some agents print sign-in links).
    Raw(String),
}

type Waiters = Arc<std::sync::Mutex<HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>>>;

pub struct Peer {
    writer: Mutex<Box<dyn AsyncWrite + Send + Unpin>>,
    waiters: Waiters,
    next: AtomicI64,
}

impl Peer {
    /// Wire a peer to a reader/writer pair. Incoming traffic that is not an
    /// answer to our own request arrives on the returned channel, which
    /// closes when the reader hits end of file.
    pub fn start<R, W>(reader: R, writer: W) -> (Arc<Self>, mpsc::UnboundedReceiver<Incoming>)
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let waiters: Waiters = Arc::default();
        let (tx, rx) = mpsc::unbounded_channel();
        let peer = Arc::new(Self {
            writer: Mutex::new(Box::new(writer)),
            waiters: waiters.clone(),
            next: AtomicI64::new(1),
        });
        tokio::spawn(read_loop(reader, waiters, tx));
        (peer, rx)
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        if let Ok(mut w) = self.waiters.lock() {
            w.insert(id, tx);
        }
        let msg = envelope(Some(id), method, params);
        if let Err(e) = self.write(&msg).await {
            if let Ok(mut w) = self.waiters.lock() {
                w.remove(&id);
            }
            return Err(e);
        }
        rx.await.unwrap_or_else(|_| Err(RpcError::closed()))
    }

    pub async fn request_within(
        &self,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value, RpcError> {
        tokio::time::timeout(limit, self.request(method, params))
            .await
            .unwrap_or_else(|_| {
                Err(RpcError {
                    code: RpcError::TIMEOUT,
                    message: format!("no answer to {method} within {}s", limit.as_secs()),
                    data: None,
                })
            })
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), RpcError> {
        self.write(&envelope(None, method, params)).await
    }

    pub async fn respond(&self, id: Value, result: Value) -> Result<(), RpcError> {
        self.write(&json!({"jsonrpc": "2.0", "id": id, "result": result}))
            .await
    }

    pub async fn respond_error(&self, id: Value, code: i64, message: &str) -> Result<(), RpcError> {
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": code, "message": message},
        }))
        .await
    }

    async fn write(&self, v: &Value) -> Result<(), RpcError> {
        let mut line = v.to_string();
        line.push('\n');
        let mut w = self.writer.lock().await;
        w.write_all(line.as_bytes())
            .await
            .map_err(|_| RpcError::closed())?;
        w.flush().await.map_err(|_| RpcError::closed())
    }
}

/// A request or notification. `params` is left out when null: JSON-RPC
/// allows only an object or array there, and strict peers refuse `null`.
fn envelope(id: Option<i64>, method: &str, params: Value) -> Value {
    let mut msg = json!({"jsonrpc": "2.0", "method": method});
    if let Some(id) = id {
        msg["id"] = json!(id);
    }
    if !params.is_null() {
        msg["params"] = params;
    }
    msg
}

async fn read_loop<R>(reader: R, waiters: Waiters, tx: mpsc::UnboundedSender<Incoming>)
where
    R: AsyncRead + Send + Unpin + 'static,
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&buf);
        let line = text.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            let _ = tx.send(Incoming::Raw(line.to_string()));
            continue;
        };
        dispatch(v, &waiters, &tx);
    }
    if let Ok(mut w) = waiters.lock() {
        for (_, waiter) in w.drain() {
            let _ = waiter.send(Err(RpcError::closed()));
        }
    }
}

fn dispatch(v: Value, waiters: &Waiters, tx: &mpsc::UnboundedSender<Incoming>) {
    let id = v.get("id").filter(|i| !i.is_null()).cloned();
    if let Some(method) = v.get("method").and_then(Value::as_str) {
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        let msg = match id {
            Some(id) => Incoming::Request {
                id,
                method: method.to_string(),
                params,
            },
            None => Incoming::Notification {
                method: method.to_string(),
                params,
            },
        };
        let _ = tx.send(msg);
        return;
    }
    let Some(n) = id.as_ref().and_then(Value::as_i64) else {
        return;
    };
    let Some(waiter) = waiters.lock().ok().and_then(|mut w| w.remove(&n)) else {
        return;
    };
    let outcome = match v.get("error") {
        Some(e) => Err(RpcError {
            code: e.get("code").and_then(Value::as_i64).unwrap_or(-32603),
            message: e
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_string(),
            data: e.get("data").cloned(),
        }),
        None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
    };
    let _ = waiter.send(outcome);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn answers_requests_and_surfaces_their_requests() {
        let (client_io, server_io) = tokio::io::duplex(4096);
        let (cr, cw) = tokio::io::split(client_io);
        let (peer, mut incoming) = Peer::start(cr, cw);
        let (sr, mut sw) = tokio::io::split(server_io);
        let server = tokio::spawn(async move {
            let mut lines = BufReader::new(sr).lines();
            let req: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(req["method"], "initialize");
            // A notification, a request of its own, a stray line, then the answer.
            sw.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"note\",\"params\":{\"a\":1}}\n")
                .await
                .unwrap();
            sw.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":\"x\",\"method\":\"ask\",\"params\":{}}\n")
                .await
                .unwrap();
            sw.write_all(b"Open this link to sign in\n").await.unwrap();
            let answer = json!({"jsonrpc": "2.0", "id": req["id"], "result": {"ok": true}});
            sw.write_all(format!("{answer}\n").as_bytes())
                .await
                .unwrap();
            let reply: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(reply["id"], "x");
            assert_eq!(reply["result"]["granted"], true);
        });
        let result = peer.request("initialize", json!({})).await.unwrap();
        assert_eq!(result["ok"], true);
        assert!(
            matches!(incoming.recv().await, Some(Incoming::Notification { method, .. }) if method == "note")
        );
        let Some(Incoming::Request { id, method, .. }) = incoming.recv().await else {
            panic!("expected the peer's request");
        };
        assert_eq!(method, "ask");
        assert!(
            matches!(incoming.recv().await, Some(Incoming::Raw(line)) if line.contains("sign in"))
        );
        peer.respond(id, json!({"granted": true})).await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_dead_peer_fails_the_waiting_request() {
        let (client_io, server_io) = tokio::io::duplex(1024);
        let (cr, cw) = tokio::io::split(client_io);
        let (peer, _incoming) = Peer::start(cr, cw);
        drop(server_io);
        let err = peer.request("x", json!({})).await.unwrap_err();
        assert_eq!(err.code, RpcError::CLOSED);
    }

    #[tokio::test]
    async fn a_split_character_survives() {
        let (client_io, server_io) = tokio::io::duplex(1024);
        let (cr, cw) = tokio::io::split(client_io);
        let (_peer, mut incoming) = Peer::start(cr, cw);
        let (_sr, mut sw) = tokio::io::split(server_io);
        let line =
            "{\"jsonrpc\":\"2.0\",\"method\":\"t\",\"params\":{\"s\":\"Grüße\"}}\n".as_bytes();
        let cut = line.iter().position(|&b| b == 0xC3).unwrap() + 1;
        sw.write_all(&line[..cut]).await.unwrap();
        sw.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        sw.write_all(&line[cut..]).await.unwrap();
        let Some(Incoming::Notification { params, .. }) = incoming.recv().await else {
            panic!("expected a notification");
        };
        assert_eq!(params["s"], "Grüße");
    }
}
