use parzi_core::context::InterKind;
use parzi_core::error::Result;
use tokio::sync::{broadcast, mpsc};

use crate::tools::ToolCallInfo;

pub(crate) type RunEventBus = broadcast::Sender<(String, RunEvent)>;

#[derive(Debug, Clone)]
pub enum RunEvent {
    Text(String),
    Reasoning {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        label: String,
    },
    ToolResult {
        id: String,
        name: String,
        ok: bool,
        ms: u64,
    },
    Usage {
        tokens_in: u64,
        tokens_out: u64,
        cost_usd: f64,
    },
    Context {
        used: u64,
        limit: u64,
    },
    ApprovalRequest {
        call: ToolCallInfo,
    },
    Notice {
        text: String,
    },
    /// The session's checklist, as `todo.write` just set it.
    Todo {
        todo: serde_json::Value,
    },
    Done {
        turns: u32,
    },
    /// The run stopped at the budget: not running, not done either.
    Paused {
        reason: String,
    },
    Error(String),
}

#[derive(Clone)]
pub struct RunSink {
    session_id: String,
    sink: mpsc::UnboundedSender<RunEvent>,
    bus: Option<RunEventBus>,
}

impl RunSink {
    pub fn new(
        session_id: &str,
        sink: mpsc::UnboundedSender<RunEvent>,
        bus: Option<RunEventBus>,
    ) -> Self {
        Self {
            session_id: session_id.to_string(),
            sink,
            bus,
        }
    }

    pub fn emit(&self, e: RunEvent) {
        if let Some(bus) = &self.bus {
            let _ = bus.send((self.session_id.clone(), e.clone()));
        }
        let _ = self.sink.send(e);
    }
}

/// A child session to start, as `HarnessBridge::spawn` takes it.
#[derive(Debug, Clone, Default)]
pub struct SpawnSpec {
    pub title: String,
    pub prompt: String,
    pub is_subsession: bool,
    pub model: Option<String>,
    pub wait: bool,
    pub mode_override: Option<String>,
    pub effort: Option<String>,
}

#[async_trait::async_trait]
pub trait HarnessBridge: Send + Sync {
    /// Starts a child session; the reply is the tool result's JSON.
    async fn spawn(&self, caller_id: &str, spec: SpawnSpec) -> Result<String>;
    async fn send_message(
        &self,
        caller_id: &str,
        session_id: &str,
        message: &str,
        kind: InterKind,
        wait: bool,
        effort: Option<String>,
    ) -> Result<String>;
    async fn read_session(
        &self,
        caller_id: &str,
        session_id: &str,
        tail_events: Option<usize>,
    ) -> Result<String>;
    async fn list_sessions(&self, caller_id: &str, only_subsessions: bool) -> Result<String>;
}

pub(crate) fn user_event_text(
    prompt: &str,
    attachments: &[parzi_core::context::AttachedFile],
) -> String {
    let mut text = prompt.to_string();
    if !attachments.is_empty() {
        let names: Vec<&str> = attachments.iter().map(|a| a.path.as_str()).collect();
        text.push_str(&format!("\n[attached: {}]", names.join(", ")));
    }
    text
}

// The harness prompt lives in parzi.md at the repo root so it reads like a
// doc, not code. Sections are ## Tools, ## Teamwork, ## Plans, ## Delivery.
const HARNESS_DOC: &str = include_str!("../../../parzi.md");

fn harness_section(name: &str) -> String {
    let mut buf = String::new();
    let mut inside = false;
    for line in HARNESS_DOC.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            if inside {
                break;
            }
            inside = h.trim().eq_ignore_ascii_case(name);
            continue;
        }
        if inside {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    format!("## {name}\n{}", buf.trim())
}

fn harness_intro() -> String {
    let end = HARNESS_DOC.find("## ").unwrap_or(HARNESS_DOC.len());
    HARNESS_DOC[..end].trim().to_string()
}

/// A run event in the shape the desk's UI reads from `parzi://run-event`
/// (`kind` tagged, snake case). `parzi serve` streams these to a remote
/// desk, which forwards them unchanged. Usage and approval requests have
/// their own paths and map to None.
#[must_use]
pub fn ui_event(session: &str, ev: &RunEvent) -> Option<serde_json::Value> {
    use serde_json::json;
    Some(match ev {
        RunEvent::Text(text) => json!({ "kind": "text", "session": session, "text": text }),
        RunEvent::Reasoning { text } => {
            json!({ "kind": "reasoning", "session": session, "text": text })
        }
        RunEvent::ToolCall { id, name, label } => {
            json!({ "kind": "tool_call", "session": session, "id": id, "name": name, "label": label })
        }
        RunEvent::ToolResult { id, name, ok, ms } => {
            json!({ "kind": "tool_result", "session": session, "id": id, "name": name, "ok": ok, "ms": ms })
        }
        RunEvent::Notice { text } => json!({ "kind": "notice", "session": session, "text": text }),
        RunEvent::Todo { todo } => json!({ "kind": "todo", "session": session, "todo": todo }),
        RunEvent::Context { used, limit } => {
            json!({ "kind": "context", "session": session, "used": used, "limit": limit })
        }
        RunEvent::Done { turns } => json!({ "kind": "done", "session": session, "turns": turns }),
        RunEvent::Paused { reason } => {
            json!({ "kind": "paused", "session": session, "reason": reason })
        }
        RunEvent::Error(error) => json!({ "kind": "error", "session": session, "error": error }),
        RunEvent::Usage { .. } | RunEvent::ApprovalRequest { .. } => return None,
    })
}

/// The instructions every session gets.
pub fn system_parts(cwd: &str) -> Vec<String> {
    let mut parts = vec![harness_intro()];
    parts.push(harness_section("tools"));
    parts.push(harness_section("teamwork"));
    parts.push(harness_section("plans"));
    parts.push(harness_section("delivery"));
    if let Some(text) = parzi_core::system::global() {
        parts.push(format!("# Global instructions\n\n{text}"));
    }
    if let Some(ctx) = parzi_core::brain::context_for(std::path::Path::new(cwd.trim())) {
        parts.push(ctx.text);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_doc_has_every_section() {
        for s in ["tools", "teamwork", "plans", "delivery"] {
            let body = harness_section(s);
            assert!(body.lines().count() > 3, "section {s} is missing or empty");
        }
        assert!(harness_intro().contains("Parzi"));
    }

    #[test]
    fn harness_doc_has_no_em_dashes() {
        assert!(
            !HARNESS_DOC.contains('—'),
            "parzi.md must not contain em dashes"
        );
        assert!(
            !HARNESS_DOC.contains('–'),
            "parzi.md must not contain en dashes"
        );
    }

    #[test]
    fn every_session_gets_every_section() {
        let parts = system_parts("C:\\x");
        for section in [
            "## tools",
            "## teamwork",
            "## plans",
            "## delivery",
        ] {
            assert!(parts.iter().any(|p| p.starts_with(section)), "{section}");
        }
        assert!(!parts.iter().any(|p| p.contains("Lane:")), "no lanes");
        {
            let plans = parts
                .iter()
                .find(|p| p.starts_with("## plans"))
                .expect("it plans");
            for word in [
                "plan.write",
                "plan.read",
                "5 to 15 items",
                "needs",
                "plan/<id>",
                "`active`",
                "`done`",
                "<parzi:plan>",
                "session.rename",
                "2 to 5 words",
            ] {
                assert!(plans.contains(word), "the plans section says {word}");
            }
        }
    }

    #[test]
    fn the_plan_stays_out_of_the_instructions() {
        // The instructions are re-sent every turn; a changing plan there
        // would spoil the prompt cache.
        assert!(system_parts("C:\\x")
            .iter()
            .all(|p| !p.contains("<parzi:plan project")));
    }
}
