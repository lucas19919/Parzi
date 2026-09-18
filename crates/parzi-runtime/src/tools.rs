//! Parzi's own tools (widgets, teamwork, plans, knowledge, leases) plus the
//! connectors a lane allows, behind one executor. Vendor agents reach them
//! over MCP; their own file and shell tools are theirs, and only their
//! permission requests pass through Parzi (see `toolhost`). Lane allowlists
//! decide what runs. Approvals pause the run, never the tool.

use crate::board_tools::{board_defs, is_board_tool};
use crate::lease_tools::{is_lease_tool, lease_defs, LeaseCtx};
use crate::mcp::McpManager;

/// One tool as an agent sees it: name, description, JSON schema.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    Auto,
    Ask,
    Deny,
}

impl ApprovalMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "auto" => Self::Auto,
            "deny" => Self::Deny,
            _ => Self::Ask,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
    pub lane: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    Allow,
    Deny,
}

/// Implemented by the UI (dialog) or CLI (flag/prompt). Test doubles approve.
#[async_trait::async_trait]
pub trait Approver: Send + Sync {
    async fn approve(&self, call: &ToolCallInfo) -> Approval;
}

pub struct AutoApprover;
pub struct DenyApprover;

#[async_trait::async_trait]
impl Approver for AutoApprover {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

#[async_trait::async_trait]
impl Approver for DenyApprover {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Deny
    }
}

pub struct ToolExecutor {
    pub cwd: String,
    pub mcp: std::sync::Arc<McpManager>,
    pub allowed: Vec<String>,
    /// A project lane's place in the lease layer (PLAN §4). `None` for an
    /// ordinary thread: no leases, no board, and no write gate.
    pub leases: Option<LeaseCtx>,
}

impl ToolExecutor {
    /// Lane allowlist: exact `fs.read`, prefix `fs.*`, or `*`. Deny by default.
    pub fn is_allowed(&self, name: &str) -> bool {
        self.allowed.iter().any(|p| {
            p == "*" || p == name || (p.ends_with(".*") && name.starts_with(&p[..p.len() - 1]))
        })
    }

    /// Per-tool approval override for MCP tools (`auto`|`ask`|`deny`).
    /// Local/ui/session/plan/lane tools have no per-tool override: None = lane mode wins.
    pub fn approval_override(&self, name: &str) -> Option<ApprovalMode> {
        if is_vendor_category(name)
            || is_ui_tool(name)
            || is_session_tool(name)
            || is_plan_tool(name)
            || is_lane_tool(name)
            || is_lease_tool(name)
            || is_board_tool(name)
        {
            return None;
        }
        let (server, tool) = name.split_once('.')?;
        match self.mcp.tool_mode(server, tool).as_deref() {
            Some("auto") => Some(ApprovalMode::Auto),
            Some("deny") => Some(ApprovalMode::Deny),
            Some(_) => Some(ApprovalMode::Ask),
            None => None,
        }
    }

    pub fn defs(&self) -> Vec<ToolDef> {
        let mut d = ui_defs();
        d.extend(session_defs());
        // Only a lane that is in the lease layer sees `lease.*` / `board.*`:
        // a plain thread has no task to check out and no board to read.
        if self.leases.is_some() {
            d.extend(lease_defs());
            d.extend(board_defs());
        }
        d
    }

    /// Parzi's own defs plus the currently-exposed MCP tools that also
    /// pass the lane allowlist. Best-effort: an unreachable server contributes
    /// nothing instead of failing the run. Call once per run start.
    pub async fn defs_with_mcp(&self) -> Vec<ToolDef> {
        let mut d = self.defs();
        for server in self.mcp.server_names() {
            let tools = match tokio::time::timeout(
                std::time::Duration::from_secs(12),
                self.mcp.exposed_tools(&server),
            )
            .await
            {
                Ok(Ok(t)) => t,
                _ => continue,
            };
            for t in tools {
                let q = t.qualified();
                if self.is_allowed(&q) {
                    d.push(t.to_def());
                }
            }
        }
        d
    }

    pub async fn execute(&self, name: &str, args: &serde_json::Value) -> (bool, String) {
        if !self.is_allowed(name) {
            return (false, format!("tool `{name}` is not allowed in this lane"));
        }
        if is_lease_tool(name) {
            return self.execute_lease_tool(name, args).await;
        }
        if is_board_tool(name) {
            return self.execute_board_tool(name, args).await;
        }
        if is_plan_tool(name) {
            return execute_plan_tool(name, args).await;
        }
        if is_knowledge_tool(name) {
            return execute_knowledge_tool(name, args).await;
        }
        if let Some((server, tool)) = name.split_once('.') {
            if is_vendor_category(name) {
                return (
                    false,
                    format!("`{name}` is the agent's own tool now, not Parzi's"),
                );
            }
            if is_ui_tool(name) || is_session_tool(name) || is_lane_tool(name) {
                return (false, format!("tool `{name}` is handled by the agent loop"));
            }
            // Server exposure gate (allow/deny lists) + per-tool deny override.
            if !self.mcp.is_tool_exposed(server, tool) {
                return (
                    false,
                    format!("tool `{name}` is disabled for this connector"),
                );
            }
            if self.mcp.tool_mode(server, tool).as_deref() == Some("deny") {
                return (
                    false,
                    format!("tool `{name}` is blocked by connector policy"),
                );
            }
            return self.mcp.call_tool(server, tool, args.clone()).await;
        }
        (false, format!("unknown tool `{name}`"))
    }
}

/// The agent's own file and shell tools, by the names lane allowlists have
/// always used: `fs.read` covers reads and searches, `fs.write` edits,
/// `shell.exec` commands. Parzi no longer runs these itself; a lane that
/// lists tools and leaves one out refuses the matching vendor action.
pub fn is_vendor_category(name: &str) -> bool {
    matches!(name, "fs.read" | "fs.write" | "fs.list" | "shell.exec")
}

pub fn ui_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "ui.show_markdown".into(),
            description: "Render rich markdown in the thread.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"markdown": {"type": "string"}},
                "required": ["markdown"],
            }),
        },
        ToolDef {
            name: "ui.show_widget".into(),
            description: "Render a rich widget card (table, chart, kanban, progress, stat, list, markdown). Prefer this over ASCII tables/boxes. Types: stat{title,text,sub} progress{title,value 0..1} list{title,items[]} table{title,columns[],rows[][]} chart-line/chart-bar{title,points[number[]]} kanban{columns[{title,cards[]}]} markdown{title,text}. Example: {\"widget\":1,\"type\":\"table\",\"title\":\"Endpoints\",\"columns\":[\"Route\",\"Method\"],\"rows\":[[\"/api\",\"GET\"]]}.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "widget": {"type": "number", "const": 1},
                    "type": {"type": "string", "enum": ["stat","progress","list","table","chart-line","chart-bar","kanban","markdown"]},
                    "title": {"type": "string"},
                    "text": {"type": "string"},
                    "sub": {"type": "string"},
                    "value": {"type": "number"},
                    "items": {"type": "array", "items": {}},
                    "columns": {"type": "array", "items": {}},
                    "rows": {"type": "array", "items": {"type": "array", "items": {}}},
                    "points": {"type": "array", "items": {"type": "number"}},
                    "payload": {"type": "object"},
                },
                "required": ["widget", "type"],
            }),
        },
        ToolDef {
            name: "ui.show_diagram".into(),
            description: "Render an architecture/flow diagram as SVG nodes+edges. ALWAYS use this for architecture, data flow, sequence, or component diagrams — never ASCII boxes (+---|etc), which break on narrow screens. Example: {\"diagram\":1,\"title\":\"API flow\",\"nodes\":[{\"id\":\"ui\",\"label\":\"UI\"},{\"id\":\"api\",\"label\":\"API\"}],\"edges\":[{\"from\":\"ui\",\"to\":\"api\",\"label\":\"POST\"}]}. Max 200 nodes / 400 edges.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "diagram": {"type": "number", "const": 1},
                    "title": {"type": "string"},
                    "nodes": {"type": "array", "items": {"type": "object", "properties": {"id": {"type": "string"}, "label": {"type": "string"}}, "required": ["id"]}},
                    "edges": {"type": "array", "items": {"type": "object", "properties": {"from": {"type": "string"}, "to": {"type": "string"}, "label": {"type": "string"}}, "required": ["from", "to"]}},
                },
                "required": ["diagram", "nodes", "edges"],
            }),
        },
        ToolDef {
            name: "ui.show_artifact".into(),
            description: "Save/update a versioned artifact card with copy/save/open-in-deck actions. Use for code over ~15 lines, full files, markdown docs, html/svg previews, json/csv data, diffs. Kinds: code|markdown|html|svg|json|csv|diff|text. Languages: rust|typescript|javascript|python|toml|json|bash|sh|diff|markdown|md|html|css. Reuse the same id to bump the version. Example: {\"artifact\":1,\"id\":\"auth-middleware\",\"title\":\"Auth middleware\",\"kind\":\"code\",\"language\":\"typescript\",\"content\":\"...\"}.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "artifact": {"type": "number", "const": 1},
                    "id": {"type": "string", "description": "slug [a-z0-9-], reused across versions"},
                    "title": {"type": "string"},
                    "kind": {"type": "string", "enum": ["code","markdown","html","svg","json","csv","diff","text"]},
                    "language": {"type": "string"},
                    "content": {"type": "string"},
                },
                "required": ["artifact", "content"],
            }),
        },
    ]
}

pub fn is_ui_tool(name: &str) -> bool {
    matches!(
        name,
        "ui.show_markdown" | "ui.show_widget" | "ui.show_diagram" | "ui.show_artifact"
    )
}

/// First-class harness session tools: agents spawn child subsessions (or full
/// root sessions), message across sessions, and inspect other transcripts.
/// In lane `Ask` mode these surface as approval cards like any other tool.
pub fn session_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "session.spawn".into(),
            description: "Spawn a child subsession (is_subsession=true, nested under this session) or a full independent top-level session (is_subsession=false). wait=true blocks for the child's reply; wait=false returns immediately with the new session id for background teamwork.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "prompt": {"type": "string"},
                    "is_subsession": {"type": "boolean"},
                    "model": {"type": "string"},
                    "lane": {"type": "string"},
                    "wait": {"type": "boolean"},
                },
                "required": ["title", "prompt"],
            }),
        },
        ToolDef {
            name: "session.send_message".into(),
            description: "Send a message to another session in this project (child, parent, or peer) and optionally wait for its reply. It arrives as typed, untrusted data from your run — not as a user turn — so say what you want done, plainly.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"},
                    "message": {"type": "string"},
                    "kind": {
                        "type": "string",
                        "enum": ["text", "lease_request", "lease_answer", "convene"],
                    },
                    "wait": {"type": "boolean"},
                },
                "required": ["session_id", "message"],
            }),
        },
        ToolDef {
            name: "session.read_session".into(),
            description: "Inspect another session's title, status, and recent transcript tail without switching to it.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"},
                    "tail_events": {"type": "number"},
                },
                "required": ["session_id"],
            }),
        },
        ToolDef {
            name: "session.list_sessions".into(),
            description: "List sessions and their status. only_subsessions=true restricts to this session's child subsessions.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "only_subsessions": {"type": "boolean"},
                },
            }),
        },
        ToolDef {
            name: "plan.read".into(),
            description: "Read the project's living PLAN.md (milestones + lane-tagged tasks).".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"project": {"type": "string"}},
                "required": ["project"],
            }),
        },
        ToolDef {
            name: "plan.update".into(),
            description: "Update one living-plan task checkbox by title match (done=true/false) or append a new task line.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project": {"type": "string"},
                    "title_match": {"type": "string"},
                    "done": {"type": "boolean"},
                    "append": {"type": "string"},
                },
                "required": ["project"],
            }),
        },
        ToolDef {
            name: "lane.dispatch".into(),
            description: "Orchestrator-only: spawn a lane worker subsession with the project's implementation role settings (model/effort/system prompt).".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "prompt": {"type": "string"},
                    "lane": {"type": "string"},
                    "wait": {"type": "boolean"},
                },
                "required": ["title", "prompt"],
            }),
        },
        ToolDef {
            name: "knowledge.read".into(),
            description: "Read the project's cumulative knowledge and lessons learned (KNOWLEDGE.md).".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"project": {"type": "string"}},
                "required": ["project"],
            }),
        },
        ToolDef {
            name: "knowledge.record".into(),
            description: "Record an architectural decision, discovered pattern, or gotcha into the project's cumulative knowledge base.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project": {"type": "string"},
                    "note": {"type": "string"},
                    "category": {"type": "string", "enum": ["decision", "pattern", "gotcha"]},
                },
                "required": ["project", "note"],
            }),
        },
    ]
}

pub fn is_plan_tool(name: &str) -> bool {
    matches!(name, "plan.read" | "plan.update")
}

pub fn is_knowledge_tool(name: &str) -> bool {
    matches!(name, "knowledge.read" | "knowledge.record")
}

pub fn is_lane_tool(name: &str) -> bool {
    matches!(name, "lane.dispatch")
}

pub fn is_session_tool(name: &str) -> bool {
    matches!(
        name,
        "session.spawn" | "session.send_message" | "session.read_session" | "session.list_sessions"
    )
}

/// Built-in (non-connector) tool catalog: the single source of truth for how
/// base tools are grouped and described in Settings. Groups: Files, Shell,
/// Teamwork, Plans, Knowledge, Display.
pub struct BuiltinTool {
    pub name: &'static str,
    pub group: &'static str,
    pub blurb: &'static str,
}

pub fn builtin_tools() -> Vec<BuiltinTool> {
    vec![
        BuiltinTool {
            name: "fs.read",
            group: "Files",
            blurb: "The agent reads and searches files",
        },
        BuiltinTool {
            name: "fs.write",
            group: "Files",
            blurb: "The agent edits and creates files",
        },
        BuiltinTool {
            name: "shell.exec",
            group: "Shell",
            blurb: "The agent runs shell commands",
        },
        BuiltinTool {
            name: "session.spawn",
            group: "Teamwork",
            blurb: "Spawn child subsessions",
        },
        BuiltinTool {
            name: "session.send_message",
            group: "Teamwork",
            blurb: "Message other sessions",
        },
        BuiltinTool {
            name: "session.read_session",
            group: "Teamwork",
            blurb: "Inspect other transcripts",
        },
        BuiltinTool {
            name: "session.list_sessions",
            group: "Teamwork",
            blurb: "List sessions",
        },
        BuiltinTool {
            name: "plan.read",
            group: "Plans",
            blurb: "Read the living project plan",
        },
        BuiltinTool {
            name: "plan.update",
            group: "Plans",
            blurb: "Update plan checkboxes",
        },
        BuiltinTool {
            name: "lane.dispatch",
            group: "Plans",
            blurb: "Dispatch a lane worker",
        },
        BuiltinTool {
            name: "knowledge.read",
            group: "Knowledge",
            blurb: "Read recorded project knowledge",
        },
        BuiltinTool {
            name: "knowledge.record",
            group: "Knowledge",
            blurb: "Record durable project knowledge",
        },
        BuiltinTool {
            name: "ui.show_markdown",
            group: "Display",
            blurb: "Render rich text (always on)",
        },
        BuiltinTool {
            name: "ui.show_widget",
            group: "Display",
            blurb: "Render cards and charts (always on)",
        },
        BuiltinTool {
            name: "ui.show_diagram",
            group: "Display",
            blurb: "Render diagrams (always on)",
        },
        BuiltinTool {
            name: "ui.show_artifact",
            group: "Display",
            blurb: "Save versioned artifacts",
        },
    ]
}

/// Group label for any known tool name (MCP `server.tool` names report
/// "Connector"). Used by Settings and diagnostics.
pub fn tool_group(name: &str) -> &'static str {
    for t in builtin_tools() {
        if t.name == name {
            return t.group;
        }
    }
    "Connector"
}

/// One-line human status for a tool call ("Reading Cargo.toml"). Pure and
/// unit-tested: the single source for the live "Now:" line in the GUI, the
/// CLI `[tool]` line, and the viewer. Unknown/connector tools fall back to
/// `Calling server.tool` plus the most interesting string arg, so opaque MCP
/// tools still read sensibly without an LLM.
pub fn humanize_tool_call(name: &str, args: &serde_json::Value) -> String {
    let str_arg = |k: &str| {
        args.get(k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    // First interesting string arg for opaque tools (path > file > cmd >
    // query > prompt > title > url > ...). Keys are exact-match on purpose:
    // guessing shapes from training data is how args get misread.
    let hint = [
        "path", "file", "cmd", "query", "prompt", "title", "url", "message", "note", "number", "id",
    ]
    .iter()
    .find_map(|k| str_arg(k));
    // A shell command arrives as a string (Claude) or an argv list (Codex).
    let command = || match args.get("command") {
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        Some(v) => v.as_str().unwrap_or_default().to_string(),
        None => str_arg("cmd").unwrap_or_default(),
    };
    let parzi = display_name(name);
    if parzi != name {
        return humanize_tool_call(&parzi, args);
    }
    match name {
        // The agents' own tools (Claude Code, Codex).
        "Bash" | "shell" => format!("Running `{}`", one_line(&command(), 60)),
        "Read" => format!(
            "Reading {}",
            str_arg("file_path").unwrap_or_else(|| "a file".into())
        ),
        "Edit" | "MultiEdit" => format!(
            "Editing {}",
            str_arg("file_path").unwrap_or_else(|| "a file".into())
        ),
        "Write" => format!(
            "Writing {}",
            str_arg("file_path").unwrap_or_else(|| "a file".into())
        ),
        "NotebookEdit" => format!(
            "Editing {}",
            str_arg("notebook_path").unwrap_or_else(|| "a notebook".into())
        ),
        "Glob" => format!(
            "Finding {}",
            str_arg("pattern").unwrap_or_else(|| "files".into())
        ),
        "Grep" => format!(
            "Searching for {}",
            one_line(&str_arg("pattern").unwrap_or_default(), 60)
        ),
        "WebFetch" => format!(
            "Fetching {}",
            str_arg("url").unwrap_or_else(|| "a page".into())
        ),
        "WebSearch" | "web_search" => format!(
            "Searching the web: {}",
            one_line(&str_arg("query").unwrap_or_default(), 60)
        ),
        "Task" | "Agent" => format!(
            "Delegating: {}",
            one_line(
                &str_arg("description").unwrap_or_else(|| "a subtask".into()),
                60
            )
        ),
        "TodoWrite" => "Updating the todo list".into(),
        "edit" => match args.get("paths").and_then(|p| p.as_array()) {
            Some(paths) if paths.len() > 1 => format!(
                "Editing {} and {} more",
                paths[0].as_str().unwrap_or("a file"),
                paths.len() - 1
            ),
            Some(paths) if !paths.is_empty() => {
                format!("Editing {}", paths[0].as_str().unwrap_or("a file"))
            }
            _ => "Editing files".into(),
        },
        "fs.read" => format!("Reading {}", str_arg("path").unwrap_or_else(|| "?".into())),
        "fs.write" => format!("Writing {}", str_arg("path").unwrap_or_else(|| "?".into())),
        "shell.exec" => format!("Running `{}`", one_line(&command(), 60)),
        "session.spawn" => format!(
            "Delegating: {}",
            one_line(&str_arg("title").unwrap_or_else(|| "subsession".into()), 60)
        ),
        "session.send_message" => format!(
            "Messaging {}",
            short_id(&str_arg("session_id").unwrap_or_else(|| "session".into()))
        ),
        "session.read_session" => "Reading session".into(),
        "session.list_sessions" => "Listing sessions".into(),
        "plan.read" => "Reading plan".into(),
        "plan.update" => format!(
            "Updating plan: {}",
            one_line(
                &str_arg("title_match")
                    .or_else(|| str_arg("append"))
                    .unwrap_or_default(),
                60
            )
        ),
        "lane.dispatch" => format!(
            "Dispatching worker: {}",
            one_line(&str_arg("title").unwrap_or_else(|| "worker".into()), 60)
        ),
        "knowledge.read" => "Reading knowledge".into(),
        "knowledge.record" => "Recording knowledge".into(),
        // PLAN §4: what the lease layer is doing, in a person's words.
        "lease.claim" => format!(
            "Checking out {}",
            str_arg("task").unwrap_or_else(|| "a task".into())
        ),
        "lease.release" => format!(
            "Releasing {}",
            str_arg("task").unwrap_or_else(|| "a task".into())
        ),
        "lease.request" => format!(
            "Asking for {}",
            str_arg("path").unwrap_or_else(|| "a file".into())
        ),
        "lease.grant" => "Handing the file over".into(),
        "lease.deny" => "Keeping the file".into(),
        "lease.transfer" => format!(
            "Transfer {} to another lane?",
            str_arg("path").unwrap_or_else(|| "a critical file".into())
        ),
        "board.list" => "Reading the board".into(),
        "board.handoff" => format!(
            "Handing off {}",
            str_arg("task").unwrap_or_else(|| "the task".into())
        ),
        "board.block" => format!(
            "Blocking {}",
            str_arg("task").unwrap_or_else(|| "the task".into())
        ),
        "ui.show_markdown" => "Rendering text".into(),
        "ui.show_widget" => format!(
            "Rendering {}",
            one_line(
                &str_arg("title")
                    .unwrap_or_else(|| str_arg("type").unwrap_or_else(|| "widget".into())),
                60
            )
        ),
        "ui.show_diagram" => "Rendering diagram".into(),
        "ui.show_artifact" => format!(
            "Saving {}",
            one_line(
                &str_arg("title")
                    .or_else(|| str_arg("id"))
                    .unwrap_or_else(|| "artifact".into()),
                60
            )
        ),
        _ => match hint {
            Some(h) => format!("Calling {name} {}", one_line(&h, 60)),
            None => format!("Calling {name}"),
        },
    }
}

/// Parzi's namespaces: a tool name is `<namespace>.<name>`.
const NAMESPACES: &[&str] = &[
    "ui",
    "session",
    "plan",
    "lane",
    "knowledge",
    "lease",
    "board",
    "project",
];

/// The name a tool travels under over MCP: dots are not allowed in tool
/// names by every vendor, so `ui.show_widget` goes out as `ui_show_widget`.
pub fn to_mcp(name: &str) -> String {
    name.replace('.', "_")
}

/// Back from `to_mcp` for Parzi's own namespaces (`ui_show_widget` →
/// `ui.show_widget`). Connector names come back unchanged.
pub fn from_mcp(tool: &str) -> String {
    for ns in NAMESPACES {
        if let Some(rest) = tool.strip_prefix(ns).and_then(|r| r.strip_prefix('_')) {
            return format!("{ns}.{rest}");
        }
    }
    tool.to_string()
}

/// Parzi's own name for a tool an agent reports under its MCP name —
/// Claude says `mcp__parzi__ui_show_widget`, Codex `parzi.ui_show_widget`,
/// OpenCode `parzi_ui_show_widget`. Any other name comes back unchanged.
pub fn display_name(name: &str) -> String {
    name.strip_prefix("mcp__parzi__")
        .or_else(|| name.strip_prefix("parzi."))
        .or_else(|| name.strip_prefix("parzi_"))
        .map_or_else(|| name.to_string(), from_mcp)
}

/// First line of `s`, capped at `n` chars (no newlines leak into status lines).
fn one_line(s: &str, n: usize) -> String {
    let first = s.lines().next().unwrap_or("").trim();
    let cut: String = first.chars().take(n).collect();
    if first.chars().count() > n {
        format!("{cut}…")
    } else {
        cut
    }
}

/// Session ids are UUIDs; status lines only need the head.
fn short_id(s: &str) -> String {
    if s.len() > 8 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        s.chars().take(8).collect()
    } else {
        one_line(s, 24)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_every_advertised_tool_exactly_once() {
        // session_defs carries session.* + plan.* + lane.dispatch + knowledge.*;
        // the three vendor categories name the agents' own tools.
        let names: Vec<String> = ["fs.read", "fs.write", "shell.exec"]
            .iter()
            .map(|n| (*n).to_string())
            .chain(ui_defs().into_iter().chain(session_defs()).map(|d| d.name))
            .collect();
        let catalog = builtin_tools();
        for n in &names {
            assert_eq!(
                catalog.iter().filter(|t| t.name == n.as_str()).count(),
                1,
                "catalog gap/dupe: {n}"
            );
        }
        assert_eq!(catalog.len(), names.len(), "catalog drift vs defs");
    }

    #[test]
    fn humanizer_names_the_interesting_arg() {
        let j = |s: &str| serde_json::from_str(s).unwrap();
        assert_eq!(
            humanize_tool_call("fs.read", &j(r#"{"path":"Cargo.toml"}"#)),
            "Reading Cargo.toml"
        );
        assert_eq!(
            humanize_tool_call("shell.exec", &j(r#"{"cmd":"cargo test -p parzi-core"}"#)),
            "Running `cargo test -p parzi-core`"
        );
        assert_eq!(
            humanize_tool_call(
                "session.spawn",
                &j(r#"{"title":"auth worker","prompt":"x"}"#)
            ),
            "Delegating: auth worker"
        );
        // Opaque connector tools degrade to name + hint, never empty.
        assert_eq!(
            humanize_tool_call("gh.issue_get", &j(r#"{"number":"12"}"#)),
            "Calling gh.issue_get 12"
        );
        assert_eq!(
            humanize_tool_call("weird.tool", &j("{}")),
            "Calling weird.tool"
        );
        // Status lines stay single-line.
        assert!(!humanize_tool_call("shell.exec", &j(r#"{"cmd":"a\nb"}"#)).contains('\n'));
    }
}

async fn execute_plan_tool(name: &str, args: &serde_json::Value) -> (bool, String) {
    let project = args.get("project").and_then(|v| v.as_str()).unwrap_or("");
    if project.trim().is_empty() {
        return (false, "plan tool needs `project`".into());
    }
    match name {
        "plan.read" => match parzi_core::plan::read_plan(project) {
            Ok(text) => {
                let cut: String = text.chars().take(24_000).collect();
                (true, cut)
            }
            Err(e) => (false, e.to_string()),
        },
        "plan.update" => {
            if let Some(append) = args
                .get("append")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
            {
                let mut raw = parzi_core::plan::read_plan(project).unwrap_or_default();
                if !raw.ends_with('\n') {
                    raw.push('\n');
                }
                raw.push_str(&format!("- [ ] {append}\n"));
                match parzi_core::plan::write_plan(project, &raw) {
                    Ok(()) => return (true, "appended".into()),
                    Err(e) => return (false, e.to_string()),
                }
            }
            let title = args
                .get("title_match")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if title.trim().is_empty() {
                return (false, "plan.update needs `title_match` or `append`".into());
            }
            let done = args.get("done").and_then(|v| v.as_bool()).unwrap_or(true);
            match parzi_core::plan::set_task_status(project, title, done) {
                Ok(true) => (true, "updated".into()),
                Ok(false) => (false, "no matching task".into()),
                Err(e) => (false, e.to_string()),
            }
        }
        _ => (false, format!("unknown plan tool `{name}`")),
    }
}

async fn execute_knowledge_tool(name: &str, args: &serde_json::Value) -> (bool, String) {
    let project = args.get("project").and_then(|v| v.as_str()).unwrap_or("");
    if project.trim().is_empty() {
        return (false, "knowledge tool needs `project`".into());
    }
    match name {
        "knowledge.read" => match parzi_core::lanes::read_knowledge(project) {
            Some(text) => {
                let cut: String = text.chars().take(24_000).collect();
                (true, cut)
            }
            None => (true, "no knowledge recorded yet for this project".into()),
        },
        "knowledge.record" => {
            let note = args.get("note").and_then(|v| v.as_str()).unwrap_or("");
            if note.trim().is_empty() {
                return (false, "knowledge.record needs `note`".into());
            }
            let category = args
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("decision");
            let formatted = format!("[{category}] {note}");
            match parzi_core::lanes::append_knowledge(project, &formatted) {
                Ok(()) => (true, "knowledge recorded".into()),
                Err(e) => (false, e.to_string()),
            }
        }
        _ => (false, format!("unknown knowledge tool `{name}`")),
    }
}
