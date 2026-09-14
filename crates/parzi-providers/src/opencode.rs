//! `opencode` adapter: OpenCode Zen (free) + Go ($10/mo) behind one key.
//!
//! Verified 2026-09-14 against the hosted endpoints and the local serve
//! `/config/providers` (which is only used as a metadata source — local
//! `opencode serve` exposes NO OpenAI-compatible surface, just its own API
//! and web UI, so the old `localhost:4096/v1` base could never work):
//!
//! - chat/completions (Bearer): all `openai-compatible` models, both bases.
//! - messages (x-api-key + anthropic-version): minimax-*, qwen3.8-flash.
//! - responses (Bearer): muse-spark-*, gpt-5.6-luna, grok-4.6 (+free sparks).
//!
//! Go requires a stable `x-opencode-session` per conversation (else HTTP 400
//! `MissingSessionID`) and an identifying User-Agent. The session id is the
//! Parzi session id (`ChatReq.session`); prompt caching rides on it.
//!
//! Key resolution (first hit wins, read-only, never logged):
//! keyring `opencode` → `OPENCODE_API_KEY` → auth.json `opencode-go.key`
//! (exact) → legacy recursive scrape. From the TUI: `/connect` → OpenCode Go.

use parzi_core::context::Role;
use parzi_core::error::{ParziError, Result};

use crate::types::{
    AuthStatus, Billing, ChatReq, EventRx, Model, Provider, env_key, keyring_get,
    now_secs, read_json_file, sanitize_tool,
};

pub const GO_BASE: &str = "https://opencode.ai/zen/go/v1";
pub const ZEN_BASE: &str = "https://opencode.ai/zen/v1";
/// Retired placeholder id: old configs/threads still carry it.
pub const LEGACY_PLACEHOLDER: &str = "opencode-default";
pub const DEFAULT_MODEL: &str = "kimi-k3";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZenKind {
    Chat,
    Messages,
    Responses,
}

/// (base, wire kind) for a Zen model id. Free-tier ids ride the Zen base,
/// everything else the Go base. Unknown ids default to Go+chat — the live
/// `/models` merge usually knows them before a chat is attempted.
pub fn route_for(model: &str) -> (&'static str, ZenKind) {
    match model {
        "minimax-m3" | "minimax-m2.7" | "qwen3.8-flash" => (GO_BASE, ZenKind::Messages),
        "muse-spark-1.3-contributor"
        | "muse-spark-1.2-contributor"
        | "gpt-5.6-luna"
        | "grok-4.6" => (GO_BASE, ZenKind::Responses),
        "muse-spark-1.3-contributor-free" | "muse-spark-1.2-contributor-free" => {
            (ZEN_BASE, ZenKind::Responses)
        }
        "big-pickle" | "nemotron-3-ultra-free" | "nemotron-3.5-lightning-free" | "mimo-v2.5-free"
        | "ling-3.0-flash-fin-free" => (ZEN_BASE, ZenKind::Chat),
        _ if model.ends_with("-free") => (ZEN_BASE, ZenKind::Chat),
        _ => (GO_BASE, ZenKind::Chat),
    }
}

/// Old threads/configs say `opencode-default`; they mean the flagship.
pub fn resolve_model_id(model: &str) -> &str {
    if model == LEGACY_PLACEHOLDER {
        DEFAULT_MODEL
    } else {
        model
    }
}

fn auth_file() -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    [".local/share/opencode/auth.json", ".config/opencode/auth.json"]
        .iter()
        .map(|rel| home.join(rel))
        .find(|p| p.is_file())
}

/// Exact `opencode-go.key` read only (H-10: no recursive scrape — another
/// provider's key must never be sent as a Bearer to opencode.ai).
pub fn resolve_key() -> Option<String> {
    if let Some(k) = keyring_get("opencode") {
        return Some(k);
    }
    if let Some(k) = env_key("OPENCODE_API_KEY") {
        return Some(k);
    }
    let v = read_json_file(&auth_file()?)?;
    v.get("opencode-go")
        .and_then(|g| g.get("key"))
        .and_then(|k| k.as_str())
        .filter(|s| s.trim().len() > 10)
        .map(|s| s.to_string())
}

pub struct Opencode {
    pub key: Option<String>,
    pub refresh: bool,
    /// Go-base override (self-hosted proxy case); free tier stays hosted.
    pub go_base: Option<String>,
}

pub fn opencode(go_base: Option<String>, refresh: bool) -> Opencode {
    Opencode { key: resolve_key(), refresh, go_base }
}

impl Opencode {
    pub(crate) fn go(&self) -> &str {
        self.go_base.as_deref().unwrap_or(GO_BASE)
    }

    pub(crate) fn headers(&self, session: &str, api_key_style: bool) -> Result<reqwest::header::HeaderMap> {
        let key = self.key.clone().unwrap_or_default();
        let mut h = reqwest::header::HeaderMap::new();
        let auth_val = if api_key_style { key } else { format!("Bearer {key}") };
        if api_key_style {
            h.insert(
                "x-api-key",
                auth_val.parse().map_err(|_| {
                    ParziError::Provider("opencode".into(), "bad credential".into())
                })?,
            );
        } else {
            h.insert(
                reqwest::header::AUTHORIZATION,
                auth_val.parse().map_err(|_| {
                    ParziError::Provider("opencode".into(), "bad credential".into())
                })?,
            );
        }
        h.insert(
            "x-opencode-session",
            session.parse().map_err(|_| {
                ParziError::Provider("opencode".into(), "bad session id".into())
            })?,
        );
        if api_key_style {
            h.insert("anthropic-version", ANTHROPIC_VERSION.parse().map_err(|_| {
                ParziError::Provider("opencode".into(), "bad version header".into())
            })?);
        }
        h.insert(
            reqwest::header::USER_AGENT,
            concat!("parzi/", env!("CARGO_PKG_VERSION")).parse().map_err(|_| {
                ParziError::Provider("opencode".into(), "bad user agent".into())
            })?,
        );
        Ok(h)
    }

    pub(crate) fn client(&self, session: &str, api_key_style: bool) -> Result<reqwest::Client> {
        reqwest::Client::builder()
            .default_headers(self.headers(session, api_key_style)?)
            // Zen's edge intermittently resets negotiated HTTP/2 streams
            // from this client (transport errors on an otherwise healthy
            // route); HTTP/1.1 + SSE is the validated shape.
            .http1_only()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .map_err(|e| ParziError::Provider("opencode".into(), e.to_string()))
    }

    pub(crate) fn openai_messages(&self, req: &ChatReq) -> Vec<serde_json::Value> {
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
            out.push(serde_json::json!({"role": role, "content": m.content}));
        }
        out
    }

    pub(crate) fn openai_tools(&self, req: &ChatReq) -> Vec<serde_json::Value> {
        req.tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": sanitize_tool(&t.name),
                        "description": t.description,
                        "parameters": t.schema,
                    }
                })
            })
            .collect()
    }

    pub(crate) fn output_cap(&self, model: &str, want: u32) -> u32 {
        let known = crate::catalog::for_provider("opencode")
            .into_iter()
            .find(|m| m.id == model)
            .map(|m| m.output_limit)
            .unwrap_or(16_384);
        want.min(known)
    }
}

#[async_trait::async_trait]
impl Provider for Opencode {
    fn id(&self) -> &'static str {
        "opencode"
    }

    async fn models(&self) -> Result<Vec<Model>> {
        let mut merged = crate::catalog::opencode();
        // Best-effort live Go ids; static metadata always wins, unknowns get
        // safe chat defaults. Never fails (offline keeps the bundled table).
        if self.refresh && self.key.is_some() {
            if let Ok(client) = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
            {
                let url = format!("{}/models", self.go());
                let key = self.key.clone().unwrap_or_default();
                if let Ok(resp) = client
                    .get(&url)
                    .bearer_auth(&key)
                    .header(reqwest::header::USER_AGENT, concat!("parzi/", env!("CARGO_PKG_VERSION")))
                    .send()
                    .await
                {
                    if let Ok(v) = resp.json::<serde_json::Value>().await {
                        if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
                            for id in arr.iter().filter_map(|m| m.get("id")?.as_str()) {
                                if !merged.iter().any(|m| m.id == id) {
                                    merged.push(Model {
                                        id: id.into(),
                                        name: id.into(),
                                        context_limit: 200_000,
                                        output_limit: 32_768,
                                        price_in: 0.0,
                                        price_out: 0.0,
                                        tools: true,
                                        vision: false,
                                        legacy: false,
                                        is_default: false,
                                        family: id.into(),
                                        family_name: id.into(),
                                        variant: None,
                                    });
                                }
                            }
                            crate::catalog::store_models("opencode", &merged);
                        }
                    }
                }
            }
        }
        Ok(merged)
    }

    async fn chat_stream(&self, req: ChatReq) -> Result<EventRx> {
        if self.key.is_none() {
            return Err(ParziError::Provider(
                "opencode".into(),
                "missing OpenCode credentials".into(),
            ));
        }
        let model = resolve_model_id(&req.model).to_string();
        let (base_cfg, kind) = route_for(&model);
        // A configured proxy overrides the Go base only; free tier is hosted.
        let base = if base_cfg == GO_BASE { self.go().to_string() } else { base_cfg.to_string() };
        let session = if req.session.trim().is_empty() {
            format!("parzi-{}-{}", std::process::id(), now_secs())
        } else {
            req.session.clone()
        };
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let me = Opencode {
            key: self.key.clone(),
            refresh: self.refresh,
            go_base: self.go_base.clone(),
        };
        tokio::spawn(async move {
            let r = match kind {
                ZenKind::Chat => me.run_chat(model, base, session, req, tx.clone()).await,
                ZenKind::Messages => me.run_messages(model, base, session, req, tx.clone()).await,
                ZenKind::Responses => me.run_responses(model, base, session, req, tx.clone()).await,
            };
            if let Err(e) = r {
                let _ = tx.send(Err(e));
            }
        });
        Ok(rx)
    }

    fn auth_status(&self) -> AuthStatus {
        match &self.key {
            Some(_) => AuthStatus::Ok,
            None => AuthStatus::Missing(
                "in the opencode TUI run `/connect` → OpenCode Go, or set OPENCODE_API_KEY".into(),
            ),
        }
    }

    fn billing(&self) -> Billing {
        if self.key.is_some() {
            Billing::Subscription
        } else {
            Billing::None
        }
    }

    fn account_label(&self) -> Option<String> {
        if self.key.is_some() {
            Some("OpenCode Go".into())
        } else {
            None
        }
    }
}
