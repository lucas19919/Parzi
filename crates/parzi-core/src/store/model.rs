//! The on-disk vocabulary: session metadata and the append-only event log.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    #[default]
    Idle,
    Done,
    Killed,
    /// Waiting for a run slot (queue mode). Pumped automatically.
    Queued,
}

impl SessionStatus {
    /// A run is over: the point where `session.md` and `meta.json` are flushed.
    pub(crate) fn is_terminal(self) -> bool {
        matches!(self, Self::Idle | Self::Done | Self::Killed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub lane: String,
    #[serde(default)]
    pub model: String,
    /// Hierarchy link for teamwork subsessions. `None` = top-level session.
    /// Backwards-compatible: old `meta.json` files without this key parse as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub status: SessionStatus,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub cost_usd: f64,
    /// How full the context window is: the tokens the model saw on the latest
    /// request plus its reply. Not a spend total (that is `tokens_in` /
    /// `tokens_out`). Drops after a compaction.
    #[serde(default)]
    pub context_tokens: u64,
    /// The window `context_tokens` is measured against, from the catalog
    /// entry of the model that answered. 0 = not measured yet.
    #[serde(default)]
    pub context_limit: u64,
    #[serde(default)]
    pub cwd: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

/// Append-only truth. `session.md` is the rendered human view.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    System {
        text: String,
    },
    User {
        text: String,
    },
    Assistant {
        text: String,
        #[serde(default)]
        done: bool,
    },
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    ToolResult {
        id: String,
        name: String,
        #[serde(default)]
        ok: bool,
        output: String,
        /// Execution time in milliseconds, for the run cards.
        /// u64 (not u128): serde_json cannot serialize u128, which silently
        /// dropped every persisted tool result.
        #[serde(default)]
        ms: u64,
    },
    Widget {
        /// Fence language of the rendered payload (`parzi-widget` / `parzi-diagram`).
        fence: String,
        payload: serde_json::Value,
    },
    Artifact {
        id: String,
        title: String,
        artifact_kind: String,
        version: u32,
        payload: serde_json::Value,
    },
    Checkpoint {
        summary: String,
    },
    Reasoning {
        text: String,
    },
    /// A run failed. The provider's own words, and its class (`auth`,
    /// `rate_limit`, …) — kept in the thread so a failure outlives the toast.
    Error {
        message: String,
        #[serde(default)]
        class: String,
    },
}
