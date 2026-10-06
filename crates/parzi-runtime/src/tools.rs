use crate::mcp::McpManager;

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
        match s.trim() {
            "auto" | "full" => Self::Auto,
            "deny" => Self::Deny,
            _ => Self::Ask,
        }
    }

    pub fn is_full_override(s: Option<&str>) -> bool {
        matches!(s, Some(o) if o.trim() == "full")
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
    pub lane: String,
    pub session: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    Allow,
    Deny,
}

#[async_trait::async_trait]
pub trait Approver: Send + Sync {
    async fn approve(&self, call: &ToolCallInfo) -> Approval;
}

pub struct AutoApprover;
pub(crate) struct DenyApprover;

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
    pub fn is_allowed(&self, name: &str) -> bool {
        self.allowed.iter().any(|p| {
            p == "*" || p == name || (p.ends_with(".*") && name.starts_with(&p[..p.len() - 1]))
        })
    }

    pub(crate) fn approval_override(&self, name: &str) -> Option<ApprovalMode> {
        if is_vendor_category(name)
            || is_ui_tool(name)
            || is_brain_tool(name)
            || is_session_tool(name)
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
        let mut d = ui_defs();
        d.extend(browser_defs());
        d.extend(brain_defs());
        d
    }

    pub async fn execute(&self, name: &str, args: &serde_json::Value) -> (bool, String) {
        if !self.is_allowed(name) {
            return (false, format!("tool `{name}` is not allowed in this lane"));
        }
        if let Some((server, tool)) = name.split_once('.') {
            if is_vendor_category(name) {
                return (
                    false,
                    format!("`{name}` is the agent's own tool now, not Parzi's"),
                );
            }
            if is_ui_tool(name)
                || is_brain_tool(name)
                || is_session_tool(name)
                || is_lane_tool(name)
            {
                return (false, format!("tool `{name}` is handled by the agent loop"));
            }
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

pub(crate) fn is_vendor_category(name: &str) -> bool {
    matches!(name, "fs.read" | "fs.write" | "fs.list" | "shell.exec")
}

fn ui_defs() -> Vec<ToolDef> {
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
            name: "ui.show_artifact".into(),
            description: "Save/update a versioned artifact card with copy and save actions. html and svg render live in a sandboxed frame with no network access (inline everything; images only as data: URIs), with a show-source toggle. Use for code over ~15 lines, full files, markdown docs, html/svg previews, json/csv data, diffs. Diagrams (architecture, flow, sequence) go here as an svg artifact, or html when they need layout or interactivity. Kinds: code|markdown|html|svg|json|csv|diff|text. Languages: rust|typescript|javascript|python|toml|json|bash|sh|diff|markdown|md|html|css. Reuse the same id to bump the version. Example: {\"artifact\":1,\"id\":\"auth-middleware\",\"title\":\"Auth middleware\",\"kind\":\"code\",\"language\":\"typescript\",\"content\":\"...\"}.".into(),
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

pub(crate) fn is_ui_tool(name: &str) -> bool {
    matches!(
        name,
        "ui.show_markdown" | "ui.show_widget" | "ui.show_artifact"
    )
}

fn browser_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "browser.open".into(),
            description: "Open a URL in a browser tab, or focus that tab if the URL is already open. A bare host is https. A phrase is a search.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"url": {"type": "string"}},
                "required": ["url"],
            }),
        },
        ToolDef {
            name: "browser.tabs".into(),
            description: "List the open tabs: id, title, and URL.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {},
            }),
        },
        ToolDef {
            name: "browser.read".into(),
            description: "Read the session's browser tab: title, URL, page text, and clickable controls. Needs a tab opened with browser.open first.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {},
            }),
        },
        ToolDef {
            name: "browser.click".into(),
            description: "Click a control on the session's browser tab. Pick it by its visible text, or by a CSS selector.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "selector": {"type": "string"},
                },
            }),
        },
        ToolDef {
            name: "browser.type".into(),
            description: "Type into a field on the session's browser tab, optionally submitting (Enter + form submit). Pick the field by its label, or by a CSS selector.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "field": {"type": "string"},
                    "selector": {"type": "string"},
                    "submit": {"type": "boolean"},
                },
                "required": ["text"],
            }),
        },
    ]
}

pub(crate) fn is_browser_tool(name: &str) -> bool {
    matches!(
        name,
        "browser.open" | "browser.tabs" | "browser.read" | "browser.click" | "browser.type"
    )
}

pub fn brain_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "brain.search".into(),
            description: "Search the brain: the user's own notes vault, an Obsidian-style folder of markdown notes about their projects, decisions, and know-how. Matches titles and text, case-insensitive, and returns up to 20 notes with a snippet each. Notes for this session's project and notes for every session are already in your instructions: pinned ones in full, the rest listed with a one-line summary. Search for anything else the user may have written down before asking them.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"],
            }),
        },
        ToolDef {
            name: "brain.read".into(),
            description: "Read one note from the user's notes vault (the brain) by its path relative to the vault, e.g. projects/parzi.md, as given by brain.search, brain.list, or the on-demand notes listed in your instructions. Returns the raw markdown, frontmatter included.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
            }),
        },
        ToolDef {
            name: "brain.list".into(),
            description: "List the user's notes vault (the brain): projects with the folders they map to, and each note's path, title, one-line summary, and whether it is pinned (pinned notes are attached to sessions in full). Pass project (a project slug, or all for the notes attached to every session) to list only those notes.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"project": {"type": "string"}},
            }),
        },
        ToolDef {
            name: "brain.write".into(),
            description: "Create or replace a note in the user's notes vault (the brain). path is relative to the vault and ends in .md; content is the whole markdown file. Write durable learnings back here (decisions, conventions, gotchas, how the code fits together) so later sessions start with them. To attach a note to a project, give it frontmatter `projects: [slug]` or link the project note as [[slug]]; `projects: [all]` attaches it to every session. Start the body with a one-line summary sentence (or set `description:` in the frontmatter): later sessions see that line before deciding to read the note. Read a note before replacing it and keep its frontmatter. The user may be asked to approve the write.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                },
                "required": ["path", "content"],
            }),
        },
    ]
}

pub fn is_brain_tool(name: &str) -> bool {
    matches!(
        name,
        "brain.search" | "brain.read" | "brain.list" | "brain.write"
    )
}

pub(crate) fn is_lane_tool(name: &str) -> bool {
    matches!(name, "lane.dispatch")
}

pub fn is_session_tool(name: &str) -> bool {
    matches!(
        name,
        "session.spawn" | "session.send_message" | "session.read_session" | "session.list_sessions"
    )
}

pub(crate) fn humanize_tool_call(name: &str, args: &serde_json::Value) -> String {
    let str_arg = |k: &str| {
        args.get(k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let hint = [
        "path", "file", "cmd", "query", "prompt", "title", "url", "message", "note", "number", "id",
    ]
    .iter()
    .find_map(|k| str_arg(k));
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
        "Bash" | "shell" | "shell.exec" => format!("Running `{}`", one_line(&command(), 60)),
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
        "lane.dispatch" => format!(
            "Dispatching worker: {}",
            one_line(&str_arg("title").unwrap_or_else(|| "worker".into()), 60)
        ),
        "browser.open" => format!(
            "Opening {}",
            one_line(&str_arg("url").unwrap_or_else(|| "a page".into()), 60)
        ),
        "browser.tabs" => "Listing tabs".into(),
        "browser.read" => "Reading the open page".into(),
        "browser.click" => format!(
            "Clicking {}",
            one_line(
                &str_arg("text")
                    .or_else(|| str_arg("selector"))
                    .unwrap_or_else(|| "a control".into()),
                60
            )
        ),
        "browser.type" => format!(
            "Typing into {}",
            one_line(
                &str_arg("field")
                    .or_else(|| str_arg("selector"))
                    .unwrap_or_else(|| "a field".into()),
                60
            )
        ),
        "brain.search" => format!(
            "Searching notes for {}",
            one_line(&str_arg("query").unwrap_or_default(), 60)
        ),
        "brain.read" => format!(
            "Reading note {}",
            one_line(&str_arg("path").unwrap_or_else(|| "?".into()), 60)
        ),
        "brain.list" => match str_arg("project") {
            Some(p) => format!("Listing notes for {}", one_line(&p, 60)),
            None => "Listing notes".into(),
        },
        "brain.write" => format!(
            "Writing note {}",
            one_line(&str_arg("path").unwrap_or_else(|| "?".into()), 60)
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

const NAMESPACES: &[&str] = &["ui", "browser", "brain", "session", "lane"];

pub fn to_mcp(name: &str) -> String {
    name.replace('.', "_")
}

pub(crate) fn from_mcp(tool: &str) -> String {
    for ns in NAMESPACES {
        if let Some(rest) = tool.strip_prefix(ns).and_then(|r| r.strip_prefix('_')) {
            return format!("{ns}.{rest}");
        }
    }
    tool.to_string()
}

pub fn display_name(name: &str) -> String {
    name.strip_prefix("mcp__parzi__")
        .or_else(|| name.strip_prefix("parzi."))
        .or_else(|| name.strip_prefix("parzi_"))
        .map_or_else(|| name.to_string(), from_mcp)
}

fn one_line(s: &str, n: usize) -> String {
    let first = s.lines().next().unwrap_or("").trim();
    let cut: String = first.chars().take(n).collect();
    if first.chars().count() > n {
        format!("{cut}…")
    } else {
        cut
    }
}

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
        assert_eq!(
            humanize_tool_call("gh.issue_get", &j(r#"{"number":"12"}"#)),
            "Calling gh.issue_get 12"
        );
        assert_eq!(
            humanize_tool_call("weird.tool", &j("{}")),
            "Calling weird.tool"
        );
        assert!(!humanize_tool_call("shell.exec", &j(r#"{"cmd":"a\nb"}"#)).contains('\n'));
    }
}
