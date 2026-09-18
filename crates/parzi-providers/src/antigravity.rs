//! Antigravity adapter (Google Cloud Code endpoint + Google OAuth).
//!
//! Core logic ported from MIT-licensed `opencode-antigravity-auth`
//! (attribution in NOTICE below) — translated to Rust:
//! OAuth token -> account select -> endpoint fallback -> wrap
//! `{project, model, request}` -> Claude<->Gemini transform ->
//! thinking-strip -> schema allowlist -> SSE transform.
//!
//! NOTICE: request/response shapes derived from opencode-antigravity-auth
//! (MIT (c) its authors). See THIRD_PARTY_NOTICES.md.
//!
//! TOS WARNING: unofficial path, ban reports exist. Opt-in only in Settings,
//! isolated so its failure never affects other providers.

use futures::StreamExt;
use parzi_core::context::Role;
use parzi_core::error::{ParziError, Result};

use crate::antigravity_oauth;
use crate::types::{
    env_key, keyring_get, read_json_file, AuthStatus, Billing, ChatReq, EventRx, Model, Provider,
    StreamEvent,
};

pub fn is_valid_gcp_project(id: &str) -> bool {
    let len = id.len();
    if !(6..=30).contains(&len) {
        return false;
    }
    let bytes = id.as_bytes();
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    if bytes[len - 1] == b'-' {
        return false;
    }
    bytes
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

const HOSTS: &[&str] = &[
    "https://cloudcode-pa.googleapis.com",
    "https://autopush-cloudcode-pa.sandbox.googleapis.com",
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
];

pub struct Antigravity {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub project_id: Option<String>,
}

impl Antigravity {
    pub fn new() -> Self {
        let access_token =
            keyring_get("antigravity").or_else(|| env_key("ANTIGRAVITY_ACCESS_TOKEN"));
        let refresh_token =
            keyring_get("antigravity-refresh").or_else(|| env_key("ANTIGRAVITY_REFRESH_TOKEN"));
        let project_id = env_key("ANTIGRAVITY_PROJECT_ID")
            .filter(|p| is_valid_gcp_project(p))
            .or_else(|| first_account_project());
        Self {
            access_token,
            refresh_token,
            project_id,
        }
    }

    fn headers(&self, access: &str) -> Result<reqwest::header::HeaderMap> {
        let mut h = antigravity_oauth::headers();
        h.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {access}")
                .parse()
                .map_err(|_| ParziError::Provider("antigravity".into(), "bad token".into()))?,
        );
        Ok(h)
    }

    /// Exchange the stored refresh token for a fresh access token (keyring updated).
    async fn refresh_now(&mut self) -> Result<()> {
        let rt = self.refresh_token.clone().ok_or_else(|| {
            ParziError::Provider(
                "antigravity".into(),
                "no refresh token — run `parzi login`".into(),
            )
        })?;
        let toks = antigravity_oauth::refresh_access(&rt).await?;
        if toks.access.is_empty() {
            return Err(ParziError::Provider(
                "antigravity".into(),
                "refresh rejected".into(),
            ));
        }
        self.access_token = Some(toks.access.clone());
        if let Ok(entry) = keyring::Entry::new("parzi", "antigravity") {
            let _ = entry.set_password(&toks.access);
        }
        if let Some(r) = toks.refresh {
            self.refresh_token = Some(r.clone());
            if let Ok(entry) = keyring::Entry::new("parzi", "antigravity-refresh") {
                let _ = entry.set_password(&r);
            }
        }
        Ok(())
    }
}

impl Default for Antigravity {
    fn default() -> Self {
        Self::new()
    }
}

/// Best-effort project id from the accounts file (read-only).
fn first_account_project() -> Option<String> {
    let v = read_json_file(&dirs::home_dir()?.join(".config/opencode/antigravity-accounts.json"))?;
    v.get("accounts")?
        .as_array()?
        .iter()
        .filter_map(|a| a.get("projectId")?.as_str().map(str::to_string))
        .next()
}

/// JSON Schema allowlist for Antigravity's strict validator.
/// Kept: type/properties/required/description/enum/items.
/// String `const` becomes single-value `enum`; non-string consts are
/// dropped (Gemini's `enum` is `repeated string` — a numeric entry 400s,
/// as proven live by `ui.show_widget`'s `"const": 1`). Everything else drops.
pub fn clean_schema(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            if let Some(t) = map.get("type") {
                out.insert("type".into(), t.clone());
            }
            if let Some(d) = map.get("description") {
                out.insert("description".into(), d.clone());
            }
            if let Some(e) = map.get("enum") {
                out.insert("enum".into(), e.clone());
            } else if let Some(c) = map.get("const").and_then(|c| c.as_str()) {
                out.insert("enum".into(), serde_json::json!([c]));
            }
            if let Some(p) = map.get("properties").and_then(|p| p.as_object()) {
                let cleaned: serde_json::Map<String, serde_json::Value> = p
                    .iter()
                    .map(|(k, v)| (k.clone(), clean_schema(v)))
                    .collect();
                out.insert("properties".into(), cleaned.into());
            }
            if let Some(r) = map.get("required") {
                out.insert("required".into(), r.clone());
            }
            if let Some(items) = map.get("items") {
                out.insert("items".into(), clean_schema(items));
            }
            if out.get("type").and_then(|t| t.as_str()) == Some("object")
                && !out.contains_key("properties")
            {
                out.insert(
                    "properties".into(),
                    serde_json::json!({"reason": {"type": "string"}}),
                );
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(a) => a.iter().map(clean_schema).collect(),
        other => other.clone(),
    }
}

fn to_contents(req: &ChatReq) -> Vec<serde_json::Value> {
    req.messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::Assistant => "model",
                _ => "user",
            };
            serde_json::json!({"role": role, "parts": crate::images::gemini_parts(&m.content, &m.images)})
        })
        .collect()
}

/// Map a family base to the effort variant `agy models` actually serves.
/// Explicit variant ids pass through untouched.
pub fn with_effort(model: &str, effort: &str) -> String {
    const TRIPLE: &[&str] = &["gemini-3.8-flash", "gemini-3.7-flash", "gemini-3.6-flash"];
    if TRIPLE.contains(&model) {
        // Only low/medium/high variants exist: extra and ultra ride high
        // plus a bigger output budget.
        let suffix = match effort {
            "low" => "low",
            "medium" | "med" => "medium",
            _ => "high",
        };
        return format!("{model}-{suffix}");
    }
    if model == "gemini-3.1-pro" {
        return format!("{model}-{}", if effort == "low" { "low" } else { "high" });
    }
    model.to_string()
}

fn wrap(req: &ChatReq, project: &Option<String>) -> serde_json::Value {
    let decls: Vec<serde_json::Value> = req
        .tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "parameters": clean_schema(&t.schema),
            })
        })
        .collect();
    let mut inner = serde_json::json!({
        "contents": to_contents(req),
        "systemInstruction": {"parts": [{"text": req.system}]},
        "generationConfig": {"maxOutputTokens": req.max_tokens},
    });
    if !decls.is_empty() {
        inner["tools"] = serde_json::json!([{"functionDeclarations": decls}]);
    }
    let model_id = with_effort(&req.model, &req.effort);
    if model_id.contains("thinking") || model_id.starts_with("claude-sonnet-4-6") {
        inner["generationConfig"]["thinkingConfig"] = serde_json::json!({"thinkingLevel": "high"});
    }
    serde_json::json!({
        "project": project.clone().unwrap_or_else(|| "rising-fact-p41fc".into()),
        "model": model_id,
        "request": inner,
    })
}

#[async_trait::async_trait]
impl Provider for Antigravity {
    fn id(&self) -> &'static str {
        "antigravity"
    }

    async fn models(&self) -> Result<Vec<Model>> {
        Ok(crate::catalog::antigravity())
    }

    fn health(&self) -> crate::types::ProviderHealth {
        let (status, err) = match self.auth_status() {
            AuthStatus::Ok => ("ok".to_string(), None),
            AuthStatus::Missing(m) => ("missing".to_string(), Some(m)),
            AuthStatus::Expired(e) => ("expired".to_string(), Some(e)),
        };
        crate::types::ProviderHealth {
            provider: "antigravity".to_string(),
            status,
            cooldown_until: None,
            last_error: err,
            active_account: self.account_label(),
            tier: self.billing().as_str().to_string(),
        }
    }

    async fn chat_stream(&self, req: ChatReq) -> Result<EventRx> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        // Ensure a usable access token: stored one, else refresh now.
        let mut this = Antigravity {
            access_token: self.access_token.clone(),
            refresh_token: self.refresh_token.clone(),
            project_id: self.project_id.clone(),
        };
        if this.access_token.is_none() && this.refresh_token.is_some() {
            if let Err(e) = this.refresh_now().await {
                let _ = tx.send(Err(e));
                return Ok(rx);
            }
        }
        let access = match this.access_token.clone() {
            Some(a) => a,
            None => {
                let _ = tx.send(Err(ParziError::Provider(
                    "antigravity".into(),
                    "not signed in — run `parzi login` (opt-in)".into(),
                )));
                return Ok(rx);
            }
        };
        let headers = match this.headers(&access) {
            Ok(h) => h,
            Err(e) => {
                let _ = tx.send(Err(e));
                return Ok(rx);
            }
        };
        let body = wrap(&req, &self.project_id);
        let can_refresh = this.refresh_token.is_some();
        tokio::spawn(async move {
            // Attempt 1; on 401/403 with a refresh token, refresh once and retry.
            match post_once(&headers, &body).await {
                Ok(resp) => {
                    stream_response(resp, tx.clone()).await;
                }
                Err(fail) if (fail.code == 401 || fail.code == 403) && can_refresh => {
                    match antigravity_oauth::refresh_access(
                        &this.refresh_token.clone().unwrap_or_default(),
                    )
                    .await
                    {
                        Ok(toks) if !toks.access.is_empty() => {
                            if let Ok(entry) = keyring::Entry::new("parzi", "antigravity") {
                                let _ = entry.set_password(&toks.access);
                            }
                            match this.headers(&toks.access) {
                                Ok(h2) => match post_once(&h2, &body).await {
                                    Ok(resp) => stream_response(resp, tx.clone()).await,
                                    Err(fail) => {
                                        let msg = if fail.code == 401 || fail.code == 403 {
                                            if fail.detail.is_empty() {
                                                format!(
                                                    "auth rejected after refresh ({})",
                                                    fail.code
                                                )
                                            } else {
                                                format!(
                                                    "auth rejected after refresh ({}): {}",
                                                    fail.code, fail.detail
                                                )
                                            }
                                        } else {
                                            fail_message(&fail)
                                        };
                                        let _ = tx.send(Err(ParziError::Provider(
                                            "antigravity".into(),
                                            msg,
                                        )));
                                    }
                                },
                                Err(e) => {
                                    let _ = tx.send(Err(e));
                                }
                            }
                        }
                        _ => {
                            let _ = tx.send(Err(ParziError::Provider(
                                "antigravity".into(),
                                "token expired — run `parzi login` again".into(),
                            )));
                        }
                    }
                }
                Err(fail) => {
                    let _ = tx.send(Err(ParziError::Provider(
                        "antigravity".into(),
                        fail_message(&fail),
                    )));
                }
            }
        });
        Ok(rx)
    }

    fn billing(&self) -> Billing {
        if self.access_token.is_some() || self.refresh_token.is_some() {
            Billing::Subscription
        } else {
            Billing::None
        }
    }

    fn account_label(&self) -> Option<String> {
        if let Some(ref p) = self.project_id {
            Some(format!("Google ({p})"))
        } else if self.access_token.is_some() || self.refresh_token.is_some() {
            Some("Google".into())
        } else {
            None
        }
    }

    fn auth_status(&self) -> AuthStatus {
        if self.access_token.is_some() || self.refresh_token.is_some() {
            AuthStatus::Ok
        } else {
            AuthStatus::Missing(
                "run `parzi login` to sign in with Google (opt-in; see TOS warning)".into(),
            )
        }
    }
}

/// Richest failure seen across the host fallback chain. A 429/5xx status
/// beats transport errors, so the router can hop instead of dying on `Err(0)`.
struct PostFail {
    code: u16,
    retry_after: Option<String>,
    detail: String,
}

/// Routing-contract message: 429/overload codes must carry retriable
/// keywords (`router::is_retriable`) and any numeric `retry-after` window
/// (`router::parse_cooldown_secs` reads `retry after Ns`).
fn fail_message(fail: &PostFail) -> String {
    let retry = fail
        .retry_after
        .as_ref()
        .map(|r| format!(" (retry after {r})"))
        .unwrap_or_default();
    let detail = if fail.detail.is_empty() {
        String::new()
    } else {
        format!(": {}", fail.detail)
    };
    match fail.code {
        0 => format!("endpoints unreachable{detail}"),
        429 => format!("http 429: rate limited{retry}{detail}"),
        c => format!("http {c}{retry}{detail}"),
    }
}

/// POST to the first healthy host. Trying the next host on a per-host
/// failure is right — but if all hosts fail, the *status* (plus
/// `retry-after` and a body snippet) is returned, never a bare code 0
/// that would hide a rate limit from the failover router.
async fn post_once(
    headers: &reqwest::header::HeaderMap,
    body: &serde_json::Value,
) -> std::result::Result<reqwest::Response, PostFail> {
    let client = reqwest::Client::builder()
        .default_headers(headers.clone())
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| PostFail {
            code: 0,
            retry_after: None,
            detail: e.to_string(),
        })?;
    let mut last: Option<PostFail> = None;
    for host in HOSTS {
        let url = format!("{host}/v1internal:streamGenerateContent?alt=sse");
        match client.post(&url).json(body).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => {
                let code = resp.status().as_u16();
                let retry_after = resp
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                let detail = resp
                    .text()
                    .await
                    .unwrap_or_default()
                    .chars()
                    .take(300)
                    .collect::<String>()
                    .trim()
                    .to_string();
                if code == 401 || code == 403 {
                    return Err(PostFail {
                        code,
                        retry_after,
                        detail,
                    });
                }
                // A real status is always more informative than a transport error.
                last = Some(PostFail {
                    code,
                    retry_after,
                    detail,
                });
            }
            Err(e) => {
                if last.as_ref().map(|f| f.code == 0).unwrap_or(true) {
                    last = Some(PostFail {
                        code: 0,
                        retry_after: None,
                        detail: e.to_string(),
                    });
                }
            }
        }
    }
    Err(last.unwrap_or(PostFail {
        code: 0,
        retry_after: None,
        detail: "no hosts attempted".into(),
    }))
}

async fn stream_response(resp: reqwest::Response, tx: crate::types::EventTx) {
    let mut buf = String::new();
    let mut stream = resp.bytes_stream();
    let mut calls: Vec<(String, String, String)> = vec![];
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(Err(ParziError::Provider(
                    "antigravity".into(),
                    format!("stream: {e}"),
                )));
                return;
            }
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
            let data = match line.strip_prefix("data:") {
                Some(d) => d.trim(),
                None => continue,
            };
            let v: serde_json::Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // Envelope unwrap: inner `response` object.
            let inner = v.get("response").unwrap_or(&v);
            if let Some(u) = inner.get("usageMetadata") {
                let pin = u
                    .get("promptTokenCount")
                    .and_then(|n| n.as_u64())
                    .unwrap_or(0);
                let pout = u
                    .get("candidatesTokenCount")
                    .and_then(|n| n.as_u64())
                    .unwrap_or(0);
                let _ = tx.send(Ok(StreamEvent::Usage {
                    tokens_in: pin,
                    tokens_out: pout,
                }));
            }
            let parts = inner
                .get("candidates")
                .and_then(|c| c.as_array())
                .and_then(|c| c.first())
                .and_then(|c| c.get("content"))
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default();
            for p in parts {
                if let Some(call) = p.get("functionCall") {
                    let name = call
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let args = call.get("args").cloned().unwrap_or(serde_json::Value::Null);
                    calls.push((
                        format!("call_{}", calls.len()),
                        name,
                        serde_json::to_string(&args).unwrap_or_default(),
                    ));
                } else if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                    // `thought: true` parts are reasoning: rail, not body text.
                    if p.get("thought").and_then(|b| b.as_bool()).unwrap_or(false) {
                        let _ = tx.send(Ok(StreamEvent::Reasoning(t.to_string())));
                    } else {
                        let _ = tx.send(Ok(StreamEvent::Text(t.to_string())));
                    }
                }
            }
        }
    }
    for (id, name, args_str) in calls {
        if name.is_empty() {
            continue;
        }
        let args = serde_json::from_str(&args_str).unwrap_or(serde_json::Value::Null);
        let _ = tx.send(Ok(StreamEvent::ToolCall { id, name, args }));
    }
}

#[cfg(test)]
mod tests {
    use super::{fail_message, PostFail};
    use crate::router::{is_retriable, parse_cooldown_secs};

    fn fail(code: u16, retry_after: Option<&str>, detail: &str) -> PostFail {
        PostFail {
            code,
            retry_after: retry_after.map(|s| s.to_string()),
            detail: detail.to_string(),
        }
    }

    #[test]
    fn rate_limit_message_hops_with_exact_cooldown() {
        let msg = fail_message(&fail(429, Some("45"), "quota exceeded"));
        assert!(is_retriable(&msg), "429 must be retriable: {msg}");
        assert_eq!(parse_cooldown_secs(&msg), Some(45));
    }

    #[test]
    fn overload_message_hops_without_header() {
        let msg = fail_message(&fail(503, None, ""));
        assert!(is_retriable(&msg), "503 must be retriable: {msg}");
        let msg = fail_message(&fail(529, None, "overloaded"));
        assert!(is_retriable(&msg), "529 must be retriable: {msg}");
    }

    #[test]
    fn auth_and_transport_failures_stay_put() {
        // 401/403 short-circuit before this helper in chat_stream, but the
        // message itself must never read as retriable either.
        assert!(!is_retriable(&fail_message(&fail(401, None, ""))));
        assert!(!is_retriable(&fail_message(&fail(
            400,
            None,
            "bad request"
        ))));
        // Pure transport failure keeps the legacy message and does not hop.
        let msg = fail_message(&fail(0, None, "connection refused"));
        assert!(msg.contains("endpoints unreachable"), "{msg}");
        assert!(!is_retriable(&msg));
    }

    #[test]
    fn gcp_project_id_validation() {
        use super::is_valid_gcp_project;
        assert!(is_valid_gcp_project("rising-fact-p41fc"));
        assert!(is_valid_gcp_project("my-project-123"));
        assert!(is_valid_gcp_project("google-cloud-1"));
        // IDE session UUIDs must be rejected:
        assert!(!is_valid_gcp_project(
            "631b5b70-d7c9-4cb5-90fa-0cc4c3a29ca5"
        ));
        assert!(!is_valid_gcp_project(
            "783307f2-fdae-41da-a1f0-491d433f4c3c"
        ));
        // Too short / too long:
        assert!(!is_valid_gcp_project("abc"));
        assert!(!is_valid_gcp_project(
            "a-very-long-project-id-exceeding-thirty-chars"
        ));
        // Starts with digit or ends with hyphen:
        assert!(!is_valid_gcp_project("1project"));
        assert!(!is_valid_gcp_project("project-"));
    }
}
