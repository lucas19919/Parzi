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

    pub fn defs(&self) -> Vec<ToolDef> {
        let mut d = local_defs();
        d.extend(ui_defs());
        d.extend(session_defs());
        d
    }

    pub async fn execute(&self, name: &str, args: &serde_json::Value) -> (bool, String) {
        if !self.is_allowed(name) {
            return (false, format!("tool `{name}` is not allowed in this lane"));
        }
        if let Some((server, tool)) = name.split_once('.').filter(|_| name.contains('.')) {
            // MCP names are `server.tool`; local names are `fs.read` style too,
            // so try local first, then MCP.
            if is_local(name) {
                return execute_local(name, args, &self.cwd).await;
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
    ]
}

pub fn is_session_tool(name: &str) -> bool {
    matches!(
        name,
        "session.spawn" | "session.send_message" | "session.read_session" | "session.list_sessions"
    )
}

/// Resolve `path` inside `cwd` (when set). Rejects escapes beyond the root.
fn resolve(cwd: &str, path: &str) -> Result<std::path::PathBuf> {
    let base = if cwd.is_empty() {
        std::env::current_dir().map_err(ParziError::Io)?
    } else {
        std::path::PathBuf::from(cwd)
    };
    let joined = base.join(path);
    // Lexical containment: `a/b/../../x` must not escape `a`.
    let mut depth = 0i32;
    for c in joined.components() {
        match c {
            std::path::Component::ParentDir => depth -= 1,
            std::path::Component::Normal(_) => depth += 1,
            _ => {}
        }
        if depth < 0 {
            return Err(ParziError::Tool(
                "fs".into(),
                format!("path escapes lane root: {path}"),
            ));
        }
    }
    Ok(joined)
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
                            let ft = e.file_type().await.map(|t| if t.is_dir() { "d" } else { "f" }).unwrap_or("?");
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
            if !cwd.is_empty() {
                c.current_dir(cwd);
            }
            c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
            match tokio::time::timeout(
                std::time::Duration::from_millis(timeout_ms),
                c.output(),
            )
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
