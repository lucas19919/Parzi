use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};

pub(crate) const MAX_LINE: usize = 32 * 1024 * 1024;
const KEEP_BUF: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    pub const CLOSED: i64 = -32099;
    pub const TIMEOUT: i64 = -32098;
    // ACP's resource-not-found.
    pub const NOT_FOUND: i64 = -32002;

    fn closed(who: &str) -> Self {
        Self {
            code: Self::CLOSED,
            message: format!("{who} exited before it answered"),
            data: None,
        }
    }

    // Only a plain "it is gone" may make the caller forget a conversation.
    pub fn not_found(&self) -> bool {
        if self.code == Self::NOT_FOUND {
            return true;
        }
        if self.code == Self::CLOSED || self.code == Self::TIMEOUT {
            return false;
        }
        let text = self.to_string().to_lowercase();
        [
            "not found",
            "no such",
            "unknown session",
            "unknown thread",
            "does not exist",
            "no rollout found",
        ]
        .iter()
        .any(|p| text.contains(p))
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        let detail = match &self.data {
            None | Some(Value::Null) => return Ok(()),
            Some(Value::String(s)) => s.trim().to_string(),
            Some(v) => v
                .get("message")
                .or_else(|| v.get("details"))
                .and_then(Value::as_str)
                .map_or_else(|| v.to_string(), str::to_string),
        };
        if detail.is_empty() || detail == self.message {
            return Ok(());
        }
        let short: String = detail.chars().take(400).collect();
        write!(f, ": {short}")
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
    Raw(String),
}

type Waiters = Arc<std::sync::Mutex<HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>>>;

// Removes the waiter however the request ends: answered, failed, timed out or dropped.
struct Waiting<'a> {
    waiters: &'a Waiters,
    id: i64,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if let Ok(mut w) = self.waiters.lock() {
            w.remove(&self.id);
        }
    }
}

pub struct Peer {
    who: &'static str,
    writer: Mutex<Box<dyn AsyncWrite + Send + Unpin>>,
    waiters: Waiters,
    next: AtomicI64,
}

impl Peer {
    pub fn start<R, W>(
        who: &'static str,
        reader: R,
        writer: W,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<Incoming>)
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let waiters: Waiters = Arc::default();
        let (tx, rx) = mpsc::unbounded_channel();
        let peer = Arc::new(Self {
            who,
            writer: Mutex::new(Box::new(writer)),
            waiters: waiters.clone(),
            next: AtomicI64::new(1),
        });
        tokio::spawn(read_loop(who, reader, waiters, tx));
        (peer, rx)
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        if let Ok(mut w) = self.waiters.lock() {
            w.insert(id, tx);
        }
        let _waiting = Waiting {
            waiters: &self.waiters,
            id,
        };
        self.write(&envelope(Some(id), method, params)).await?;
        rx.await.unwrap_or_else(|_| Err(RpcError::closed(self.who)))
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
        let failed = |e: std::io::Error| RpcError {
            code: RpcError::CLOSED,
            message: format!("could not write to {}: {e}", self.who),
            data: None,
        };
        let mut w = self.writer.lock().await;
        w.write_all(line.as_bytes()).await.map_err(failed)?;
        w.flush().await.map_err(failed)
    }
}

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

pub(crate) enum Line {
    Eof,
    Text,
    Dropped(usize),
}

// One line into `buf`, at most `max` bytes; a longer one is read through to
// its newline and dropped so a runaway child cannot grow us without bound.
pub(crate) async fn read_line<R>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max: usize,
) -> std::io::Result<Line>
where
    R: AsyncBufRead + Unpin + ?Sized,
{
    buf.clear();
    if buf.capacity() > KEEP_BUF {
        buf.shrink_to(KEEP_BUF);
    }
    let mut dropped = 0usize;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(match dropped {
                0 if buf.is_empty() => Line::Eof,
                0 => Line::Text,
                n => Line::Dropped(n),
            });
        }
        let (take, end) = match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if dropped == 0 && buf.len() + take <= max {
            buf.extend_from_slice(&chunk[..take]);
        } else {
            dropped += buf.len() + take;
            buf.clear();
            buf.shrink_to(KEEP_BUF);
        }
        reader.consume(take);
        if end {
            return Ok(if dropped == 0 {
                Line::Text
            } else {
                Line::Dropped(dropped)
            });
        }
    }
}

async fn read_loop<R>(
    who: &'static str,
    reader: R,
    waiters: Waiters,
    tx: mpsc::UnboundedSender<Incoming>,
) where
    R: AsyncRead + Send + Unpin + 'static,
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        match read_line(&mut reader, &mut buf, MAX_LINE).await {
            Ok(Line::Eof) | Err(_) => break,
            Ok(Line::Dropped(n)) => {
                tracing::warn!("dropped a {n}-byte line from {who}: over the 32 MiB cap");
                continue;
            }
            Ok(Line::Text) => {}
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
            let _ = waiter.send(Err(RpcError::closed(who)));
        }
    }
}

fn dispatch(mut v: Value, waiters: &Waiters, tx: &mpsc::UnboundedSender<Incoming>) {
    let id = v.get_mut("id").map(Value::take).filter(|i| !i.is_null());
    if let Some(method) = v.get("method").and_then(Value::as_str).map(str::to_string) {
        let params = v.get_mut("params").map_or(Value::Null, Value::take);
        let msg = match id {
            Some(id) => Incoming::Request { id, method, params },
            None => Incoming::Notification { method, params },
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
    let outcome = match v.get_mut("error").map(Value::take) {
        Some(mut e) => Err(RpcError {
            code: e.get("code").and_then(Value::as_i64).unwrap_or(-32603),
            message: e
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_string(),
            data: e.get_mut("data").map(Value::take),
        }),
        None => Ok(v.get_mut("result").map_or(Value::Null, Value::take)),
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
        let (peer, mut incoming) = Peer::start("Test", cr, cw);
        let (sr, mut sw) = tokio::io::split(server_io);
        let server = tokio::spawn(async move {
            let mut lines = BufReader::new(sr).lines();
            let req: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(req["method"], "initialize");
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
        let (peer, _incoming) = Peer::start("Test", cr, cw);
        drop(server_io);
        let err = peer.request("x", json!({})).await.unwrap_err();
        assert_eq!(err.code, RpcError::CLOSED);
        assert!(err.message.contains("Test"), "names the program: {err}");
        assert!(peer.waiters.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_timed_out_request_leaves_no_waiter() {
        let (client_io, _server_io) = tokio::io::duplex(1 << 16);
        let (cr, cw) = tokio::io::split(client_io);
        let (peer, _incoming) = Peer::start("Test", cr, cw);
        let err = peer
            .request_within("x", json!({}), Duration::from_millis(20))
            .await
            .unwrap_err();
        assert_eq!(err.code, RpcError::TIMEOUT);
        assert!(peer.waiters.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_oversize_line_is_dropped_and_the_next_one_read() {
        let data: &[u8] = b"0123456789abcdef\nshort\ntail";
        let mut r = BufReader::with_capacity(4, data);
        let mut buf = Vec::with_capacity(1 << 20);
        assert!(matches!(
            read_line(&mut r, &mut buf, 8).await.unwrap(),
            Line::Dropped(17)
        ));
        assert!(buf.is_empty() && buf.capacity() <= KEEP_BUF);
        assert!(matches!(
            read_line(&mut r, &mut buf, 8).await.unwrap(),
            Line::Text
        ));
        assert_eq!(buf, b"short\n");
        assert!(matches!(
            read_line(&mut r, &mut buf, 8).await.unwrap(),
            Line::Text
        ));
        assert_eq!(buf, b"tail");
        assert!(matches!(
            read_line(&mut r, &mut buf, 8).await.unwrap(),
            Line::Eof
        ));
    }

    #[test]
    fn only_a_plain_not_found_loses_a_conversation() {
        let e = |code, message: &str| RpcError {
            code,
            message: message.into(),
            data: None,
        };
        assert!(e(-32600, "thread not found: th1").not_found());
        assert!(e(-32603, "no rollout found for thread id th1").not_found());
        assert!(e(RpcError::NOT_FOUND, "Resource").not_found());
        assert!(!e(-32603, "Internal error").not_found());
        assert!(!e(-32603, "model is overloaded").not_found());
        assert!(!e(RpcError::TIMEOUT, "no answer to x within 60s").not_found());
        let in_data = RpcError {
            code: -32603,
            message: "Internal error".into(),
            data: Some(json!({"message": "Session abc does not exist"})),
        };
        assert!(in_data.not_found());
    }

    #[tokio::test]
    async fn a_split_character_survives() {
        let (client_io, server_io) = tokio::io::duplex(1024);
        let (cr, cw) = tokio::io::split(client_io);
        let (_peer, mut incoming) = Peer::start("Test", cr, cw);
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
