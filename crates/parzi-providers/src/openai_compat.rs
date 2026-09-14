//! Shared OpenAI-compatible streaming engine.
//! Serves xai and opencode — only auth/URL/billing differ.

use futures::StreamExt;
use parzi_core::context::Role;
use parzi_core::error::{ParziError, Result};

use crate::types::{
    AuthStatus, Billing, ChatReq, EventRx, EventTx, Model, Provider, StreamEvent, ToolDef,
    desanitize_tool, sanitize_tool,
};

pub struct OpenAiCompat {
    pub id: &'static str,
    pub base_url: String,
    pub api_key: Option<String>,
    pub extra_headers: Vec<(String, String)>,
    pub static_models: Vec<Model>,
    pub missing_hint: String,
    /// Disk-cache refresh kill-switch (config `catalog_refresh`).
    pub refresh: bool,
    /// Billing class when a credential is present (opencode fronts a plan).
    pub billing: Billing,
    /// Account label for the UI when signed in.
    pub account: Option<String>,
}

impl OpenAiCompat {
    pub fn client(&self) -> Result<reqwest::Client> {
        let mut b = reqwest::Client::builder().timeout(std::time::Duration::from_secs(120));
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(k) = &self.api_key {
            let v: reqwest::header::HeaderValue = format!("Bearer {k}")
                .parse()
                .map_err(|_| ParziError::Provider(self.id.into(), "bad api key".into()))?;
            headers.insert(reqwest::header::AUTHORIZATION, v);
        }
        for (k, v) in &self.extra_headers {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(k.as_bytes()).map_err(|_| {
                    ParziError::Provider(self.id.into(), "bad header name".into())
                })?,
                v.parse()
                    .map_err(|_| ParziError::Provider(self.id.into(), "bad header value".into()))?,
            );
        }
        b = b.default_headers(headers);
        b.build()
            .map_err(|e| ParziError::Provider(self.id.into(), e.to_string()))
    }

    fn to_openai_messages(&self, req: &ChatReq) -> Vec<serde_json::Value> {
        let mut out = vec![];
        if !req.system.trim().is_empty() {
            out.push(serde_json::json!({"role": "system", "content": req.system}));
        }
        for m in &req.messages {
            let role = match m.role {
                Role::System => "system",
                Role::User | Role::Tool => "user",
                Role::Assistant => "assistant",
            };
            out.push(serde_json::json!({"role": role, "content": crate::images::openai_content(&m.content, &m.images)}));
        }
        out
    }

    fn to_openai_tools(&self, req: &ChatReq) -> Vec<serde_json::Value> {
        req.tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        // Dots 400 on OpenAI-compatible gates; mapped back on receipt.
                        "name": sanitize_tool(&t.name),
                        "description": t.description,
                        "parameters": t.schema,
                    }
                })
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiCompat {
    fn id(&self) -> &'static str {
        self.id
    }

    async fn models(&self) -> Result<Vec<Model>> {
        // Best-effort live list; disk cache; static catalog. Never fails.
        // Keyless local servers (`opencode serve`) serve /models
        // without auth, so loopback bases always attempt the live pull —
        // otherwise the picker would show a stale hardcoded handful.
        if self.refresh && (self.api_key.is_some() || is_loopback(&self.base_url)) {
            if let Ok(client) = self.client() {
                let url = format!("{}/models", self.base_url.trim_end_matches('/'));
                if let Ok(resp) = client.get(&url).send().await {
                    if let Ok(v) = resp.json::<serde_json::Value>().await {
                        if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
                            let ids: Vec<String> = arr
                                .iter()
                                .filter_map(|m| m.get("id")?.as_str().map(str::to_string))
                                .collect();
                            if !ids.is_empty() {
                                let live: Vec<Model> = ids
                                    .into_iter()
                                    .map(|id| {
                                        let known = self.static_models.iter().find(|m| m.id == id);
                                        Model {
                                            context_limit: known.map_or(128_000, |m| m.context_limit),
                                            output_limit: known.map_or(16_384, |m| m.output_limit),
                                            price_in: known.map_or(0.0, |m| m.price_in),
                                            price_out: known.map_or(0.0, |m| m.price_out),
                                            tools: known.map_or(true, |m| m.tools),
                                            vision: known.map_or(false, |m| m.vision),
                                            legacy: known.map_or(false, |m| m.legacy),
                                            is_default: known.map_or(false, |m| m.is_default),
                                            family: known.map_or_else(|| id.clone(), |m| m.family.clone()),
                                            family_name: known.map_or_else(|| id.clone(), |m| m.family_name.clone()),
                                            variant: known.and_then(|m| m.variant.clone()),
                                            name: known.map_or_else(|| id.clone(), |m| m.name.clone()),
                                            id,
                                        }
                                    })
                                    .collect();
                                crate::catalog::store_models(self.id, &live);
                                return Ok(live);
                            }
                        }
                    }
                }
            }
        }
        if let Some(cached) = crate::catalog::cached_models(self.id) {
            return Ok(cached);
        }
        Ok(self.static_models.clone())
    }

    async fn chat_stream(&self, req: ChatReq) -> Result<EventRx> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let client = self.client()?;
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": req.model,
            "messages": self.to_openai_messages(&req),
            "tools": self.to_openai_tools(&req),
            "max_tokens": req.max_tokens,
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        let id = self.id;
        let defs = req.tools.clone();
        tokio::spawn(async move {
            if let Err(e) = run_sse(client, url, body, defs, tx.clone(), id).await {
                let _ = tx.send(Err(e));
            }
        });
        Ok(rx)
    }

    fn auth_status(&self) -> AuthStatus {
        match &self.api_key {
            Some(_) => AuthStatus::Ok,
            None => AuthStatus::Missing(self.missing_hint.clone()),
        }
    }

    fn billing(&self) -> Billing {
        if self.api_key.is_some() {
            self.billing
        } else {
            Billing::None
        }
    }

    fn account_label(&self) -> Option<String> {
        if self.api_key.is_some() {
            self.account.clone()
        } else {
            None
        }
    }
}

/// True for localhost bases (`http://localhost:…`, `127.x`, `::1`).
fn is_loopback(base: &str) -> bool {
    let host = base
        .split("://")
        .nth(1)
        .unwrap_or(base)
        .split('/')
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("");
    let bare = host.strip_prefix('[').unwrap_or(host);
    bare.eq_ignore_ascii_case("localhost")
        || bare.starts_with("127.")
        || bare == "::1"
}

async fn run_sse(
    client: reqwest::Client,
    url: String,
    body: serde_json::Value,
    defs: Vec<ToolDef>,
    tx: EventTx,
    id: &'static str,
) -> Result<()> {
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| ParziError::Provider(id.into(), format!("request: {e}")))?;
    if !resp.status().is_success() {
        let code = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let short: String = text.chars().take(300).collect();
        return Err(ParziError::Provider(id.into(), format!("http {code}: {short}")));
    }
    // Accumulate tool-call argument fragments keyed by index.
    let mut calls: std::collections::HashMap<usize, (String, String, String)> =
        std::collections::HashMap::new();
    let mut buf = String::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ParziError::Provider(id.into(), format!("stream: {e}")))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
            let data = match line.strip_prefix("data:") {
                Some(d) => d.trim(),
                None => continue,
            };
            if data == "[DONE]" {
                continue;
            }
            let v: serde_json::Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(u) = v.get("usage") {
                let pin = u.get("prompt_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                let pout = u.get("completion_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                let _ = tx.send(Ok(StreamEvent::Usage { tokens_in: pin, tokens_out: pout }));
                continue;
            }
            let delta = match v
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|c| c.first())
                .and_then(|c| c.get("delta"))
            {
                Some(d) => d,
                None => continue,
            };
            if let Some(t) = delta.get("content").and_then(|c| c.as_str()) {
                if !t.is_empty() {
                    let _ = tx.send(Ok(StreamEvent::Text(t.to_string())));
                }
            }
            if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                for tc in tcs {
                    let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                    let e = calls.entry(idx).or_insert_with(|| {
                        (format!("call_{idx}"), String::new(), String::new())
                    });
                    if let Some(call_id) = tc.get("id").and_then(|i| i.as_str()) {
                        e.0 = call_id.to_string();
                    }
                    if let Some(f) = tc.get("function") {
                        if let Some(n) = f.get("name").and_then(|n| n.as_str()) {
                            e.1 = n.to_string();
                        }
                        if let Some(a) = f.get("arguments").and_then(|a| a.as_str()) {
                            e.2.push_str(a);
                        }
                    }
                }
            }
        }
    }
    let mut ordered: Vec<_> = calls.into_iter().collect();
    ordered.sort_by_key(|(i, _)| *i);
    for (_, (call_id, name, args_str)) in ordered {
        if name.is_empty() {
            continue;
        }
        let args = serde_json::from_str(&args_str).unwrap_or(serde_json::Value::Null);
        let name = desanitize_tool(&defs, &name);
        let _ = tx.send(Ok(StreamEvent::ToolCall { id: call_id, name, args }));
    }
    Ok(())
}
