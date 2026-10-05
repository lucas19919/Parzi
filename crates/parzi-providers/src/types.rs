use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    Auth,
    RateLimit,
    Overloaded,
    ContextOverflow,
    BadRequest,
    Process,
    SessionLost,
    Unknown,
}

impl ErrorClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::RateLimit => "rate_limit",
            Self::Overloaded => "overloaded",
            Self::ContextOverflow => "context_overflow",
            Self::BadRequest => "bad_request",
            Self::Process => "process",
            Self::SessionLost => "session_lost",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderError {
    pub class: ErrorClass,
    pub message: String,
}

impl ProviderError {
    pub fn new(class: ErrorClass, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
        }
    }

    pub fn process(message: impl Into<String>) -> Self {
        Self::new(ErrorClass::Process, message)
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProviderError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ready,
    SignedOut,
    NotInstalled,
    Disabled,
    Error,
    Unchecked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub label: String,
    pub used_percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
}

impl UsageWindow {
    #[must_use]
    pub fn spent(&self, now: u64) -> bool {
        self.used_percent >= 100.0 && self.resets_at.is_none_or(|t| t > now)
    }

    #[must_use]
    pub fn current(&self, now: u64) -> bool {
        self.resets_at.is_some_and(|t| t > now)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub efforts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub provider: String,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default)]
    pub hint: String,
    #[serde(default)]
    pub usage: Vec<UsageWindow>,
    #[serde(default)]
    pub models: Vec<ModelInfo>,
    #[serde(default)]
    pub gated: bool,
    pub checked_at: u64,
}

impl ProviderStatus {
    pub fn new(provider: &str, state: State, hint: impl Into<String>) -> Self {
        Self {
            provider: provider.to_string(),
            state,
            version: None,
            account: None,
            hint: hint.into(),
            usage: vec![],
            models: vec![],
            gated: false,
            checked_at: now_secs(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolServer {
    pub name: String,
    pub url: String,
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct TurnSpec {
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub instructions: Option<String>,
    pub resume: Option<serde_json::Value>,
    pub prompt: String,
    pub images: Vec<PathBuf>,
    pub tools: Option<ToolServer>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    Session {
        resume: serde_json::Value,
    },
    TextDelta(String),
    ReasoningDelta(String),
    Message(String),
    Reasoning(String),
    ToolStarted {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolFinished {
        id: String,
        name: String,
        ok: bool,
        output: String,
    },
    Usage {
        input: u64,
        output: u64,
        cost_usd: Option<f64>,
    },
    Context {
        used: u64,
        limit: u64,
    },
    Limits(Vec<UsageWindow>),
    Notice(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnEnd {
    Completed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequest {
    pub id: String,
    pub tool: String,
    pub title: String,
    pub input: serde_json::Value,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny(String),
}

#[async_trait::async_trait]
pub trait PermissionGate: Send + Sync {
    async fn decide(&self, request: PermissionRequest) -> PermissionDecision;
}

pub type EventTx = mpsc::UnboundedSender<ProviderEvent>;

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;

    fn gated(&self) -> bool;

    async fn status(&self) -> ProviderStatus;

    async fn run_turn(
        &self,
        spec: TurnSpec,
        gate: Arc<dyn PermissionGate>,
        events: EventTx,
        cancel: CancellationToken,
    ) -> Result<TurnEnd, ProviderError>;
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn tail(text: &str, max: usize) -> String {
    let t = text.trim();
    let n = t.chars().count();
    if n <= max {
        return t.to_string();
    }
    let skip = n - max;
    format!("…{}", t.chars().skip(skip).collect::<String>())
}

pub(crate) async fn sleep_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => std::future::pending().await,
    }
}
