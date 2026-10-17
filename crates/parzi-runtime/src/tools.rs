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

#[derive(Debug, Clone)]
pub struct AskRequest {
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
    /// Empty for questions; otherwise the request kind ("text", "file",
    /// "image"). The UI shows both inline, in the asking session.
    pub kind: String,
    pub lane: String,
    pub session: String,
}

#[async_trait::async_trait]
pub trait Asker: Send + Sync {
    async fn ask(&self, req: &AskRequest) -> String;
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
        // Connector modes govern connector tools only: a connector named
        // `shell` or `project` must not relax Parzi's own gates.
        if is_vendor_category(name) || is_parzi_tool(name) {
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
        let mut d = all_defs();
        d.retain(|t| self.is_allowed(&t.name));
        d
    }

    pub async fn execute(&self, name: &str, args: &serde_json::Value) -> (bool, String) {
        if !self.is_allowed(name) {
            return (false, format!("tool `{name}` is not allowed in this lane"));
        }
        if let Some((server, tool)) = name.split_once('.') {
            if is_vendor_category(name) {
                return (false, format!("tool `{name}` is not allowed in this lane"));
            }
            if is_ui_tool(name)
                || is_brain_tool(name)
                || is_session_tool(name)
                || is_lane_tool(name)
                || is_plan_tool(name)
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

/// The vendor category of an agent's own tool: shell, write or read.
pub(crate) fn category(tool: &str) -> Option<&'static str> {
    match tool {
        "Bash" | "BashOutput" | "KillShell" | "shell" | "execute" => Some("shell.exec"),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "edit" | "delete" | "move" => {
            Some("fs.write")
        }
        "Read" | "Glob" | "Grep" | "LS" | "NotebookRead" | "read" | "search" => Some("fs.read"),
        _ => {
            // The vendor shell list can never be complete: any tool whose
            // name smells like command execution routes to the shell gate
            // instead of slipping through as unknown.
            let l = tool.to_lowercase();
            // ACP agents name a call by its title ("Run create_file?",
            // "Running edit_file" from Antigravity): the file verb in it says
            // it writes.
            if FILE_WRITES.iter().any(|w| l.contains(w)) {
                return Some("fs.write");
            }
            if l.contains("shell")
                || l.contains("bash")
                || l.contains("terminal")
                || l.contains("powershell")
                || matches!(
                    l.as_str(),
                    "cmd" | "exec" | "execute" | "command" | "run_command"
                )
            {
                Some("shell.exec")
            } else {
                None
            }
        }
    }
}

/// File-writing tool names inside an ACP call's title.
const FILE_WRITES: &[&str] = &[
    "create_file",
    "edit_file",
    "write_file",
    "write_to_file",
    "replace_file",
    "multi_replace_file",
];

/// The files an edit tool call names: Claude's `file_path` (and
/// `notebook_path`), Codex's `paths`, Antigravity's `TargetFile`, and the
/// usual spellings of others.
#[must_use]
pub fn edit_paths(tool: &str, args: &serde_json::Value) -> Vec<String> {
    if category(tool) != Some("fs.write") {
        return vec![];
    }
    let mut out: Vec<String> = vec![];
    for key in [
        "file_path",
        "notebook_path",
        "path",
        "filePath",
        "file",
        "TargetFile",
        "target_file",
        "targetFile",
    ] {
        if let Some(p) = args.get(key).and_then(serde_json::Value::as_str) {
            out.push(p.to_string());
        }
    }
    for key in ["paths", "files"] {
        if let Some(list) = args.get(key).and_then(serde_json::Value::as_array) {
            out.extend(
                list.iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string),
            );
        }
    }
    out.retain(|p| !p.trim().is_empty());
    out.dedup();
    out
}

/// The command line a shell tool call runs, Parzi's or an agent's own.
#[must_use]
pub fn shell_command(tool: &str, args: &serde_json::Value) -> Option<String> {
    if !matches!(tool, "shell.exec" | "shell.start") && category(tool) != Some("shell.exec") {
        return None;
    }
    let cmd = match args.get("command").or_else(|| args.get("cmd")) {
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        Some(v) => v.as_str().unwrap_or_default().to_string(),
        None => String::new(),
    };
    Some(cmd.trim().to_string()).filter(|c| !c.is_empty())
}

/// Every tool Parzi itself offers. Used to tell lane-allowlist entries
/// that name our own tools apart from entries that speak in vendor
/// categories (e.g. "fs.*") — "shell.exec" is unfortunately both.
pub(crate) fn is_parzi_tool(name: &str) -> bool {
    is_ui_tool(name)
        || is_browser_tool(name)
        || is_desk_tool(name)
        || is_session_tool(name)
        || is_lane_tool(name)
        || is_image_tool(name)
        || is_doc_tool(name)
        || is_models_tool(name)
        || is_question_tool(name)
        || is_project_tool(name)
        || is_shell_tool(name)
        || is_brain_tool(name)
        || is_plan_tool(name)
}

/// Every one of Parzi's own tool definitions, before a lane filters them.
pub fn all_defs() -> Vec<ToolDef> {
    let mut d = ui_defs();
    d.extend(browser_defs());
    d.extend(desk_defs());
    d.extend(brain_defs());
    d.extend(session_defs());
    d.extend(lane_defs());
    d.extend(image_defs());
    d.extend(doc_defs());
    d.extend(team_defs());
    d.extend(question_defs());
    d.extend(plan_defs());
    d.extend(project_defs());
    d.extend(shell_defs());
    d
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
            description: "A versioned artifact card with copy/save. Use for code over ~15 lines, files, docs, data, diffs, diagrams (svg, or html for layout). Contract: FIRST say in chat text what the artifact is in one or two sentences (that streams as your reply), THEN make exactly one call with the complete content. Never paste the content into chat as well, and never call with half the content planning to finish it later: an incomplete call fails validation and the person sees an error card. Reuse an id for new versions of the same thing. html/svg render live sandboxed with no network: inline everything, images as data: URIs. Kinds: code|markdown|html|svg|json|csv|diff|text|preview. preview renders a live component inline from JSON like {\"component\":\"omnibar\",\"variant\":2} (omnibar only, variant 1-7 integer, live ships 2; anything else errors). The artifact IS the delivery: never send the user to a tab or window. Example: {\"artifact\":1,\"id\":\"auth-middleware\",\"title\":\"Auth middleware\",\"kind\":\"code\",\"language\":\"typescript\",\"content\":\"...\"}.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "artifact": {"type": "number", "const": 1},
                    "id": {"type": "string", "description": "slug [a-z0-9-], reused across versions"},
                    "title": {"type": "string"},
                    "kind": {"type": "string", "enum": ["code","markdown","html","svg","json","csv","diff","text","preview"]},
                    "language": {"type": "string"},
                    "content": {"type": "string"},
                },
                "required": ["artifact", "content"],
            }),
        },
    ]
}

pub(crate) fn is_ui_tool(name: &str) -> bool {
    matches!(name, "ui.show_markdown" | "ui.show_artifact")
}

/// The optional `tab` every page tool takes: one of this session's tab ids
/// from browser.open. Without it the tool works on the tab used last.
fn tab_arg() -> serde_json::Value {
    serde_json::json!({"type": "string", "description": "a tab id from browser.open; default: the tab you used last"})
}

fn browser_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "browser.open".into(),
            description: "Open a URL in one of your page tabs and return its tab id. Without tab or new, your last tab navigates (a first call opens one). new=true opens another tab; tab=<id> navigates that one. Tabs open in the background; focus=true also shows the tab to the human. A bare host is https, a phrase is a search.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"},
                    "new": {"type": "boolean"},
                    "tab": tab_arg(),
                    "focus": {"type": "boolean"},
                },
                "required": ["url"],
            }),
        },
        ToolDef {
            name: "browser.tabs".into(),
            description: "List your page tabs (id, title, URL) and the title of the tab the human is looking at. all=true asks the human first and lists every tab with its URL.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"all": {"type": "boolean"}},
            }),
        },
        ToolDef {
            name: "browser.read".into(),
            description: "Read a page tab: title, URL, text (same-origin frames included) and the controls you can click or type into. Text comes in pages: offset and limit (default 4000, max 20000 chars); the reply says where the next page starts. tab=active reads the human's own current page, which they must allow.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tab": tab_arg(),
                    "offset": {"type": "number"},
                    "limit": {"type": "number"},
                },
            }),
        },
        ToolDef {
            name: "browser.click".into(),
            description: "Click a control on a page tab, picked by its visible text or a CSS selector.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "selector": {"type": "string"},
                    "tab": tab_arg(),
                },
            }),
        },
        ToolDef {
            name: "browser.type".into(),
            description: "Type into a field on a page tab (inputs, text areas and rich editors), picked by its label or a CSS selector; default: the focused field. Replaces the text unless append=true. submit=true then presses Enter once.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "field": {"type": "string"},
                    "selector": {"type": "string"},
                    "append": {"type": "boolean"},
                    "submit": {"type": "boolean"},
                    "tab": tab_arg(),
                },
                "required": ["text"],
            }),
        },
        ToolDef {
            name: "browser.press".into(),
            description: "Press a key on a page tab: Enter, Tab, Escape, Backspace, Delete, Space, ArrowUp/Down/Left/Right, Home, End, PageUp, PageDown, a single character, or a chord like ctrl+a. Goes to the element a selector picks, else the focused one.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "key": {"type": "string"},
                    "selector": {"type": "string"},
                    "tab": tab_arg(),
                },
                "required": ["key"],
            }),
        },
        ToolDef {
            name: "browser.select".into(),
            description: "Choose an option in a dropdown (a select element) on a page tab, by the option's text or value. Pick the dropdown by its label or a CSS selector.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "option": {"type": "string"},
                    "field": {"type": "string"},
                    "selector": {"type": "string"},
                    "tab": tab_arg(),
                },
                "required": ["option"],
            }),
        },
        ToolDef {
            name: "browser.scroll".into(),
            description: "Scroll a page tab: to is down (default), up, top or bottom; or bring the element a CSS selector picks into view.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "to": {"type": "string", "enum": ["down", "up", "top", "bottom"]},
                    "selector": {"type": "string"},
                    "tab": tab_arg(),
                },
            }),
        },
        ToolDef {
            name: "browser.wait".into(),
            description: "Wait until a CSS selector matches a visible element, or a text appears on the page (gone=true: until it disappears). timeout_ms defaults to 10000, at most 60000.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "selector": {"type": "string"},
                    "text": {"type": "string"},
                    "gone": {"type": "boolean"},
                    "timeout_ms": {"type": "number"},
                    "tab": tab_arg(),
                },
            }),
        },
        ToolDef {
            name: "browser.shot".into(),
            description: "Screenshot a page tab as a JPEG under ~/.parzi/shots. Returns the file path: open it with your own vision (Read) to see the pixels and verify layouts, diagrams, and rendered pages.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"tab": tab_arg()},
            }),
        },
        ToolDef {
            name: "browser.close".into(),
            description: "Close one of your page tabs.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"tab": tab_arg()},
                "required": ["tab"],
            }),
        },
    ]
}

pub(crate) fn is_browser_tool(name: &str) -> bool {
    matches!(
        name,
        "browser.open"
            | "browser.tabs"
            | "browser.read"
            | "browser.click"
            | "browser.type"
            | "browser.press"
            | "browser.select"
            | "browser.scroll"
            | "browser.wait"
            | "browser.shot"
            | "browser.close"
    )
}

fn desk_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "desk.state".into(),
            description: "What the Parzi window shows: your own page tabs, the title of the tab the human is on, whether the window has focus, the sessions running now, and the folder and project of the human's current tab.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {},
            }),
        },
        ToolDef {
            name: "desk.open".into(),
            description: "Open something in the Parzi window for the human: what=session (id; default this session), settings (section: general, providers, connections, appearance, system), brain (path of a note), history (view: sessions, history, bookmarks or downloads), or home. Opens in the background unless focus=true.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "what": {"type": "string", "enum": ["session", "settings", "brain", "history", "home"]},
                    "id": {"type": "string"},
                    "section": {"type": "string"},
                    "path": {"type": "string"},
                    "view": {"type": "string", "enum": ["sessions", "history", "bookmarks", "downloads"]},
                    "focus": {"type": "boolean"},
                },
                "required": ["what"],
            }),
        },
        ToolDef {
            name: "desk.notify".into(),
            description: "Tell the human something with a notification in the Parzi window; when the window is in the background its taskbar button flashes too. Keep it to one short line.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                },
                "required": ["text"],
            }),
        },
        ToolDef {
            name: "desk.screenshot".into(),
            description: "Screenshot the Parzi window itself (its tabs, threads and panels; page tabs show as blank) as a JPEG under ~/.parzi/shots. Returns the path to open with your own vision.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {},
            }),
        },
    ]
}

pub(crate) fn is_desk_tool(name: &str) -> bool {
    matches!(
        name,
        "desk.state" | "desk.open" | "desk.notify" | "desk.screenshot"
    )
}

/// Every desk and page tool name, for lane allowlists.
pub const DESK_TOOLS: &[&str] = &[
    "browser.open",
    "browser.tabs",
    "browser.read",
    "browser.click",
    "browser.type",
    "browser.press",
    "browser.select",
    "browser.scroll",
    "browser.wait",
    "browser.shot",
    "browser.close",
    "desk.state",
    "desk.open",
    "desk.notify",
    "desk.screenshot",
];

/// Desk tools need a window in this process; page tools also need the
/// Windows page engine. `parzi serve`, a CLI run with no window, a server
/// session and macOS get neither.
#[must_use]
pub fn window_tool_offered(name: &str, attached: bool, pages: bool) -> bool {
    if is_browser_tool(name) {
        pages
    } else if is_desk_tool(name) {
        attached && (name != "desk.screenshot" || pages)
    } else {
        true
    }
}

pub fn brain_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "brain.search".into(),
            description: "Search the user's notes vault by title and text. A note matches when it holds every word of the query; title matches rank first. Returns up to 20 notes with a snippet each.".into(),
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
            description: "List the user's notes vault (the brain): projects with the folders they map to, and each note's path, title, one-line summary, and whether it is pinned (pinned notes are attached to sessions in full). Pass project (a project slug, or all for the notes attached to every session) to list only those notes. Shows at most 50 notes; use brain.search for the rest.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"project": {"type": "string"}},
            }),
        },
        ToolDef {
            name: "brain.write".into(),
            description: "Create or replace a vault note (path relative to the vault, ends .md; content is the whole file). To attach it to a project: frontmatter `projects: [slug]` or a [[slug]] link; `[all]` attaches it everywhere. Start the body with a one-line summary. Read a note before replacing it and keep its frontmatter.".into(),
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
            name: "brain.delete".into(),
            description: "Delete a note from the user's notes vault (path relative to the vault, as given by brain.search or brain.list). Use it for notes that are wrong or obsolete; a project goes with project.delete.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
            }),
        },
    ]
}

pub fn is_brain_tool(name: &str) -> bool {
    matches!(
        name,
        "brain.search" | "brain.read" | "brain.list" | "brain.write" | "brain.delete"
    )
}

pub(crate) fn is_lane_tool(name: &str) -> bool {
    matches!(name, "lane.dispatch" | "lane.status")
}

pub(crate) fn is_image_tool(name: &str) -> bool {
    matches!(name, "image.generate")
}

pub(crate) fn is_doc_tool(name: &str) -> bool {
    matches!(name, "doc.read")
}

pub(crate) fn is_models_tool(name: &str) -> bool {
    matches!(name, "models.list")
}

pub(crate) fn is_shell_tool(name: &str) -> bool {
    matches!(
        name,
        "shell.exec" | "shell.start" | "shell.logs" | "shell.kill"
    )
}

fn shell_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "shell.exec".into(),
            description: "Run a shell command and wait for it (default 120s, timeout_ms up to 600000). Output is capped; the rest spills to a file whose path comes back. Pass workdir instead of cd. Stdin is closed: non-interactive flags only.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cmd": {"type": "string"},
                    "workdir": {"type": "string"},
                    "timeout_ms": {"type": "number"},
                },
                "required": ["cmd"],
            }),
        },
        ToolDef {
            name: "shell.start".into(),
            description: "Start a long-running command (dev server, watcher, build) in the background. Returns a shell id immediately; poll with shell.logs, stop with shell.kill.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cmd": {"type": "string"},
                    "workdir": {"type": "string"},
                },
                "required": ["cmd"],
            }),
        },
        ToolDef {
            name: "shell.logs".into(),
            description: "Read new output from a background shell. Pass back next_offset to walk forward.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "offset": {"type": "number"},
                    "tail": {"type": "number"},
                },
                "required": ["id"],
            }),
        },
        ToolDef {
            name: "shell.kill".into(),
            description: "Kill a background shell and its whole process tree.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"id": {"type": "string"}},
                "required": ["id"],
            }),
        },
    ]
}

pub(crate) fn is_question_tool(name: &str) -> bool {
    matches!(name, "ask.user" | "request.user" | "memory.review")
}

pub(crate) fn is_project_tool(name: &str) -> bool {
    matches!(
        name,
        "project.create" | "project.archive" | "project.delete"
    )
}

fn question_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "ask.user".into(),
            description: "Ask the user a question mid-turn and wait for their answer. Use it at real forks instead of guessing: which approach, which scope, proceed/stop. Keep options short; the user may also type free text.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "question": {"type": "string"},
                    "options": {"type": "array", "items": {"type": "string"}},
                },
                "required": ["question"],
            }),
        },
        ToolDef {
            name: "memory.review".into(),
            description: "Flag a stored note back to the user for re-confirmation (a founder decision that may be stale, a convention to keep or drop). Shows the note and asks Keep or Remove; Remove deletes it. Use when memory says something the current work contradicts.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
            }),
        },
        ToolDef {
            name: "request.user".into(),
            description: "Ask the human to give or do something mid-turn and wait for their reply: send a picture, paste inspiration or text, drop a file path, confirm something only they can see. It holds the turn until they answer, so reserve it for real needs, not forks (those are ask.user). Set kind to file when you need a file, image when you need a picture, else text.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "request": {"type": "string"},
                    "kind": {"type": "string", "enum": ["text", "file", "image"]},
                },
                "required": ["request"],
            }),
        },
    ]
}

/// The project's plan: read it, change it. Neither shows an approval card:
/// the plan is Parzi's own note, not the person's files (a read-only lane
/// still only reads it).
pub fn is_plan_tool(name: &str) -> bool {
    matches!(name, "plan.read" | "plan.write")
}

fn plan_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "plan.read".into(),
            description: "Read a project's plan: its items with their state, needs, branch, detail and note, and the sessions working on each (with their branches). project is a project slug; default: this session's project. Read-only.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"project": {"type": "string"}},
            }),
        },
        ToolDef {
            name: "plan.write".into(),
            description: "Change this project's plan. Every field is optional and what you leave out stays: title, goal, phases (ordered names such as Now and Next), and items, merged by id: [{id, title?, detail?, phase?, needs?, state?, branch?, note?}]. An id is 1 to 24 of a-z, 0-9 and -; a new item needs a title. needs names other items of the plan, never in a circle. state is planned, active, done or dropped; marking an item active links this session to it. Limits: 40 items, title 80 chars, detail and note 400. remove: true on an item takes out a mistake (finished or abandoned work is done or dropped). replace: true starts a new plan and keeps the old one in the brain as history.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "goal": {"type": "string"},
                    "phases": {"type": "array", "items": {"type": "string"}},
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {"type": "string", "description": "a-z, 0-9 and -, at most 24"},
                                "title": {"type": "string"},
                                "detail": {"type": "string"},
                                "phase": {"type": "string"},
                                "needs": {"type": "array", "items": {"type": "string"}},
                                "state": {"type": "string", "enum": ["planned", "active", "done", "dropped"]},
                                "branch": {"type": "string"},
                                "note": {"type": "string"},
                                "remove": {"type": "boolean"},
                            },
                            "required": ["id"],
                        },
                    },
                    "replace": {"type": "boolean"},
                },
            }),
        },
    ]
}

fn project_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "project.create".into(),
            description: "Create a real project: makes the folder, registers it in the brain, and moves this session into it. Use it when the work deserves a home instead of scratch — then build inside it.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "folder": {"type": "string"},
                },
                "required": ["title", "folder"],
            }),
        },
        ToolDef {
            name: "project.archive".into(),
            description: "Archive or unarchive a project by slug (archived defaults true). Archived projects stay on disk but leave every list, routing match, and catalog. Pass archived false to bring one back.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "slug": {"type": "string"},
                    "archived": {"type": "boolean"},
                },
                "required": ["slug"],
            }),
        },
        ToolDef {
            name: "project.delete".into(),
            description: "Delete a project by slug: removes its project note. Mapped notes keep their frontmatter; mappings to a missing slug are ignored. The folder on disk is left alone.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"slug": {"type": "string"}},
                "required": ["slug"],
            }),
        },
    ]
}

fn image_defs() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "image.generate".into(),
        description: "Generate an image from a text prompt. Saves a PNG under generated/ in the working folder and publishes it as an image artifact the user sees. Describe the picture concretely (subject, style, mood, composition).".into(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {"type": "string"},
                "size": {"type": "string", "description": "square, wide, or tall"},
            },
            "required": ["prompt"],
        }),
    }]
}

fn doc_defs() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "doc.read".into(),
        description: "Read a document as text: a PDF or text file from an http(s) URL or a local path. Use it for research papers, manuals, and reports (for web pages prefer the browser tools). Returns the first ~12k characters.".into(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {"source": {"type": "string"}},
            "required": ["source"],
        }),
    }]
}

fn team_defs() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "models.list".into(),
            description: "List the agent bench: enabled providers with display names. Check this before staffing subagents.".into(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {},
        }),
    }]
}

pub fn is_session_tool(name: &str) -> bool {
    matches!(
        name,
        "session.spawn"
            | "session.send_message"
            | "session.read_session"
            | "session.list_sessions"
            | "session.rename"
    )
}

fn session_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "session.spawn".into(),
            description: "Start a subsession: a background agent working its own prompt with the same tools you have. It inherits your project, folder, lane, and model unless overridden. wait=true (default) blocks until it finishes and returns its result; wait=false returns its session id at once, and Parzi messages you when it ends (waking you if you are idle), so never poll.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "prompt": {"type": "string"},
                    "is_subsession": {"type": "boolean"},
                    "model": {"type": "string"},
                    "effort": {"type": "string"},
                    "lane": {"type": "string"},
                    "wait": {"type": "boolean"},
                },
                "required": ["prompt"],
            }),
        },
        ToolDef {
            name: "session.send_message".into(),
            description: "Send a follow-up message to another session (for example a background subsession) and optionally wait for its reply.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string"},
                    "message": {"type": "string"},
                    "kind": {"type": "string"},
                    "effort": {"type": "string"},
                    "wait": {"type": "boolean"},
                },
                "required": ["session_id", "message"],
            }),
        },
        ToolDef {
            name: "session.read_session".into(),
            description: "Read what another session (for example a background subsession) has produced. tail_events caps how many recent events come back.".into(),
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
            name: "session.rename".into(),
            description: "Rename this session (only your own): 2 to 5 words in sentence case that say what it is about, at most 60 chars. A session starts out named after the first words of its first message.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"title": {"type": "string"}},
                "required": ["title"],
            }),
        },
        ToolDef {
            name: "session.list_sessions".into(),
            description: "List sessions. only_subsessions=true shows only the subsessions of the calling session.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"only_subsessions": {"type": "boolean"}},
            }),
        },
    ]
}

fn lane_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "lane.dispatch".into(),
            description: "Dispatch a lane worker: like session.spawn but onto a named lane (for example a research lane) instead of inheriting yours. Pass an explicit model to staff by strength.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "prompt": {"type": "string"},
                    "lane": {"type": "string"},
                    "model": {"type": "string"},
                    "effort": {"type": "string"},
                    "wait": {"type": "boolean"},
                },
                "required": ["prompt"],
            }),
        },
        ToolDef {
            name: "lane.status".into(),
            description: "See every lane's live state across sessions: queued and active runs grouped by lane with session ids and titles. Read-only. Use it before staffing so lanes do not double-book.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {},
            }),
        },
    ]
}

pub fn humanize_tool_call(name: &str, args: &serde_json::Value) -> String {
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
            "Starting a helper: {}",
            one_line(&str_arg("title").unwrap_or_else(|| "untitled".into()), 60)
        ),
        "session.send_message" => format!(
            "Messaging {}",
            session_label(&str_arg("session_id").unwrap_or_else(|| "session".into()))
        ),
        "session.read_session" => "Checking on a helper".into(),
        "session.list_sessions" => "Listing helpers".into(),
        "session.rename" => "Naming this session".into(),
        "plan.read" => "Reading the plan".into(),
        "plan.write" => {
            let items = args
                .get("items")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            let replace = args.get("replace").and_then(serde_json::Value::as_bool) == Some(true);
            let count = match items {
                1 => "1 item".to_string(),
                n => format!("{n} items"),
            };
            match (replace, items) {
                (true, _) => format!("Writing a new plan: {count}"),
                (false, 0) => "Updating the plan".into(),
                (false, _) => format!("Updating the plan: {count}"),
            }
        }
        "lane.dispatch" => format!(
            "Handing off: {}",
            one_line(&str_arg("title").unwrap_or_else(|| "a task".into()), 60)
        ),
        "browser.open" => format!(
            "Opening {}",
            one_line(&str_arg("url").unwrap_or_else(|| "a page".into()), 60)
        ),
        "browser.tabs" if args.get("all").and_then(serde_json::Value::as_bool) == Some(true) => {
            "Listing every open tab".into()
        }
        "browser.tabs" => "Listing tabs".into(),
        "browser.read" if str_arg("tab").as_deref() == Some("active") => {
            "Reading the page you have open".into()
        }
        "browser.read" => "Reading the open page".into(),
        "browser.shot" => "Looking at the open page".into(),
        "browser.press" => format!(
            "Pressing {}",
            one_line(&str_arg("key").unwrap_or_else(|| "a key".into()), 30)
        ),
        "browser.select" => format!(
            "Choosing {}",
            one_line(&str_arg("option").unwrap_or_else(|| "an option".into()), 60)
        ),
        "browser.scroll" => format!(
            "Scrolling {}",
            str_arg("to").unwrap_or_else(|| "down".into())
        ),
        "browser.wait" => format!(
            "Waiting for {}",
            one_line(
                &str_arg("text")
                    .or_else(|| str_arg("selector"))
                    .unwrap_or_else(|| "the page".into()),
                60
            )
        ),
        "browser.close" => "Closing a tab".into(),
        "desk.state" => "Looking at the window".into(),
        "desk.open" => format!(
            "Opening {}",
            str_arg("what").unwrap_or_else(|| "a view".into())
        ),
        "desk.notify" => format!(
            "Notifying: {}",
            one_line(&str_arg("text").unwrap_or_default(), 60)
        ),
        "desk.screenshot" => "Taking a screenshot of the window".into(),
        "image.generate" => format!(
            "Drawing {}",
            one_line(&str_arg("prompt").unwrap_or_else(|| "an image".into()), 60)
        ),
        "doc.read" => format!(
            "Reading {}",
            one_line(
                &str_arg("source").unwrap_or_else(|| "a document".into()),
                60
            )
        ),
        "models.list" => "Listing models".into(),
        "shell.start" => format!(
            "Starting {}",
            one_line(&str_arg("cmd").unwrap_or_else(|| "a command".into()), 60)
        ),
        "shell.logs" => "Reading command output".into(),
        "shell.kill" => "Stopping a command".into(),
        "ask.user" => format!(
            "Asking {}",
            one_line(
                &str_arg("question").unwrap_or_else(|| "a question".into()),
                60
            )
        ),
        "request.user" => format!(
            "Requesting {}",
            one_line(
                &str_arg("request").unwrap_or_else(|| "something".into()),
                60
            )
        ),
        "project.create" => format!(
            "Creating project {}",
            one_line(&str_arg("title").unwrap_or_else(|| "untitled".into()), 40)
        ),
        "project.archive" => format!(
            "Archiving project {}",
            one_line(&str_arg("slug").unwrap_or_else(|| "?".into()), 40)
        ),
        "project.delete" => format!(
            "Deleting project {}",
            one_line(&str_arg("slug").unwrap_or_else(|| "?".into()), 40)
        ),
        "brain.delete" => format!(
            "Deleting note {}",
            one_line(&str_arg("path").unwrap_or_else(|| "?".into()), 60)
        ),
        "lane.status" => "Checking what is running".into(),
        "memory.review" => format!(
            "Reviewing note {}",
            one_line(&str_arg("path").unwrap_or_else(|| "?".into()), 60)
        ),
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
        "ui.show_markdown" => "Writing a note".into(),
        "ui.show_artifact" => format!(
            "Publishing {}",
            one_line(
                &str_arg("title")
                    .or_else(|| str_arg("id"))
                    .unwrap_or_else(|| "an artifact".into()),
                60
            )
        ),
        // Connector tools ("mcp__github__list_issues", "gh.issue_get")
        // read by their last word.
        _ => {
            let short = short_tool_name(name).replace('_', " ");
            match hint {
                Some(h) => format!("Using {short}: {}", one_line(&h, 60)),
                None => format!("Using {short}"),
            }
        }
    }
}

fn short_tool_name(name: &str) -> &str {
    name.rsplit("__")
        .next()
        .and_then(|s| s.rsplit('.').next())
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
}

/// Every namespace of Parzi's own tools: an MCP name `plan_write` reads
/// back as `plan.write`, so labels and checks see the real name.
const NAMESPACES: &[&str] = &[
    "ui", "browser", "desk", "brain", "session", "lane", "plan", "shell", "project", "image",
    "doc", "models", "ask", "request", "memory",
];

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

/// A label's argument: its first line, at most `n` chars.
fn one_line(s: &str, n: usize) -> String {
    parzi_core::text::clip(s.lines().next().unwrap_or("").trim(), n)
}

/// A session id as labels show it; anything else (a title) as one line.
fn session_label(s: &str) -> String {
    if s.len() > 8 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        parzi_core::text::short_id(s).to_string()
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
            "Starting a helper: auth worker"
        );
        assert_eq!(
            humanize_tool_call("gh.issue_get", &j(r#"{"number":"12"}"#)),
            "Using issue get: 12"
        );
        assert_eq!(humanize_tool_call("weird.tool", &j("{}")), "Using tool");
        assert_eq!(
            humanize_tool_call("mcp__github__list_issues", &j("{}")),
            "Using list issues"
        );
        assert!(!humanize_tool_call("shell.exec", &j(r#"{"cmd":"a\nb"}"#)).contains('\n'));
    }

    #[test]
    fn connector_modes_never_override_parzi_tools() {
        let auto = |tools: &[&str]| parzi_core::config::McpServerCfg {
            command: "nope".into(),
            tool_modes: tools
                .iter()
                .map(|t| ((*t).to_string(), "auto".to_string()))
                .collect(),
            enabled: true,
            ..Default::default()
        };
        let servers = [
            ("shell", auto(&["exec", "start"])),
            ("project", auto(&["create"])),
            ("browser", auto(&["open"])),
            ("doc", auto(&["read"])),
            ("gh", auto(&["issue_get"])),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let tools = ToolExecutor {
            cwd: String::new(),
            mcp: std::sync::Arc::new(McpManager::new(servers, 60)),
            allowed: vec!["*".into()],
        };
        for own in [
            "shell.exec",
            "shell.start",
            "project.create",
            "browser.open",
            "doc.read",
        ] {
            assert_eq!(tools.approval_override(own), None, "{own}");
        }
        assert_eq!(
            tools.approval_override("gh.issue_get"),
            Some(ApprovalMode::Auto)
        );
    }

    #[test]
    fn window_tools_need_a_window() {
        for t in DESK_TOOLS {
            assert!(is_parzi_tool(t), "{t}");
            assert!(!window_tool_offered(t, false, false), "{t} with no window");
            assert!(window_tool_offered(t, true, true), "{t} on Windows");
        }
        // A window without the page engine (macOS) keeps the desk itself.
        assert!(window_tool_offered("desk.notify", true, false));
        assert!(window_tool_offered("desk.open", true, false));
        assert!(!window_tool_offered("desk.screenshot", true, false));
        assert!(!window_tool_offered("browser.read", true, false));
        assert!(window_tool_offered("brain.read", false, false));
        let all: Vec<String> = browser_defs()
            .into_iter()
            .chain(desk_defs())
            .map(|d| d.name)
            .collect();
        assert_eq!(all.len(), DESK_TOOLS.len());
        assert!(all.iter().all(|n| DESK_TOOLS.contains(&n.as_str())));
        assert_eq!(display_name("mcp__parzi__desk_notify"), "desk.notify");
    }

    #[test]
    fn antigravity_file_calls_name_their_files() {
        let j = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
        assert_eq!(
            edit_paths(
                "Run create_file?",
                &j(r#"{"TargetFile": "notes.txt", "CodeContent": "hello"}"#)
            ),
            ["notes.txt"]
        );
        assert_eq!(
            edit_paths("Running edit_file", &j(r#"{"target_file": "src/a.rs"}"#)),
            ["src/a.rs"]
        );
        assert_eq!(category("Running write_to_file"), Some("fs.write"));
        assert_eq!(category("Run view_file?"), None, "reading writes nothing");
        assert!(edit_paths("Run view_file?", &j(r#"{"TargetFile": "a"}"#)).is_empty());
        assert_eq!(category("run_command"), Some("shell.exec"));
    }

    #[test]
    fn plan_tools_read_as_what_they_do() {
        let j = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
        let three = r#"{"items": [{"id":"a"},{"id":"b"},{"id":"c"}]}"#;
        assert_eq!(
            humanize_tool_call("plan.write", &j(three)),
            "Updating the plan: 3 items"
        );
        assert_eq!(
            humanize_tool_call("mcp__parzi__plan_write", &j(three)),
            "Updating the plan: 3 items",
            "an MCP name reads the same"
        );
        assert_eq!(
            humanize_tool_call(
                "plan.write",
                &j(r#"{"items": [{"id":"a"}], "replace": true}"#)
            ),
            "Writing a new plan: 1 item"
        );
        assert_eq!(
            humanize_tool_call("plan.write", &j(r#"{"title": "x"}"#)),
            "Updating the plan"
        );
        assert_eq!(
            humanize_tool_call("plan.read", &j("{}")),
            "Reading the plan"
        );
        assert_eq!(
            humanize_tool_call("mcp__parzi__session_rename", &j(r#"{"title": "x"}"#)),
            "Naming this session"
        );
        for name in [
            "plan.read",
            "plan.write",
            "session.rename",
            "shell.exec",
            "project.create",
            "image.generate",
            "ask.user",
            "memory.review",
        ] {
            assert_eq!(display_name(&format!("mcp__parzi__{}", to_mcp(name))), name);
            assert!(is_parzi_tool(name), "{name}");
        }
        assert!(is_plan_tool("plan.write") && is_session_tool("session.rename"));
    }

    #[test]
    fn labels_take_the_first_line_within_a_cap() {
        assert_eq!(one_line("  a\nb", 5), "a");
        assert_eq!(one_line("héllo world", 3), "hé…");
        assert_eq!(session_label("0123456789-abcdef"), "01234567");
        assert_eq!(session_label("my helper"), "my helper");
    }
}
