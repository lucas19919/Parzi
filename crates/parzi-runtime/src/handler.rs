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
    Done {
        turns: u32,
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

#[async_trait::async_trait]
pub trait HarnessBridge: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    async fn spawn_session(
        &self,
        caller_id: &str,
        title: &str,
        prompt: &str,
        is_subsession: bool,
        model: Option<String>,
        lane: Option<String>,
        wait: bool,
        mode_override: Option<String>,
        effort: Option<String>,
    ) -> Result<String>;
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
// doc, not code. Sections are ## Tools, ## Teamwork, ## Build, ## Work.
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

pub fn system_parts(lane: &str, cwd: &str) -> Vec<String> {
    let mut parts = vec![if lane.is_empty() {
        harness_intro()
    } else {
        format!("{} Lane: {lane}.", harness_intro())
    }];
    parts.push(harness_section("tools"));
    if lane == "work" {
        parts.push(harness_section("work"));
    } else {
        parts.push(harness_section("teamwork"));
        parts.push(harness_section("build"));
    }
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
        for s in ["tools", "teamwork", "build", "work", "delivery"] {
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
    fn lanes_get_the_right_sections() {
        let build = system_parts("build", "C:\\x");
        assert!(build.iter().any(|p| p.starts_with("## teamwork")));
        assert!(build.iter().any(|p| p.starts_with("## build")));
        assert!(build.iter().any(|p| p.starts_with("## delivery")));
        let work = system_parts("work", "C:\\x");
        assert!(work.iter().any(|p| p.starts_with("## work")));
        assert!(!work.iter().any(|p| p.starts_with("## teamwork")));
        assert!(work.iter().any(|p| p.starts_with("## delivery")));
    }
}
