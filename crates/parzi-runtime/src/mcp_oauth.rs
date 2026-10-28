//! Signing in to a hosted MCP server the way the MCP spec says (OAuth 2.1):
//! find the server's authorization server, register Parzi as a client, send
//! the person to sign in once in their browser (PKCE, a loopback callback
//! on 127.0.0.1), and keep the token, refreshing it when it runs out.
//! Tokens live in `~/.parzi/mcp-auth/<connector>.json`.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How long the person has to finish signing in.
const SIGN_IN_WAIT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Token {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Unix seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub token_endpoint: String,
    /// The server the token is for (OAuth's `resource`).
    pub resource: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn store_path(name: &str) -> Option<PathBuf> {
    let safe = parzi_core::connectors::connector_name(name);
    Some(
        parzi_core::paths::parzi_dir()
            .ok()?
            .join("mcp-auth")
            .join(format!("{safe}.json")),
    )
}

#[must_use]
pub fn load(name: &str) -> Option<Token> {
    serde_json::from_str(&std::fs::read_to_string(store_path(name)?).ok()?).ok()
}

fn save(name: &str, t: &Token) -> Result<(), String> {
    let path = store_path(name).ok_or("no place to keep the sign-in")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(t).map_err(|e| e.to_string())?;
    parzi_core::atomic_write(&path, text.as_bytes()).map_err(|e| e.to_string())
}

/// Signs out: the kept token goes.
pub fn forget(name: &str) {
    if let Some(p) = store_path(name) {
        let _ = std::fs::remove_file(p);
    }
}

/// PKCE's S256 challenge for a verifier.
#[must_use]
pub fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn verifier() -> String {
    // 64 characters from two random UUIDs: inside PKCE's 43 to 128.
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// Percent-encoding for a query value.
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn dec(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `scheme://host[:port]` and the path of an address.
fn split_url(url: &str) -> Option<(String, String)> {
    let (scheme, rest) = url.split_once("://")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let path = path.split(['?', '#']).next().unwrap_or("");
    Some((
        format!("{scheme}://{host}"),
        path.trim_end_matches('/').to_string(),
    ))
}

/// Where a server's protected-resource metadata may be: the address its
/// 401 named, else the well-known spots for its path and its origin.
fn resource_metadata_urls(server: &str, www_auth: Option<&str>) -> Vec<String> {
    let mut out = vec![];
    if let Some(named) = www_auth.and_then(|h| param(h, "resource_metadata")) {
        out.push(named);
    }
    if let Some((origin, path)) = split_url(server) {
        if !path.is_empty() {
            out.push(format!(
                "{origin}/.well-known/oauth-protected-resource{path}"
            ));
        }
        out.push(format!("{origin}/.well-known/oauth-protected-resource"));
    }
    out
}

/// Where an authorization server's metadata may be (RFC 8414, then OpenID).
fn auth_server_metadata_urls(issuer: &str) -> Vec<String> {
    let Some((origin, path)) = split_url(issuer) else {
        return vec![];
    };
    if path.is_empty() {
        vec![
            format!("{origin}/.well-known/oauth-authorization-server"),
            format!("{origin}/.well-known/openid-configuration"),
        ]
    } else {
        vec![
            format!("{origin}/.well-known/oauth-authorization-server{path}"),
            format!("{origin}/.well-known/openid-configuration{path}"),
            format!("{origin}{path}/.well-known/openid-configuration"),
        ]
    }
}

/// A `key="value"` (or bare `key=value`) out of a WWW-Authenticate header.
fn param(header: &str, key: &str) -> Option<String> {
    let at = header.find(&format!("{key}="))? + key.len() + 1;
    let rest = &header[at..];
    let v = if let Some(quoted) = rest.strip_prefix('"') {
        quoted.split('"').next()?
    } else {
        rest.split([',', ' ']).next()?
    };
    Some(v.to_string()).filter(|v| !v.is_empty())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into()
}

fn get_json(url: &str) -> Option<Value> {
    let mut res = agent()
        .get(url)
        .header("Accept", "application/json")
        .call()
        .ok()?;
    if res.status().as_u16() != 200 {
        return None;
    }
    serde_json::from_str(&res.body_mut().read_to_string().ok()?).ok()
}

/// What signing in needs from the server's side.
#[derive(Debug, Clone, PartialEq)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    pub register: Option<String>,
    pub scopes: Vec<String>,
}

/// Finds where to sign in for `server` (blocking: run it off the async
/// workers). `www_auth` is the header of the 401 that asked, if one did.
pub fn discover(server: &str, www_auth: Option<&str>) -> Result<Endpoints, String> {
    let resource = resource_metadata_urls(server, www_auth)
        .into_iter()
        .find_map(|u| get_json(&u));
    let scopes: Vec<String> = resource
        .as_ref()
        .and_then(|r| r.get("scopes_supported"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_string))
        .collect();
    let (origin, _) =
        split_url(server).ok_or_else(|| format!("`{server}` is not a web address"))?;
    // An older server has no resource metadata: it is its own authority.
    let issuer = resource
        .as_ref()
        .and_then(|r| r.get("authorization_servers"))
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .map_or_else(|| origin.clone(), str::to_string);
    let meta = auth_server_metadata_urls(&issuer)
        .into_iter()
        .find_map(|u| get_json(&u));
    let field = |k: &str| {
        meta.as_ref()
            .and_then(|m| m.get(k))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let (iss_origin, _) = split_url(&issuer).unwrap_or((origin, String::new()));
    let scopes = if scopes.is_empty() {
        meta.as_ref()
            .and_then(|m| m.get("scopes_supported"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_string))
            .filter(|s| s != "openid")
            .collect()
    } else {
        scopes
    };
    Ok(Endpoints {
        authorize: field("authorization_endpoint")
            .unwrap_or_else(|| format!("{iss_origin}/authorize")),
        token: field("token_endpoint").unwrap_or_else(|| format!("{iss_origin}/token")),
        register: field("registration_endpoint")
            .or_else(|| meta.is_none().then(|| format!("{iss_origin}/register"))),
        scopes,
    })
}

/// Registers Parzi as a client (RFC 7591): its id, and a secret if given.
fn register(endpoint: &str, redirect: &str) -> Result<(String, Option<String>), String> {
    let body = serde_json::json!({
        "client_name": "Parzi",
        "redirect_uris": [redirect],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    let mut res = agent()
        .post(endpoint)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .send(body.to_string())
        .map_err(|e| format!("registering with the sign-in service failed: {e}"))?;
    let status = res.status().as_u16();
    let text = res.body_mut().read_to_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(format!(
            "the sign-in service refused Parzi ({status}): {}",
            text.chars().take(200).collect::<String>()
        ));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let id = v
        .get("client_id")
        .and_then(Value::as_str)
        .ok_or("the sign-in service gave no client id")?;
    Ok((
        id.to_string(),
        v.get("client_secret")
            .and_then(Value::as_str)
            .map(str::to_string),
    ))
}

fn token_request(endpoint: &str, form: &[(&str, &str)]) -> Result<Value, String> {
    let mut res = agent()
        .post(endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .send(query(form))
        .map_err(|e| format!("the sign-in service did not answer: {e}"))?;
    let status = res.status().as_u16();
    let text = res.body_mut().read_to_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(format!(
            "the sign-in service said no ({status}): {}",
            text.chars().take(200).collect::<String>()
        ));
    }
    // A few services answer form-encoded.
    serde_json::from_str(&text).or_else(|_| {
        let o: serde_json::Map<String, Value> = text
            .split('&')
            .filter_map(|kv| {
                kv.split_once('=')
                    .map(|(k, v)| (dec(k), Value::String(dec(v))))
            })
            .collect();
        Ok(Value::Object(o))
    })
}

fn token_from(v: &Value, base: &Token) -> Result<Token, String> {
    let access = v
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or("the sign-in service gave no token")?;
    Ok(Token {
        access_token: access.to_string(),
        refresh_token: v
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| base.refresh_token.clone()),
        expires_at: v
            .get("expires_in")
            .and_then(Value::as_u64)
            .map(|s| now() + s),
        ..base.clone()
    })
}

/// A fresh token for one that ran out (blocking).
pub fn refresh(name: &str) -> Result<Token, String> {
    let old = load(name).ok_or("not signed in")?;
    let refresh = old
        .refresh_token
        .clone()
        .ok_or("the sign-in ran out; sign in again")?;
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh.as_str()),
        ("client_id", old.client_id.as_str()),
        ("resource", old.resource.as_str()),
    ];
    if let Some(secret) = old.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let fresh = token_from(&token_request(&old.token_endpoint, &form)?, &old)?;
    save(name, &fresh)?;
    Ok(fresh)
}

/// The token to send now, refreshed first when it is about to run out
/// (blocking). None when not signed in.
#[must_use]
pub fn bearer(name: &str) -> Option<String> {
    let t = load(name)?;
    let soon = t.expires_at.is_some_and(|at| at <= now() + 60);
    if soon && t.refresh_token.is_some() {
        if let Ok(fresh) = refresh(name) {
            return Some(fresh.access_token);
        }
    }
    Some(t.access_token)
}

/// The query of the request line that reached the callback, if it did.
fn callback_query(request: &str) -> Option<String> {
    let line = request.lines().next()?;
    let target = line.split_whitespace().nth(1)?;
    let (path, q) = target.split_once('?').unwrap_or((target, ""));
    (path == "/callback").then(|| q.to_string())
}

fn query_value(q: &str, key: &str) -> Option<String> {
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (dec(k) == key).then(|| dec(v))
    })
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Parzi</title><body style=\"font:15px system-ui;background:#161616;color:#ececec;display:grid;place-items:center;height:100vh;margin:0\"><p>Signed in. You can close this tab and go back to Parzi.</p>";

/// Signs in to `server` as connector `name`: `open` shows the person the
/// sign-in page (their browser). Keeps the token and returns it.
pub async fn sign_in(
    name: &str,
    server: &str,
    www_auth: Option<String>,
    open: impl FnOnce(&str) -> Result<(), String>,
) -> Result<Token, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let (server_s, redirect_s) = (server.to_string(), redirect.clone());
    let (ends, client_id, secret) = tokio::task::spawn_blocking(move || {
        let ends = discover(&server_s, www_auth.as_deref())?;
        let endpoint = ends
            .register
            .clone()
            .ok_or("this server's sign-in does not let apps register; add an API key instead")?;
        // The loopback port changes each time, so each sign-in registers
        // anew: cheap, and what the spec expects of a native app.
        let (id, secret) = register(&endpoint, &redirect_s)?;
        Ok::<_, String>((ends, id, secret))
    })
    .await
    .map_err(|e| e.to_string())??;
    let verifier = verifier();
    let state = uuid::Uuid::new_v4().simple().to_string();
    let scope = ends.scopes.join(" ");
    let mut q = vec![
        ("response_type", "code"),
        ("client_id", client_id.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("state", state.as_str()),
        ("resource", server),
    ];
    let ch = challenge(&verifier);
    q.push(("code_challenge", ch.as_str()));
    q.push(("code_challenge_method", "S256"));
    if !scope.is_empty() {
        q.push(("scope", scope.as_str()));
    }
    let sep = if ends.authorize.contains('?') {
        '&'
    } else {
        '?'
    };
    open(&format!("{}{sep}{}", ends.authorize, query(&q)))?;
    // The browser comes back here with the code (other requests, a
    // favicon say, get an empty answer).
    let code = tokio::time::timeout(SIGN_IN_WAIT, async {
        loop {
            let (mut sock, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.map_err(|e| e.to_string())?;
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let Some(q) = callback_query(&request) else {
                let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                continue;
            };
            let page = if query_value(&q, "error").is_some() {
                DONE_PAGE.replace("Signed in. You can close this tab and go back to Parzi.", "Sign-in was cancelled. You can close this tab.")
            } else {
                DONE_PAGE.to_string()
            };
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
            let _ = sock.write_all(reply.as_bytes()).await;
            if let Some(err) = query_value(&q, "error") {
                let why = query_value(&q, "error_description").unwrap_or(err);
                return Err(format!("sign-in was not finished: {why}"));
            }
            if query_value(&q, "state").as_deref() != Some(state.as_str()) {
                return Err("the sign-in answer did not match; try again".to_string());
            }
            return query_value(&q, "code").ok_or_else(|| "the sign-in gave no code".to_string());
        }
    })
    .await
    .map_err(|_| "sign-in took too long; try again".to_string())??;
    let base = Token {
        client_id: client_id.clone(),
        client_secret: secret.clone(),
        token_endpoint: ends.token.clone(),
        resource: server.to_string(),
        ..Default::default()
    };
    let name = name.to_string();
    tokio::task::spawn_blocking(move || {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("client_id", client_id.as_str()),
            ("code_verifier", verifier.as_str()),
            ("resource", base.resource.as_str()),
        ];
        if let Some(s) = secret.as_deref() {
            form.push(("client_secret", s));
        }
        let t = token_from(&token_request(&base.token_endpoint, &form)?, &base)?;
        save(&name, &t)?;
        Ok(t)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_the_rfc_example() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let v = verifier();
        assert!((43..=128).contains(&v.len()));
    }

    #[test]
    fn metadata_is_looked_for_where_the_specs_say() {
        assert_eq!(
            resource_metadata_urls(
                "https://mcp.notion.com/mcp",
                Some(
                    r#"Bearer error="invalid_token", resource_metadata="https://mcp.notion.com/.well-known/oauth-protected-resource/mcp""#
                )
            ),
            [
                "https://mcp.notion.com/.well-known/oauth-protected-resource/mcp",
                "https://mcp.notion.com/.well-known/oauth-protected-resource/mcp",
                "https://mcp.notion.com/.well-known/oauth-protected-resource",
            ]
        );
        assert_eq!(
            auth_server_metadata_urls("https://auth.example.com"),
            [
                "https://auth.example.com/.well-known/oauth-authorization-server",
                "https://auth.example.com/.well-known/openid-configuration",
            ]
        );
        assert_eq!(
            auth_server_metadata_urls("https://example.com/tenant1/")[0],
            "https://example.com/.well-known/oauth-authorization-server/tenant1"
        );
    }

    #[test]
    fn the_callback_and_its_query_read_right() {
        let req = "GET /callback?code=a%2Fb&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let q = callback_query(req).unwrap();
        assert_eq!(query_value(&q, "code").as_deref(), Some("a/b"));
        assert_eq!(query_value(&q, "state").as_deref(), Some("xyz"));
        assert!(callback_query("GET /favicon.ico HTTP/1.1\r\n\r\n").is_none());
        assert_eq!(enc("a b/c"), "a%20b%2Fc");
        assert_eq!(dec("a%20b%2Fc"), "a b/c");
    }
}
