//! `codex` adapter: OpenAI Responses API behind whichever credential wins.
//!
//! Resolution order (first hit decides the billing class):
//! 1. keyring `codex` — a ChatGPT access token pasted by hand      → subscription
//! 2. `~/.codex/auth.json` `tokens.access_token` (+ `account_id`)
//!    written by `codex login`; read-only, never written            → subscription
//! 3. keyring `openai` / `OPENAI_API_KEY`                          → api key
//!
//! A subscription token is not valid on `api.openai.com`; it talks to the
//! ChatGPT Codex backend instead (same Responses wire shape, plus the
//! `chatgpt-account-id` header and `store: false`).

use futures::StreamExt;
use parzi_core::context::Role;
use parzi_core::error::{ParziError, Result};

use crate::types::{
    AuthStatus, Billing, ChatReq, EventRx, Model, Provider, StreamEvent, env_key, keyring_get,
    read_json_file,
};

const API_BASE: &str = "https://api.openai.com/v1";
const CHATGPT_BASE: &str = "https://chatgpt.com/backend-api/codex";

pub struct Codex {
    pub token: Option<String>,
    pub billing: Billing,
    /// ChatGPT account id (subscription path only).
    pub account_id: Option<String>,
    pub base_url: String,
}

struct CliSignIn {
    access: String,
    account_id: Option<String>,
}

fn cli_sign_in() -> Option<CliSignIn> {
    let v = read_json_file(&dirs::home_dir()?.join(".codex/auth.json"))?;
    let toks = v.get("tokens")?;
    let access = toks
        .get("access_token")
        .and_then(|t| t.as_str())
        .filter(|t| !t.trim().is_empty())?
        .to_string();
    Some(CliSignIn {
        access,
        account_id: toks
            .get("account_id")
            .and_then(|a| a.as_str())
            .map(|a| a.to_string()),
    })
}

impl Codex {
    pub fn new(base_url: Option<String>) -> Self {
        if let Some(t) = keyring_get("codex") {
            return Self {
                token: Some(t),
                billing: Billing::Subscription,
                account_id: None,
                base_url: base_url.unwrap_or_else(|| CHATGPT_BASE.into()),
            };
        }
        if let Some(s) = cli_sign_in() {
            return Self {
                token: Some(s.access),
                billing: Billing::Subscription,
                account_id: s.account_id,
                base_url: base_url.unwrap_or_else(|| CHATGPT_BASE.into()),
            };
        }
        let key = keyring_get("openai").or_else(|| env_key("OPENAI_API_KEY"));
        let billing = if key.is_some() {
            Billing::ApiKey
        } else {
            Billing::None
        };
        Self {
            token: key,
            billing,
            account_id: None,
            base_url: base_url.unwrap_or_else(|| API_BASE.into()),
        }
    }
}

#[async_trait::async_trait]
impl Provider for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    async fn models(&self) -> Result<Vec<Model>> {
        Ok(crate::catalog::codex())
    }

    fn billing(&self) -> Billing {
        self.billing
    }

    fn account_label(&self) -> Option<String> {
        match self.billing {
            Billing::Subscription => Some("ChatGPT / Codex".into()),
            _ => None,
        }
    }

    async fn chat_stream(&self, req: ChatReq) -> Result<EventRx> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let key = self.token.clone().ok_or_else(|| {
            ParziError::Provider("codex".into(), "missing Codex credentials".into())
        })?;
        let url = format!("{}/responses", self.base_url.trim_end_matches('/'));
        let input: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::Assistant => "assistant",
                    _ => "user",
                };
                serde_json::json!({"role": role, "content": m.content})
            })
            .collect();
        let tools: Vec<serde_json::Value> = req
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function", "name": t.name,
                    "description": t.description, "parameters": t.schema,
                })
            })
            .collect();
        // Native reasoning effort (Responses API). Extra/ultra ride xhigh.
        let reasoning_effort = match req.effort.as_str() {
            "low" => "low",
            "medium" | "med" => "medium",
            "high" => "high",
            "extra" | "ultra" => "xhigh",
            _ => "medium",
        };
        let mut body = serde_json::json!({
            "model": req.model, "input": input, "tools": tools,
            "stream": true,
            "reasoning": {"effort": reasoning_effort, "summary": "auto"},
            "instructions": req.system,
        });
        let subscription = self.billing == Billing::Subscription;
        if subscription {
            // ChatGPT backend: stateless only; output cap is the plan's.
            body["store"] = serde_json::Value::Bool(false);
        } else {
            body["max_output_tokens"] = serde_json::json!(req.max_tokens);
        }
        let account_id = self.account_id.clone();
        tokio::spawn(async move {
            let mut headers = reqwest::header::HeaderMap::new();
            if subscription {
                if let Some(acct) = account_id.as_deref().and_then(|a| a.parse().ok()) {
                    headers.insert("chatgpt-account-id", acct);
                }
                if let Ok(v) = "responses=experimental".parse() {
                    headers.insert("OpenAI-Beta", v);
                }
                if let Ok(v) = "codex_cli_rs".parse() {
                    headers.insert("originator", v);
                }
            }
            let client = match reqwest::Client::builder()
                .default_headers(headers)
                .timeout(std::time::Duration::from_secs(180))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(Err(ParziError::Provider("codex".into(), e.to_string())));
                    return;
                }
            };
            let resp = match client
                .post(&url)
                .bearer_auth(&key)
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(ParziError::Provider("codex".into(), format!("request: {e}"))));
                    return;
                }
            };
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
                let _ = tx.send(Err(ParziError::Provider(
                    "codex".into(),
                    format!("http {code}{retry}: {short}"),
                )));
                return;
            }
            let mut arg_buf = String::new();
            let mut fn_name = String::new();
            let mut call_id = String::new();
            let mut buf = String::new();
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(Err(ParziError::Provider("codex".into(), format!("stream: {e}"))));
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
                    if data == "[DONE]" {
                        continue;
                    }
                    let v: serde_json::Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match typ {
                        "response.output_text.delta" => {
                            if let Some(t) = v.get("delta").and_then(|d| d.as_str()) {
                                let _ = tx.send(Ok(StreamEvent::Text(t.to_string())));
                            }
                        }
                        "response.reasoning_summary_text.delta" => {
                            if let Some(t) = v.get("delta").and_then(|d| d.as_str()) {
                                let _ = tx.send(Ok(StreamEvent::Reasoning(t.to_string())));
                            }
                        }
                        "response.function_call_arguments.delta" => {
                            if let Some(d) = v.get("delta").and_then(|d| d.as_str()) {
                                arg_buf.push_str(d);
                            }
                        }
                        "response.output_item.done" => {
                            if let Some(item) = v.get("item") {
                                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                                    fn_name = item.get("name").and_then(|n| n.as_str()).unwrap_or("").into();
                                    call_id = item.get("call_id").and_then(|n| n.as_str()).unwrap_or("call_0").into();
                                    arg_buf = item.get("arguments").and_then(|a| a.as_str()).unwrap_or("").into();
                                }
                            }
                        }
                        "response.completed" => {
                            if let Some(u) = v.get("response").and_then(|r| r.get("usage")) {
                                let pin = u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                                let pout = u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                                let _ = tx.send(Ok(StreamEvent::Usage { tokens_in: pin, tokens_out: pout }));
                            }
                        }
                        _ => {}
                    }
                }
            }
            if !fn_name.is_empty() {
                let args = serde_json::from_str(&arg_buf).unwrap_or(serde_json::Value::Null);
                let _ = tx.send(Ok(StreamEvent::ToolCall { id: call_id, name: fn_name, args }));
            }
        });
        Ok(rx)
    }

    fn auth_status(&self) -> AuthStatus {
        match &self.token {
            Some(_) => AuthStatus::Ok,
            None => AuthStatus::Missing(
                "sign in with the Codex CLI (`codex login`) or add an OpenAI API key".into(),
            ),
        }
    }
}
