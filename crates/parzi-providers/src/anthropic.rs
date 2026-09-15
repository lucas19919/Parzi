//! Native Anthropic Messages API engine (used by the `claude` adapter).
//! SSE event parsing, tool input_json accumulation, thinking rail, usage.

use futures::StreamExt;
use parzi_core::context::Role;
use parzi_core::error::{ParziError, Result};

use crate::types::{
    desanitize_tool, sanitize_tool, AuthStatus, Billing, ChatReq, EventRx, Model, Provider,
    StreamEvent, ToolDef,
};

pub struct AnthropicNative {
    pub id: &'static str,
    pub api_key: Option<String>,
    pub oauth_token: Option<String>,
    /// The OAuth token on disk is past its `expiresAt` (still sent: the
    /// server is the judge, but the UI shows "expired" instead of "ok").
    pub oauth_expired: bool,
    /// Plan label when signed in ("Claude Max").
    pub account: Option<String>,
    pub beta: Vec<String>,
    pub missing_hint: String,
    pub static_models: Vec<Model>,
}

/// Models that still take a fixed thinking budget. Everything current
/// (Opus 5, Sonnet 5, Fable 5.x, Opus 4.7+) rejects `budget_tokens` with a
/// 400 and takes adaptive thinking + `output_config.effort` instead.
fn takes_budget(model: &str) -> bool {
    model.contains("haiku-4-5")
        || model.contains("-4-5")
        || model.contains("-4-1")
        || model.contains("-4-0")
        || model.contains("-3-")
}

impl AnthropicNative {
    const URL: &'static str = "https://api.anthropic.com/v1/messages";
    const VERSION: &'static str = "2023-06-01";

    fn client(&self) -> Result<reqwest::Client> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "anthropic-version",
            Self::VERSION
                .parse()
                .map_err(|_| ParziError::Provider(self.id.into(), "bad version header".into()))?,
        );
        if let Some(t) = &self.oauth_token {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {t}")
                    .parse()
                    .map_err(|_| ParziError::Provider(self.id.into(), "bad token".into()))?,
            );
            if !self.beta.is_empty() {
                headers.insert(
                    "anthropic-beta",
                    self.beta.join(",").parse().map_err(|_| {
                        ParziError::Provider(self.id.into(), "bad beta header".into())
                    })?,
                );
            }
        } else if let Some(k) = &self.api_key {
            headers.insert(
                "x-api-key",
                k.parse()
                    .map_err(|_| ParziError::Provider(self.id.into(), "bad api key".into()))?,
            );
        }
        reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .map_err(|e| ParziError::Provider(self.id.into(), e.to_string()))
    }

    fn body(&self, req: &ChatReq) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::Assistant => "assistant",
                    _ => "user",
                };
                serde_json::json!({"role": role, "content": crate::images::anthropic_content(&m.content, &m.images)})
            })
            .collect();
        let tools: Vec<serde_json::Value> = req
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    // Dots 400 (`^[a-zA-Z0-9_-]{1,64}$`); mapped back on receipt.
                    "name": sanitize_tool(&t.name),
                    "description": t.description,
                    "input_schema": t.schema,
                })
            })
            .collect();
        let mut body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": messages,
            "tools": tools,
            "stream": true,
        });
        if takes_budget(&req.model) {
            // Legacy knob, clamped under max_tokens (min 1024).
            let want = crate::router::thinking_budget(&req.effort);
            let budget = want.min(req.max_tokens.saturating_sub(1024).max(1024));
            body["thinking"] = serde_json::json!({"type": "enabled", "budget_tokens": budget});
        } else {
            // Current models: adaptive thinking; the effort pill is the
            // native `output_config.effort` level (documented ladder
            // low/medium/high/xhigh/max — extra rides xhigh, ultra rides max).
            let effort = match req.effort.as_str() {
                "low" => "low",
                "medium" | "med" => "medium",
                "high" => "high",
                "extra" => "xhigh",
                "ultra" => "max",
                _ => "medium",
            };
            body["thinking"] = serde_json::json!({"type": "adaptive", "display": "summarized"});
            body["output_config"] = serde_json::json!({"effort": effort});
        }
        body
    }
}

#[async_trait::async_trait]
impl Provider for AnthropicNative {
    fn id(&self) -> &'static str {
        self.id
    }

    async fn models(&self) -> Result<Vec<Model>> {
        Ok(self.static_models.clone())
    }

    async fn chat_stream(&self, req: ChatReq) -> Result<EventRx> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let client = self.client()?;
        let tools = req.tools.clone();
        let body = self.body(&req);
        let id = self.id;
        tokio::spawn(async move {
            if let Err(e) = run(client, body, tools, tx.clone(), id).await {
                let _ = tx.send(Err(e));
            }
        });
        Ok(rx)
    }

    fn auth_status(&self) -> AuthStatus {
        if self.oauth_token.is_some() {
            if self.oauth_expired {
                AuthStatus::Expired(
                    "Claude Code sign-in expired — run `claude` once to refresh it".into(),
                )
            } else {
                AuthStatus::Ok
            }
        } else if self.api_key.is_some() {
            AuthStatus::Ok
        } else {
            AuthStatus::Missing(self.missing_hint.clone())
        }
    }

    fn billing(&self) -> Billing {
        if self.oauth_token.is_some() {
            Billing::Subscription
        } else if self.api_key.is_some() {
            Billing::ApiKey
        } else {
            Billing::None
        }
    }

    fn account_label(&self) -> Option<String> {
        if self.oauth_token.is_some() {
            self.account.clone()
        } else {
            None
        }
    }
}

async fn run(
    client: reqwest::Client,
    body: serde_json::Value,
    tools: Vec<ToolDef>,
    tx: crate::types::EventTx,
    id: &'static str,
) -> Result<()> {
    let resp = client
        .post(AnthropicNative::URL)
        .json(&body)
        .send()
        .await
        .map_err(|e| ParziError::Provider(id.into(), format!("request: {e}")))?;
    if !resp.status().is_success() {
        let code = resp.status();
        let retry = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(|r| format!(" (retry after {r})"))
            .unwrap_or_default();
        let text = resp.text().await.unwrap_or_default();
        let short: String = text.chars().take(300).collect();
        return Err(ParziError::Provider(
            id.into(),
            format!("http {code}{retry}: {short}"),
        ));
    }
    let mut tool_json = String::new();
    let mut tool_name = String::new();
    let mut tool_id = String::new();
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
            let v: serde_json::Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match typ {
                "content_block_delta" => {
                    let d = v.get("delta").cloned().unwrap_or_default();
                    match d.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                        "text_delta" => {
                            if let Some(t) = d.get("text").and_then(|t| t.as_str()) {
                                let _ = tx.send(Ok(StreamEvent::Text(t.to_string())));
                            }
                        }
                        "thinking_delta" => {
                            if let Some(t) = d.get("thinking").and_then(|t| t.as_str()) {
                                if !t.is_empty() {
                                    let _ = tx.send(Ok(StreamEvent::Reasoning(t.to_string())));
                                }
                            }
                        }
                        "input_json_delta" => {
                            if let Some(p) = d.get("partial_json").and_then(|p| p.as_str()) {
                                tool_json.push_str(p);
                            }
                        }
                        _ => {}
                    }
                }
                "content_block_start" => {
                    if let Some(b) = v.get("content_block") {
                        if b.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            tool_id = b
                                .get("id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("call_0")
                                .to_string();
                            tool_name = b
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string();
                            tool_json.clear();
                        }
                    }
                }
                "message_delta" => {
                    if let Some(u) = v.get("usage") {
                        let out = u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                        let _ = tx.send(Ok(StreamEvent::Usage {
                            tokens_in: 0,
                            tokens_out: out,
                        }));
                    }
                }
                "message_start" => {
                    if let Some(u) = v
                        .get("message")
                        .and_then(|m| m.get("usage"))
                        .and_then(|u| u.as_object())
                    {
                        let inp = u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                        let _ = tx.send(Ok(StreamEvent::Usage {
                            tokens_in: inp,
                            tokens_out: 0,
                        }));
                    }
                }
                _ => {}
            }
        }
    }
    if !tool_name.is_empty() {
        let args = serde_json::from_str(&tool_json).unwrap_or(serde_json::Value::Null);
        let name = desanitize_tool(&tools, &tool_name);
        let _ = tx.send(Ok(StreamEvent::ToolCall {
            id: tool_id,
            name,
            args,
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::takes_budget;

    #[test]
    fn current_models_take_effort_not_budget() {
        for m in [
            "claude-opus-5",
            "claude-sonnet-5",
            "claude-fable-5-1",
            "claude-opus-4-8",
            "claude-sonnet-4-6",
        ] {
            assert!(!takes_budget(m), "{m}");
        }
        for m in [
            "claude-haiku-4-5",
            "claude-sonnet-4-5",
            "claude-opus-4-1",
            "claude-3-7-sonnet",
        ] {
            assert!(takes_budget(m), "{m}");
        }
    }
}
