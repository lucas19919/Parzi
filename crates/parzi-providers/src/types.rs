//! One shape for every provider. Parzi does not call model APIs: it drives
//! the vendor's own agent program and sees each turn as a stream of
//! [`ProviderEvent`]s, the way t3code does.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// What a failure was. Adapters decide it from the vendor's own error
/// fields (status codes, error types), never by searching message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    /// Not signed in, token rejected, account blocked.
    Auth,
    /// A plan window or rate limit is used up.
    RateLimit,
    /// The vendor is overloaded or down.
    Overloaded,
    /// The conversation no longer fits the model.
    ContextOverflow,
    /// The request itself was refused as malformed.
    BadRequest,
    /// The vendor program is missing, would not start, or died.
    Process,
    /// Anything the vendor did not classify.
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
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderError {
    pub class: ErrorClass,
    /// The vendor's own words, trimmed. Shown to the person as-is.
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

/// Where a provider stands right now, as its own program reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ready,
    SignedOut,
    NotInstalled,
    Disabled,
    Error,
    /// Installed, but the program offers no way to check sign-in without
    /// starting a session. The first turn tells.
    Unchecked,
}

/// One plan window ("Session", "Weekly") and how much of it is used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub label: String,
    pub used_percent: f64,
    /// Unix seconds when the window resets, when the vendor says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// What the vendor program takes as its model argument.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
    /// Effort levels this model accepts, in the vendor's own words. Empty =
    /// the vendor has no effort knob for it.
    #[serde(default)]
    pub efforts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub provider: String,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Signed-in plan or account ("Claude Max", "ChatGPT Plus").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// What to do next when not ready, or the probe's own message.
    #[serde(default)]
    pub hint: String,
    #[serde(default)]
    pub usage: Vec<UsageWindow>,
    #[serde(default)]
    pub models: Vec<ModelInfo>,
    /// Unix seconds of this probe.
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
            checked_at: now_secs(),
        }
    }
}

/// What the vendor may do without asking Parzi first. Every action outside
/// it arrives as a [`PermissionRequest`] and Parzi's own gate decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Read and answer only; every change is refused.
    ReadOnly,
    /// Every change asks.
    Ask,
    /// File edits inside the workspace run; commands ask.
    Edits,
    /// The vendor's own normal mode; Parzi still sees what it asks.
    Auto,
}

/// Parzi's own tools, served over MCP for this one turn.
#[derive(Debug, Clone)]
pub struct ToolServer {
    /// MCP server name the agent sees (`parzi`).
    pub name: String,
    /// Streamable-HTTP endpoint. The path carries the per-run secret.
    pub url: String,
    /// Same secret, for vendors that send it as a bearer header.
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct TurnSpec {
    /// Parzi's session id, for logs and for a fresh vendor session id.
    pub session_id: String,
    pub cwd: PathBuf,
    /// Vendor model argument. `None` = the vendor's default.
    pub model: Option<String>,
    pub effort: Option<String>,
    pub access: Access,
    /// Parzi's standing instructions (lane, workspace, role, knowledge).
    pub instructions: Option<String>,
    /// From a previous turn's [`ProviderEvent::Session`]. `None` = new session.
    pub resume: Option<serde_json::Value>,
    pub prompt: String,
    /// Local image files that ride with the prompt.
    pub images: Vec<PathBuf>,
    pub tools: Option<ToolServer>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    /// The vendor's handle for resuming this conversation on the next turn.
    Session { resume: serde_json::Value },
    TextDelta(String),
    ReasoningDelta(String),
    /// A finished assistant message: the text that goes in the transcript.
    Message(String),
    /// A finished reasoning block.
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
    /// Tokens this turn spent so far, as increments.
    Usage {
        input: u64,
        output: u64,
        cost_usd: Option<f64>,
    },
    /// How full the model's window is.
    Context { used: u64, limit: u64 },
    /// Plan windows, as the vendor just reported them.
    Limits(Vec<UsageWindow>),
    /// Something worth a line in the thread (a retry, a reroute).
    Notice(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnEnd {
    Completed,
    Interrupted,
}

/// An action the vendor wants to take and Parzi has to allow.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequest {
    pub id: String,
    /// The vendor's tool name ("Bash", "Edit", "commandExecution", …).
    pub tool: String,
    /// One line for the approval card.
    pub title: String,
    pub input: serde_json::Value,
    /// Files the action writes, when the vendor says. The lease gate reads it.
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    /// Allow, and let the vendor stop asking for this kind in this session.
    AllowAlways,
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

    /// Ask the vendor program where it stands. Never spends quota.
    async fn status(&self) -> ProviderStatus;

    /// Run one turn to its end. Events stream on `events`; the returned
    /// error is the turn's failure as the vendor classified it.
    async fn run_turn(
        &self,
        spec: TurnSpec,
        gate: Arc<dyn PermissionGate>,
        events: EventTx,
        cancel: CancellationToken,
    ) -> Result<TurnEnd, ProviderError>;
}

/// Seconds since the Unix epoch (0 on clock failure).
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Last `max` characters of a vendor's stderr, for a failure message.
pub(crate) fn tail(text: &str, max: usize) -> String {
    let t = text.trim();
    let n = t.chars().count();
    if n <= max {
        return t.to_string();
    }
    let skip = n - max;
    format!("…{}", t.chars().skip(skip).collect::<String>())
}
