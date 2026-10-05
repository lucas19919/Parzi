use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use parzi_providers::ToolServer;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use crate::toolhost::ToolHost;
use crate::tools::{from_mcp, to_mcp};

const SERVER_NAME: &str = "parzi";
const PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_BODY: usize = 8 * 1024 * 1024;

type Runs = Arc<RwLock<HashMap<String, Arc<ToolHost>>>>;

pub struct McpHost {
    addr: SocketAddr,
    runs: Runs,
}

pub struct Registration {
    runs: Runs,
    token: String,
    pub server: ToolServer,
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut r) = self.runs.write() {
            r.remove(&self.token);
        }
    }
}

impl McpHost {
    pub async fn start() -> std::io::Result<Arc<Self>> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let runs: Runs = Arc::default();
        let host = Arc::new(Self {
            addr,
            runs: runs.clone(),
        });
        tokio::spawn(async move {
            loop {
                let stream = match listener.accept().await {
                    Ok((stream, _)) => stream,
                    Err(_) => {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        continue;
                    }
                };
                let runs = runs.clone();
                tokio::spawn(async move {
                    let svc = hyper::service::service_fn(move |req| handle(req, runs.clone()));
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), svc)
                        .await;
                });
            }
        });
        Ok(host)
    }

    pub fn register(&self, host: Arc<ToolHost>) -> Registration {
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        if let Ok(mut r) = self.runs.write() {
            r.insert(token.clone(), host);
        }
        Registration {
            runs: self.runs.clone(),
            server: ToolServer {
                name: SERVER_NAME.into(),
                url: format!("http://{}/mcp/{token}", self.addr),
                token: token.clone(),
            },
            token,
        }
    }
}

fn plain(code: StatusCode) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(Bytes::new()));
    *r.status_mut() = code;
    r
}

fn json_reply(v: &Value) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(Bytes::from(v.to_string())));
    r.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    r
}

fn token_of(req: &Request<Incoming>) -> Option<String> {
    if let Some(t) = req.uri().path().strip_prefix("/mcp/") {
        let t = t.trim_end_matches('/');
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    req.headers()
        .get(hyper::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string())
}

async fn handle(req: Request<Incoming>, runs: Runs) -> Result<Response<Full<Bytes>>, Infallible> {
    if let Some(origin) = req
        .headers()
        .get(hyper::header::ORIGIN)
        .and_then(|o| o.to_str().ok())
    {
        let local = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
            .iter()
            .any(|p| origin.starts_with(p));
        if !local {
            return Ok(plain(StatusCode::FORBIDDEN));
        }
    }
    let host = token_of(&req).and_then(|t| runs.read().ok()?.get(&t).cloned());
    let Some(host) = host else {
        return Ok(plain(StatusCode::NOT_FOUND));
    };
    match *req.method() {
        Method::POST => {}
        Method::DELETE => return Ok(plain(StatusCode::OK)),
        _ => {
            let mut r = plain(StatusCode::METHOD_NOT_ALLOWED);
            r.headers_mut().insert(
                hyper::header::ALLOW,
                hyper::header::HeaderValue::from_static("POST, DELETE"),
            );
            return Ok(r);
        }
    }
    let body = match Limited::new(req.into_body(), MAX_BODY).collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return Ok(plain(StatusCode::PAYLOAD_TOO_LARGE)),
    };
    let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
        return Ok(json_reply(&json!({
            "jsonrpc": "2.0",
            "id": null,
            "error": {"code": -32700, "message": "not JSON"},
        })));
    };
    let reply = match msg {
        Value::Array(batch) => {
            let mut out = vec![];
            for m in &batch {
                if let Some(r) = dispatch(m, &host).await {
                    out.push(r);
                }
            }
            (!out.is_empty()).then_some(Value::Array(out))
        }
        one => dispatch(&one, &host).await,
    };
    Ok(match reply {
        Some(v) => json_reply(&v),
        None => plain(StatusCode::ACCEPTED),
    })
}

async fn dispatch(msg: &Value, host: &ToolHost) -> Option<Value> {
    let id = msg.get("id").filter(|i| !i.is_null())?.clone();
    let method = msg.get("method").and_then(Value::as_str)?;
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let wanted = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("");
            let version = PROTOCOLS
                .iter()
                .find(|p| **p == wanted)
                .copied()
                .unwrap_or(PROTOCOLS[0]);
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": SERVER_NAME, "title": "Parzi", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Parzi's own tools: widgets and artifacts in the thread, teamwork across sessions, plans, knowledge and leases.",
            })
        }
        "ping" => json!({}),
        "tools/list" => {
            let tools: Vec<Value> = host
                .defs()
                .await
                .iter()
                .map(|d| json!({"name": to_mcp(&d.name), "description": d.description, "inputSchema": d.schema}))
                .collect();
            json!({"tools": tools})
        }
        "tools/call" => {
            let asked = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params
                .get("arguments")
                .filter(|a| !a.is_null())
                .cloned()
                .unwrap_or_else(|| json!({}));
            let name = host
                .defs()
                .await
                .iter()
                .find(|d| to_mcp(&d.name) == asked)
                .map_or_else(|| from_mcp(asked), |d| d.name.clone());
            let (ok, output) = host.call(&name, &args).await;
            json!({"content": [{"type": "text", "text": output}], "isError": !ok})
        }
        other => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("Parzi's MCP server has no method {other}")},
            }))
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}
