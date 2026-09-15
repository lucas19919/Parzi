//! Unified tools: local Rust fns + MCP servers behind one executor.
//! Lane allowlists decide what runs. Approvals pause the handler, never the tool.

use parzi_core::error::{ParziError, Result};
use parzi_providers::ToolDef;

use crate::mcp::McpManager;

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
        if is_local(name)
            || is_ui_tool(name)
            || is_session_tool(name)
            || is_plan_tool(name)
            || is_lane_tool(name)
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
        let mut d = local_defs();
        d.extend(ui_defs());
        d.extend(session_defs());
        d
    }

    /// Local + always-on defs plus the currently-exposed MCP tools that also
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
                    d.push(t.to_provider_def());
                }
            }
        }
        d
    }

    pub async fn execute(&self, name: &str, args: &serde_json::Value) -> (bool, String) {
        if !self.is_allowed(name) {
            return (false, format!("tool `{name}` is not allowed in this lane"));
        }
        if is_plan_tool(name) {
            return execute_plan_tool(name, args).await;
        }
        if is_knowledge_tool(name) {
            return execute_knowledge_tool(name, args).await;
        }
        if let Some((server, tool)) = name.split_once('.').filter(|_| name.contains('.')) {
            // MCP names are `server.tool`; local names are `fs.read` style too,
            // so try local first, then MCP.
            if is_local(name) {
                return execute_local(name, args, &self.cwd).await;
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
        if is_local(name) {
            return execute_local(name, args, &self.cwd).await;
        }
        (false, format!("unknown tool `{name}`"))
    }
}

fn is_local(name: &str) -> bool {
    matches!(name, "fs.read" | "fs.write" | "fs.list" | "shell.exec")
}

pub fn local_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "fs.read".into(),
            description: "Read a text file relative to the lane cwd.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
            }),
        },
        ToolDef {
            name: "fs.write".into(),
            description: "Write (create/overwrite) a text file relative to the lane cwd.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                },
                "required": ["path", "content"],
            }),
        },
        ToolDef {
            name: "fs.list".into(),
            description: "List directory entries relative to the lane cwd.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
            }),
        },
        ToolDef {
            name: "shell.exec".into(),
            description: "Run a shell command in the lane cwd. Prefer fs.* for files.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cmd": {"type": "string"},
                    "timeout_ms": {"type": "number"},
                },
                "required": ["cmd"],
            }),
        },
    ]
}

/// Agent-facing UI tools. Handled by the handler (appends widget events),
/// declared here so every provider advertises the same surface.
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
            description: "Send a message to another session (child, parent, or peer) and optionally wait for its reply. The target session continues from the message like a new user turn.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"},
                    "message": {"type": "string"},
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
            blurb: "Read files in the project",
        },
        BuiltinTool {
            name: "fs.write",
            group: "Files",
            blurb: "Create and overwrite files",
        },
        BuiltinTool {
            name: "fs.list",
            group: "Files",
            blurb: "Browse directories",
        },
        BuiltinTool {
            name: "shell.exec",
            group: "Shell",
            blurb: "Run shell commands in the repo",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_every_advertised_tool_exactly_once() {
        // session_defs carries session.* + plan.* + lane.dispatch + knowledge.*.
        let names: Vec<String> = local_defs()
            .into_iter()
            .chain(ui_defs())
            .chain(session_defs())
            .map(|d| d.name)
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
}

/// Resolve `path` strictly inside `cwd`. Rejects absolute / rooted / drive-
/// relative / UNC / verbatim paths, `..` escapes, empty cwd, and symlinked
/// escapes via canonicalization. Used by every fs.* tool.
fn resolve(cwd: &str, path: &str) -> Result<std::path::PathBuf> {
    use std::path::{Component, Path};
    if cwd.trim().is_empty() {
        return Err(ParziError::Tool("fs".into(), "lane cwd is not set".into()));
    }
    let req = Path::new(path);
    // Any absolute / prefix / root component in the *request* is rejected
    // outright: `base.join(absolute)` would discard the base on all platforms.
    for c in req.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => {
                return Err(ParziError::Tool(
                    "fs".into(),
                    format!("absolute path not allowed: {path}"),
                ));
            }
            _ => {}
        }
    }
    // Windows drive-relative (`C:foo`) and verbatim (`\\?\`) arrive as
    // Normal + Prefix or are caught above; also reject explicit UNC prefixes,
    // backslash-rooted, and NUL bytes that confuse later joins.
    if path.contains('\0')
        || path.starts_with(r"\\")
        || path.starts_with("//")
        || path.starts_with(r"\")
        || path.starts_with('/')
    {
        return Err(ParziError::Tool(
            "fs".into(),
            format!("absolute path not allowed: {path}"),
        ));
    }
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        return Err(ParziError::Tool(
            "fs".into(),
            format!("absolute path not allowed: {path}"),
        ));
    }
    // Lexical containment counted over the *request* only: `..` must never
    // climb above the lane root, no matter how deep the base is.
    let mut depth = 0i32;
    for c in req.components() {
        match c {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(ParziError::Tool(
                        "fs".into(),
                        format!("path escapes lane root: {path}"),
                    ));
                }
            }
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            _ => {}
        }
    }
    let base = std::path::PathBuf::from(cwd);
    let joined = base.join(req);
    // Canonicalize to catch symlink / junction escapes. For not-yet-existing
    // targets (fs.write creates parents), walk up to the nearest existing
    // ancestor and require it to stay under the canonical root.
    let canon_base = base.canonicalize().unwrap_or(base.clone());
    let mut probe: Option<&Path> = Some(&joined);
    let mut canon_probe: Option<std::path::PathBuf> = None;
    while let Some(p) = probe {
        if let Ok(c) = p.canonicalize() {
            canon_probe = Some(c);
            break;
        }
        probe = p.parent();
    }
    if let Some(canon) = canon_probe {
        if !canon.starts_with(&canon_base) {
            return Err(ParziError::Tool(
                "fs".into(),
                format!("path escapes lane root: {path}"),
            ));
        }
    } else {
        // Nothing on disk canonicalizes (fresh tree): fall back to the
        // lexical guarantee above, which already rejected every escape.
    }
    Ok(joined)
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

async fn execute_local(name: &str, args: &serde_json::Value, cwd: &str) -> (bool, String) {
    match name {
        "fs.read" => {
            let p = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
            match resolve(cwd, p) {
                Ok(full) => match tokio::fs::read_to_string(&full).await {
                    Ok(t) => {
                        let cut: String = t.chars().take(24_000).collect();
                        (true, cut)
                    }
                    Err(e) => (false, format!("read failed: {e}")),
                },
                Err(e) => (false, e.to_string()),
            }
        }
        "fs.write" => {
            let p = args.get("path").and_then(|p| p.as_str()).unwrap_or("");
            let content = args.get("content").and_then(|c| c.as_str()).unwrap_or("");
            match resolve(cwd, p) {
                Ok(full) => {
                    if let Some(parent) = full.parent() {
                        if let Err(e) = tokio::fs::create_dir_all(parent).await {
                            return (false, format!("mkdir failed: {e}"));
                        }
                    }
                    match tokio::fs::write(&full, content).await {
                        Ok(()) => (true, format!("wrote {} bytes", content.len())),
                        Err(e) => (false, format!("write failed: {e}")),
                    }
                }
                Err(e) => (false, e.to_string()),
            }
        }
        "fs.list" => {
            let p = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");
            match resolve(cwd, p) {
                Ok(full) => match tokio::fs::read_dir(&full).await {
                    Ok(mut rd) => {
                        let mut out = String::new();
                        while let Ok(Some(e)) = rd.next_entry().await {
                            let ft = e
                                .file_type()
                                .await
                                .map(|t| if t.is_dir() { "d" } else { "f" })
                                .unwrap_or("?");
                            out.push_str(&format!("{ft} {}\n", e.file_name().to_string_lossy()));
                            if out.len() > 12_000 {
                                out.push_str("…(truncated)\n");
                                break;
                            }
                        }
                        (true, out)
                    }
                    Err(e) => (false, format!("list failed: {e}")),
                },
                Err(e) => (false, e.to_string()),
            }
        }
        "shell.exec" => {
            let cmd = args.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
            if cmd.trim().is_empty() {
                return (false, "empty command".into());
            }
            if cwd.trim().is_empty() {
                return (false, "lane cwd is not set".into());
            }
            let timeout_ms = args
                .get("timeout_ms")
                .and_then(|n| n.as_u64())
                .unwrap_or(30_000)
                .min(120_000);
            let mut c = if cfg!(windows) {
                let mut c = tokio::process::Command::new("cmd");
                c.arg("/C").arg(cmd);
                c
            } else {
                let mut c = tokio::process::Command::new("sh");
                c.arg("-c").arg(cmd);
                c
            };
            c.current_dir(cwd);
            // H-6: children never inherit provider keys or other secrets.
            c.env_clear();
            for (k, v) in [
                ("PATH", std::env::var("PATH").unwrap_or_default()),
                (
                    "SYSTEMROOT",
                    std::env::var("SYSTEMROOT").unwrap_or_default(),
                ),
                ("TEMP", std::env::var("TEMP").unwrap_or_default()),
                ("TMP", std::env::var("TMP").unwrap_or_default()),
                ("HOME", std::env::var("HOME").unwrap_or_default()),
                ("APPDATA", std::env::var("APPDATA").unwrap_or_default()),
                (
                    "USERPROFILE",
                    std::env::var("USERPROFILE").unwrap_or_default(),
                ),
                ("LANG", std::env::var("LANG").unwrap_or_default()),
                ("LC_ALL", std::env::var("LC_ALL").unwrap_or_default()),
            ] {
                if !v.is_empty() {
                    c.env(k, v);
                }
            }
            c.stdin(std::process::Stdio::null());
            c.kill_on_drop(true);
            c.stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), c.output())
                .await
            {
                Ok(Ok(o)) => {
                    let mut s = String::from_utf8_lossy(&o.stdout).to_string();
                    if !o.status.success() {
                        s.push_str(&String::from_utf8_lossy(&o.stderr));
                    }
                    let cut: String = s.chars().take(8_000).collect();
                    (o.status.success(), cut)
                }
                Ok(Err(e)) => (false, format!("spawn failed: {e}")),
                Err(_) => (false, format!("timed out after {timeout_ms}ms")),
            }
        }
        _ => (false, format!("unknown local tool `{name}`")),
    }
}
