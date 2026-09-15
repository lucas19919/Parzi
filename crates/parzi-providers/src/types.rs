//! Shared provider types. One shape for every adapter.

use parzi_core::context::ChatMessage;
use tokio::sync::mpsc;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Model {
    pub id: String,
    /// Friendly display name ("Claude Opus 5"). Falls back to id.
    pub name: String,
    pub context_limit: u32,
    pub output_limit: u32,
    /// USD per 1M tokens at API-key rates. Subscription runs bill 0 (the
    /// orchestrator zeroes prices when the live credential is a subscription).
    pub price_in: f64,
    pub price_out: f64,
    /// Capability overlay (t3code ModelManifest pattern, simplified).
    #[serde(default = "true_bool")]
    pub tools: bool,
    #[serde(default)]
    pub vision: bool,
    #[serde(default)]
    pub legacy: bool,
    #[serde(default)]
    pub is_default: bool,
    /// Family grouping for the picker (effort variants collapse into one row).
    /// Defaults to the model id (singleton family).
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub family_name: String,
    /// Effort variant within the family: low | medium | high. None = single.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

fn true_bool() -> bool {
    true
}

impl Model {
    pub fn vision(mut self) -> Self {
        self.vision = true;
        self
    }
    pub fn no_tools(mut self) -> Self {
        self.tools = false;
        self
    }
    pub fn legacy(mut self) -> Self {
        self.legacy = true;
        self
    }
    pub fn default(mut self) -> Self {
        self.is_default = true;
        self
    }
    pub fn family(mut self, family: &str, family_name: &str) -> Self {
        self.family = family.into();
        self.family_name = family_name.into();
        self
    }
    pub fn variant(mut self, variant: &str) -> Self {
        self.variant = Some(variant.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ChatReq {
    pub model: String,
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: u32,
    /// low | med | high. Each adapter maps it to its native knob
    /// (effort, reasoning level, variant) or to the output budget.
    pub effort: String,
    /// Stable Parzi session id. Backends with per-conversation routing or
    /// prompt caching (opencode Zen) key off this; empty falls back to a
    /// per-request id (works, caches poorly).
    pub session: String,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Text(String),
    /// Model reasoning (thought parts). Rendered in a collapsible rail,
    /// never sent back as context.
    Reasoning(String),
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    Usage {
        tokens_in: u64,
        tokens_out: u64,
    },
}

#[derive(Debug, Clone)]
pub enum AuthStatus {
    Ok,
    Missing(String),
    Expired(String),
}

/// How the *resolved* credential bills. Decided per adapter at build time
/// from whichever credential actually won, so the router never has to
/// guess from the provider id: a Claude adapter holding an OAuth token is a
/// subscription, the same adapter holding `ANTHROPIC_API_KEY` is a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Billing {
    /// Flat-rate plan (Claude Max/Pro, ChatGPT/Codex, Antigravity, opencode).
    Subscription,
    /// Pay-per-token key. Explicit pick only unless `routing.keys_in_auto`.
    ApiKey,
    /// Nothing resolved.
    None,
}

impl Billing {
    pub fn as_str(self) -> &'static str {
        match self {
            Billing::Subscription => "subscription",
            Billing::ApiKey => "api_key",
            Billing::None => "none",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderHealth {
    pub provider: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown_until: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Human label of the signed-in plan/account when known ("Claude Max").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_account: Option<String>,
    /// `Billing::as_str()` of the resolved credential.
    pub tier: String,
}

pub type EventTx = mpsc::UnboundedSender<parzi_core::error::Result<StreamEvent>>;
pub type EventRx = mpsc::UnboundedReceiver<parzi_core::error::Result<StreamEvent>>;

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn models(&self) -> parzi_core::error::Result<Vec<Model>>;
    /// Streams events on a channel. Closes (drops tx) when done.
    async fn chat_stream(&self, req: ChatReq) -> parzi_core::error::Result<EventRx>;
    fn auth_status(&self) -> AuthStatus;
    /// Billing class of the credential this adapter resolved. Adapters that
    /// can hold either kind override this; the default is the conservative
    /// answer (a key), so an unknown adapter never sneaks into auto routing.
    fn billing(&self) -> Billing {
        match self.auth_status() {
            AuthStatus::Ok | AuthStatus::Expired(_) => Billing::ApiKey,
            AuthStatus::Missing(_) => Billing::None,
        }
    }
    /// Plan/account label for the UI ("Claude Max", "ChatGPT"). None = unknown.
    fn account_label(&self) -> Option<String> {
        None
    }
    fn health(&self) -> ProviderHealth {
        let (status, err) = match self.auth_status() {
            AuthStatus::Ok => ("ok".to_string(), None),
            AuthStatus::Missing(m) => ("missing".to_string(), Some(m)),
            AuthStatus::Expired(e) => ("expired".to_string(), Some(e)),
        };
        ProviderHealth {
            provider: self.id().to_string(),
            status,
            cooldown_until: None,
            last_error: err,
            active_account: self.account_label(),
            tier: self.billing().as_str().to_string(),
        }
    }
}

/// Read a credential file. Returns None on any failure — never logs contents.
pub fn read_json_file(path: &std::path::Path) -> Option<serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

/// Best-effort token scrape from a CLI credential file (read-only).
/// Shape-drift proof: recursive search for token-like keys. Never logs values.
pub fn find_token(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                if let serde_json::Value::String(s) = val {
                    let kl = k.to_lowercase();
                    if (kl.contains("token")
                        || kl.contains("api_key")
                        || kl == "key"
                        || kl == "apikey")
                        && s.trim().len() > 10
                    {
                        return Some(s.clone());
                    }
                }
            }
            map.values().find_map(find_token)
        }
        serde_json::Value::Array(a) => a.iter().find_map(find_token),
        _ => None,
    }
}

/// OS keyring lookup. Any failure (headless, locked) degrades to None.
pub fn keyring_get(provider: &str) -> Option<String> {
    keyring::Entry::new("parzi", provider)
        .ok()
        .and_then(|e| e.get_password().ok())
        .filter(|s| !s.trim().is_empty())
}

pub fn env_key(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

/// Seconds since the Unix epoch (0 on clock failure).
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Tool-name sanitizer. Anthropic, OpenAI (+ every OpenAI-compatible
/// gateway) and Zen all require `^[a-zA-Z0-9_-]{1,64}$`; Parzi tools are
/// dotted (`fs.read`), which 400s everywhere except Gemini. Sanitize on
/// send, map back on receipt with [`desanitize_tool`].
pub fn sanitize_tool(name: &str) -> String {
    name.replace('.', "_")
}

/// Reverse of [`sanitize_tool`]: exact match first (an MCP tool may natively
/// carry underscores), else the tool whose sanitized form equals the returned
/// name, else passthrough.
pub fn desanitize_tool(defs: &[ToolDef], name: &str) -> String {
    if defs.iter().any(|d| d.name == name) {
        return name.to_string();
    }
    defs.iter()
        .find(|d| sanitize_tool(&d.name) == name)
        .map(|d| d.name.clone())
        .unwrap_or_else(|| name.to_string())
}
