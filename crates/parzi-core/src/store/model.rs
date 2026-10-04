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
    Queued,
}

impl SessionStatus {
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
    #[serde(default)]
    pub context_tokens: u64,
    #[serde(default)]
    pub context_limit: u64,
    #[serde(default)]
    pub cwd: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

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
        #[serde(default)]
        ms: u64,
    },
    Widget {
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
    Error {
        message: String,
        #[serde(default)]
        class: String,
    },
}
