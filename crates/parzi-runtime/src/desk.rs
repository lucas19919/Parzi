use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub fn gui_path() -> Option<std::path::PathBuf> {
    parzi_core::paths::parzi_dir()
        .ok()
        .map(|d| d.join("gui.json"))
}

pub async fn call_within(op: &str, mut body: Value, wait: Duration) -> Result<Value, String> {
    let path = gui_path().ok_or_else(|| "Parzi is not open".to_string())?;
    let raw = std::fs::read_to_string(&path).map_err(|_| "Parzi is not open".to_string())?;
    let meta: Value = serde_json::from_str(&raw).map_err(|_| "Parzi is not open".to_string())?;
    let port = meta
        .get("port")
        .and_then(Value::as_u64)
        .ok_or_else(|| "Parzi is not open".to_string())?;
    let token = meta
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !body.is_object() {
        body = json!({});
    }
    body["op"] = json!(op);
    body["token"] = json!(token);
    let line = serde_json::to_string(&body).map_err(|e| e.to_string())?;

    let stream = tokio::time::timeout(
        Duration::from_millis(400),
        tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")),
    )
    .await
    .map_err(|_| "Parzi is not open".to_string())?
    .map_err(|_| "Parzi is not open".to_string())?;

    let (read, mut write) = stream.into_split();
    write
        .write_all(line.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    write.write_all(b"\n").await.map_err(|e| e.to_string())?;
    write.shutdown().await.map_err(|e| e.to_string())?;

    let mut reader = BufReader::new(read);
    let mut buf = String::new();
    tokio::time::timeout(wait, reader.read_line(&mut buf))
        .await
        .map_err(|_| "the window did not answer".to_string())?
        .map_err(|e| e.to_string())?;
    if buf.trim().is_empty() {
        return Err("the window did not answer".into());
    }
    let v: Value = serde_json::from_str(&buf).map_err(|e| e.to_string())?;
    if v.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(v
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("failed")
            .to_string());
    }
    Ok(v)
}

pub async fn call(op: &str, body: Value) -> Result<Value, String> {
    call_within(op, body, Duration::from_secs(8)).await
}

#[must_use]
pub fn normalize_url(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.starts_with("http://") || t.starts_with("https://") {
        return t.to_string();
    }
    let host = t.split(['/', '?', '#']).next().unwrap_or("");
    let bare = host.split(':').next().unwrap_or("");
    if bare == "localhost" || bare == "127.0.0.1" {
        return format!("http://{t}");
    }
    if t.contains('.') && !t.contains(' ') {
        return format!("https://{t}");
    }
    let mut q = String::new();
    for c in t.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => q.push(c),
            ' ' => q.push('+'),
            other => {
                let mut buf = [0; 4];
                for b in other.encode_utf8(&mut buf).as_bytes() {
                    q.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    format!("https://duckduckgo.com/?q={q}")
}

#[cfg(test)]
mod tests {
    use super::normalize_url;

    #[test]
    fn plain_http_only_for_a_local_host() {
        assert_eq!(normalize_url("localhost:1420/x"), "http://localhost:1420/x");
        assert_eq!(normalize_url("127.0.0.1:8080"), "http://127.0.0.1:8080");
        assert_eq!(
            normalize_url("localhost.evil.com"),
            "https://localhost.evil.com"
        );
        assert_eq!(normalize_url("github.com"), "https://github.com");
        assert_eq!(
            normalize_url("rust borrow checker"),
            "https://duckduckgo.com/?q=rust+borrow+checker"
        );
    }
}
