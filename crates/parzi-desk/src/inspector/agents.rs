//! Agents deck: swarm runs table, approval card, live tool trace.

/// Agent run state. Only here does colour/state meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentStatus {
    Active,
    Queued,
    Done,
    Killed,
    #[default]
    Idle,
}

impl AgentStatus {
    /// Parse a status word; unknown words become `Idle`.
    #[must_use]
    pub fn from_str(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "queued" => Self::Queued,
            "done" => Self::Done,
            "killed" => Self::Killed,
            _ => Self::Idle,
        }
    }

    /// Still consuming budget.
    #[must_use]
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Active | Self::Queued)
    }
}

/// One row of the agents table: root thread or subsession.
#[derive(Debug, Clone, Default)]
pub struct AgentNode {
    pub id: String,
    pub lane: String,
    pub title: String,
    pub model: String,
    pub status: AgentStatus,
    pub tokens: u64,
    pub cost: f64,
    pub tool: Option<String>,
    /// Tree depth; the view clamps indentation at 5.
    pub depth: u8,
}

impl AgentNode {
    /// # Errors
    /// Empty/oversize ids or non-finite/negative cost.
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.id.len() > 64 {
            return Err("agent id: 1-64 chars".to_string());
        }
        if !self.cost.is_finite() || self.cost < 0.0 {
            return Err("agent cost must be finite and >= 0".to_string());
        }
        Ok(())
    }

    /// Model half of `provider/model`, or `auto`.
    #[must_use]
    pub fn short_model(&self) -> String {
        match self.model.split_once('/') {
            Some((_, m)) if !m.is_empty() => m.to_string(),
            _ => "auto".to_string(),
        }
    }
}

/// A tool call from the active thread's events.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw args JSON (summarized by [`arg_summary`]).
    pub args_raw: String,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub id: String,
    pub ok: bool,
    pub ms: u64,
}

/// One trace row: call joined with its result (missing result = running).
#[derive(Debug, Clone)]
pub struct ToolTraceEntry {
    pub name: String,
    pub args: String,
    pub ok: Option<bool>,
    pub ms: u64,
}

/// Keys worth surfacing first, in order (mirrors the web UI).
const ARG_KEYS: &str = "path command cmd query url title session_id id message prompt";
const WS: &[char] = &[' ', '\n', '\r', '\t'];

/// One-line args summary, truncated to 64 chars.
#[must_use]
pub fn arg_summary(raw: &str) -> String {
    for key in ARG_KEYS.split(' ') {
        if let Some(value) = extract_string_value(raw, key) {
            return truncate(&value, 64);
        }
    }
    truncate(&collapse_ws(raw), 64)
}

/// Find `"key" : "value"` and return the value (summaries, not parsing).
fn extract_string_value(raw: &str, key: &str) -> Option<String> {
    let quoted = format!("\"{key}\"");
    let i = raw.find(quoted.as_str())?;
    let rest = raw[i + quoted.len()..].trim_start_matches(WS);
    let head = rest.strip_prefix(':')?.trim_start_matches(WS);
    let mut chars = head.strip_prefix('"')?.chars();
    let mut out = String::new();
    loop {
        match chars.next()? {
            '\\' => out.push(chars.next()?),
            '"' => return Some(out),
            c => out.push(c),
        }
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(s: &str, max: usize) -> String {
    let head: String = s.chars().take(max.saturating_sub(3)).collect();
    if s.chars().count() <= max {
        s.to_string()
    } else {
        head + "..."
    }
}

/// Join calls with results (newest first; missing ones still run).
#[must_use]
pub fn build_trace(
    calls: &[ToolCall],
    results: &[ToolResult],
    limit: usize,
) -> Vec<ToolTraceEntry> {
    let mut out = Vec::new();
    for c in calls.iter().rev().take(limit) {
        let hit = results.iter().find(|r| r.id == c.id);
        out.push(ToolTraceEntry {
            name: c.name.clone(),
            args: arg_summary(&c.args_raw),
            ok: hit.map(|r| r.ok),
            ms: hit.map_or(0, |r| r.ms),
        });
    }
    out
}

/// `1500` -> `1.5k`, `12000` -> `12k`, `2500000` -> `2.5M`.
#[must_use]
pub fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        return format!("{}.{}M", n / 1_000_000, (n % 1_000_000) / 100_000);
    }
    if n >= 10_000 {
        return format!("{}k", n / 1000);
    }
    if n >= 1000 {
        return format!("{}.{}k", n / 1000, (n % 1000) / 100);
    }
    n.to_string()
}

/// `$0`, `<$0.01`, or `$x.xx`.
#[must_use]
pub fn fmt_cost(c: f64) -> String {
    if c.is_nan() || c <= 0.0 {
        return "$0".to_string();
    }
    if c < 0.01 {
        return "<$0.01".to_string();
    }
    format!("${c:.2}")
}

/// `80` -> `80ms`, `1500` -> `1.5s`.
#[must_use]
pub fn fmt_ms(ms: u64) -> String {
    if ms >= 1000 {
        return format!("{}.{}s", ms / 1000, (ms % 1000) / 100);
    }
    format!("{ms}ms")
}

#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub key: String,
    pub name: String,
    pub lane: String,
    pub args_preview: String,
}

/// Actions the deck can request; the bridge executes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentAction {
    Focus(String),
    Fork(String),
    Kill(String),
    Spawn(String),
    Approve { key: String, allow: bool },
}

/// Deck state: nodes, selection, approval, active thread events.
#[derive(Debug, Default)]
pub struct AgentsState {
    pub nodes: Vec<AgentNode>,
    pub active_id: Option<String>,
    pub approval: Option<PendingApproval>,
    pub trace_calls: Vec<ToolCall>,
    pub trace_results: Vec<ToolResult>,
}

impl AgentsState {
    /// (agents, live agents, total tokens, total cost).
    #[must_use]
    pub fn summary(&self) -> (usize, usize, u64, f64) {
        let live = self.nodes.iter().filter(|n| n.status.is_live()).count();
        let tokens = self.nodes.iter().map(|n| n.tokens).sum();
        let cost = self.nodes.iter().map(|n| n.cost).sum();
        (self.nodes.len(), live, tokens, cost)
    }

    /// Rows in tree order (the bridge emits pre-order).
    #[must_use]
    pub fn visible(&self) -> Vec<&AgentNode> {
        self.nodes.iter().collect()
    }
}

/// Thin deck; returns the first action clicked this frame, if any.
pub fn show(ui: &mut egui::Ui, s: &mut AgentsState) -> Option<AgentAction> {
    if s.nodes.is_empty() {
        ui.label("No swarm yet: open a thread. Subsessions appear here as a live tree.");
        return None;
    }
    let (total, live, tokens, cost) = s.summary();
    ui.horizontal(|ui| {
        ui.label(format!("{total} agents ({live} live)"));
        ui.label(format!("{} - {}", fmt_tokens(tokens), fmt_cost(cost)));
    });
    let mut action: Option<AgentAction> = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for node in s.visible() {
            ui.horizontal(|ui| {
                ui.add_space(f32::from(node.depth.min(5)) * 14.0);
                let dot = match node.status {
                    AgentStatus::Active => "*",
                    AgentStatus::Queued => "~",
                    AgentStatus::Done => "+",
                    AgentStatus::Killed => "x",
                    AgentStatus::Idle => "-",
                };
                ui.label(dot);
                let t = if node.title.is_empty() {
                    "untitled"
                } else {
                    &node.title
                };
                let here = Some(&node.id) == s.active_id.as_ref();
                let mark = if here { "> " } else { "" };
                let label = format!("{mark}{} - {t}", node.lane);
                if ui.button(&label).clicked() {
                    action = Some(AgentAction::Focus(node.id.clone()));
                }
                let model = node.short_model();
                let toks = fmt_tokens(node.tokens);
                ui.weak(format!("{model} - {toks}"));
                ui.weak(node.tool.clone().unwrap_or_else(|| "-".to_string()));
                if ui.small_button("Fork").clicked() {
                    action = Some(AgentAction::Fork(node.id.clone()));
                }
                if node.status.is_live() {
                    if ui.small_button("Kill").clicked() {
                        action = Some(AgentAction::Kill(node.id.clone()));
                    }
                } else if ui.small_button("Spawn").clicked() {
                    action = Some(AgentAction::Spawn(node.id.clone()));
                }
            });
        }
        if let Some(appr) = &s.approval {
            ui.separator();
            ui.label(format!("Approval - {} - lane {}", appr.name, appr.lane));
            ui.monospace(&appr.args_preview);
            ui.horizontal(|ui| {
                for (label, allow) in [("Approve", true), ("Deny", false)] {
                    if ui.button(label).clicked() {
                        action = Some(AgentAction::Approve {
                            key: appr.key.clone(),
                            allow,
                        });
                    }
                }
            });
        }
        ui.label("Tool trace");
        let trace = build_trace(&s.trace_calls, &s.trace_results, 14);
        if trace.is_empty() {
            ui.label("No tool calls in this thread yet.");
        }
        for t in trace {
            let live = t.ok.is_none();
            let mark = if live {
                ".."
            } else if t.ok == Some(true) {
                "+"
            } else {
                "x"
            };
            let when = if live {
                "running".to_string()
            } else {
                fmt_ms(t.ms)
            };
            ui.monospace(format!("{mark} {} {} {when}", t.name, t.args));
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, status: AgentStatus) -> AgentNode {
        AgentNode {
            id: id.into(),
            status,
            model: "claude/opus".into(),
            tokens: 1500,
            cost: 0.5,
            ..Default::default()
        }
    }

    #[test]
    fn status_tokens_cost_and_time() {
        assert_eq!(AgentStatus::from_str("active"), AgentStatus::Active);
        assert_eq!(AgentStatus::from_str("bogus"), AgentStatus::Idle);
        assert!(AgentStatus::Queued.is_live() && !AgentStatus::Done.is_live());
        assert_eq!(fmt_tokens(999), "999");
        assert_eq!(fmt_tokens(1500), "1.5k");
        assert_eq!(fmt_tokens(12_000), "12k");
        assert_eq!(fmt_tokens(2_500_000), "2.5M");
        assert_eq!(fmt_cost(0.0), "$0");
        assert_eq!(fmt_cost(0.005), "<$0.01");
        assert_eq!(fmt_cost(1.5), "$1.50");
        assert_eq!(fmt_ms(80), "80ms");
        assert_eq!(fmt_ms(1500), "1.5s");
    }

    fn call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            args_raw: "{}".into(),
        }
    }

    fn done(id: &str) -> ToolResult {
        ToolResult {
            id: id.into(),
            ok: true,
            ms: 42,
        }
    }

    #[test]
    fn args_and_trace() {
        assert_eq!(arg_summary(r#"{"path": "/tmp/x.md"}"#), "/tmp/x.md");
        assert_eq!(arg_summary(r#"{"a":1}"#), r#"{"a":1}"#);
        assert_eq!(arg_summary(r#"{"path": "a\"b"}"#), "a\"b");
        assert!(arg_summary(&"x".repeat(100)).chars().count() <= 64);
        let calls = vec![call("1", "read"), call("2", "run")];
        let results = vec![done("1")];
        let trace = build_trace(&calls, &results, 14);
        assert_eq!(trace.len(), 2);
        assert!(trace[0].ok.is_none() && trace[1].ok == Some(true) && trace[1].ms == 42);
        assert_eq!(build_trace(&calls, &results, 1).len(), 1);
    }

    #[test]
    fn summary_filter_validate_and_models() {
        let mut s = AgentsState::default();
        s.nodes.push(node("a", AgentStatus::Active));
        s.nodes.push(node("b", AgentStatus::Done));
        assert_eq!(s.summary(), (2, 1, 3000, 1.0));
        assert_eq!(s.visible().len(), 2);
        assert_eq!(s.nodes[0].short_model(), "opus");
        assert!(node("", AgentStatus::Idle).validate().is_err());
        let mut bad = node("a", AgentStatus::Idle);
        bad.cost = f64::NAN;
        assert!(bad.validate().is_err());
    }
}
