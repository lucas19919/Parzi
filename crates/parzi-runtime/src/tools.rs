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
    pub session: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    Allow,
    Deny,
    /// Nobody answered before the card's wait ran out.
    Expired,
}

#[async_trait::async_trait]
pub trait Approver: Send + Sync {
    async fn approve(&self, call: &ToolCallInfo) -> Approval;

    /// Whether a person sees the cards. One that refuses unseen says so, so
    /// its refusals never read as the person's own.
    fn asks_a_person(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
pub struct AskRequest {
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
    pub details: Vec<String>,
    pub multi: bool,
    pub kind: String,
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

    fn asks_a_person(&self) -> bool {
        false
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
        if is_vendor_category(name) || is_parzi_tool(name) {
            return None;
        }
        let (server, tool) = name.split_once('.')?;
        self.connector_mode(server, tool)
    }

    /// Whether this session may use the connector `server` at all.
    pub fn connector_allowed(&self, server: &str) -> bool {
        self.allowed
            .iter()
            .any(|p| p == "*" || p.strip_prefix(CONNECTOR_ENTRY) == Some(server))
    }

    /// The person's choice for one connector tool (Settings › Connections);
    /// `None` leaves it to the session's approval mode.
    pub(crate) fn connector_mode(&self, server: &str, tool: &str) -> Option<ApprovalMode> {
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
        let off = || {
            (
                false,
                format!("`{name}` is not enabled for agents in Settings; do it another way, or ask the person to turn it on"),
            )
        };
        if !self.is_allowed(name) {
            return off();
        }
        if let Some((server, tool)) = name.split_once('.') {
            if is_vendor_category(name) {
                return off();
            }
            if is_ui_tool(name)
                || is_brain_tool(name)
                || is_session_tool(name)
                || is_plan_tool(name)
            {
                return (false, format!("tool `{name}` is handled by the agent loop"));
            }
            if !self.mcp.is_tool_exposed(server, tool) {
                return (
                    false,
                    format!("`{tool}` is turned off for the {server} connector in Settings; ask the person to turn it on"),
                );
            }
            if self.mcp.tool_mode(server, tool).as_deref() == Some("deny") {
                return (
                    false,
                    format!("`{tool}` is set to Deny for the {server} connector in Settings; ask the person to allow it there"),
                );
            }
            return self.mcp.call_tool(server, tool, args.clone()).await;
        }
        (false, format!("unknown tool `{name}`"))
    }
}

/// An allowlist entry for a whole connector. It has its own prefix, so a
/// connector called `shell` or `github` never lets Parzi's tools of that
/// name through.
pub const CONNECTOR_ENTRY: &str = "connector:";

#[must_use]
pub fn connector_entry(server: &str) -> String {
    format!("{CONNECTOR_ENTRY}{server}")
}

pub(crate) fn is_vendor_category(name: &str) -> bool {
    matches!(name, "fs.read" | "fs.write" | "fs.list" | "shell.exec")
}

pub(crate) fn category(tool: &str) -> Option<&'static str> {
    match tool {
        "Bash" | "BashOutput" | "KillShell" | "PowerShell" | "Monitor" | "shell" | "execute" => {
            Some("shell.exec")
        }
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "edit" | "delete" | "move" => {
            Some("fs.write")
        }
        "Read" | "Glob" | "Grep" | "LS" | "NotebookRead" | "read" | "search" => Some("fs.read"),
        _ => {
            let l = tool.to_lowercase();
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

const FILE_WRITES: &[&str] = &[
    "create_file",
    "edit_file",
    "write_file",
    "write_to_file",
    "replace_file",
    "multi_replace_file",
];

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

#[must_use]
pub fn shell_command(tool: &str, args: &serde_json::Value) -> Option<String> {
    if !matches!(tool, "shell.exec" | "shell.start") && category(tool) != Some("shell.exec") {
        return None;
    }
    command_arg(args)
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
}

/// The command a shell call runs: `cmd`, or `command` as Bash and most agents
/// name it, as one string or a list of words joined with spaces.
#[must_use]
pub fn command_arg(args: &serde_json::Value) -> Option<String> {
    let words = |v: &serde_json::Value| match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    };
    ["cmd", "command"]
        .iter()
        .filter_map(|k| args.get(*k).and_then(words))
        .find(|c| !c.trim().is_empty())
}

/// A count or a duration however an agent sends it: 120000, 120000.0 or
/// "120000". A fraction is dropped; a negative number or a word is no count.
#[must_use]
pub fn lenient_u64(v: &serde_json::Value) -> Option<u64> {
    // Finite and not negative, so the cast only saturates past u64::MAX.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole = |f: f64| (f.is_finite() && f >= 0.0).then(|| f.trunc() as u64);
    match v {
        serde_json::Value::Number(n) => n.as_u64().or_else(|| n.as_f64().and_then(whole)),
        serde_json::Value::String(s) => {
            let s = s.trim();
            s.parse::<u64>()
                .ok()
                .or_else(|| s.parse::<f64>().ok().and_then(whole))
        }
        _ => None,
    }
}

/// A switch however an agent sends it: true, "true", "yes", "on" or 1, and
/// their opposites. Anything else is no answer, so the tool's default holds.
#[must_use]
pub fn lenient_bool(v: &serde_json::Value) -> Option<bool> {
    match v {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::Number(n) if n.as_f64().is_some_and(|f| f.fract() == 0.0) => {
            match lenient_u64(v) {
                Some(1) => Some(true),
                Some(0) => Some(false),
                _ => None,
            }
        }
        serde_json::Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// `args[key]` read with [`lenient_u64`].
#[must_use]
pub fn int_arg(args: &serde_json::Value, key: &str) -> Option<u64> {
    args.get(key).and_then(lenient_u64)
}

/// `args[key]` read with [`lenient_bool`].
#[must_use]
pub fn bool_arg(args: &serde_json::Value, key: &str) -> Option<bool> {
    args.get(key).and_then(lenient_bool)
}

pub(crate) fn is_wait_tool(name: &str) -> bool {
    name == "wait"
}

pub(crate) fn is_parzi_tool(name: &str) -> bool {
    is_ui_tool(name)
        || is_browser_tool(name)
        || is_desk_tool(name)
        || is_session_tool(name)
        || is_image_tool(name)
        || is_github_tool(name)
        || is_models_tool(name)
        || is_question_tool(name)
        || is_project_tool(name)
        || is_shell_tool(name)
        || is_wait_tool(name)
        || is_brain_tool(name)
        || is_plan_tool(name)
}

pub fn all_defs() -> Vec<ToolDef> {
    let mut d = ui_defs();
    d.extend(browser_defs());
    d.extend(desk_defs());
    d.extend(brain_defs());
    d.extend(session_defs());
    d.extend(image_defs());
    d.extend(github_defs());
    d.extend(team_defs());
    d.extend(question_defs());
    d.extend(plan_defs());
    d.extend(project_defs());
    d.extend(shell_defs());
    d.extend(wait_defs());
    d
}

fn ui_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "ui.show_artifact".into(),
            description: "A versioned card with copy and save, for what the person keeps: code longer than a screen, a diff, a document, or a picture already on disk. FIRST say in one or two sentences what it is, THEN one call with the complete content; never paste it into chat too. Reuse an id for a new version. Kinds: code (set language), diff, markdown (a document), json, csv, text, image (content is the picture's full path), preview. Not for charts (a viz block in your reply), diagrams (the diagonaldiagrams connector), or tables of numbers (markdown); hand built html and svg are refused. preview shows a real Parzi component, for showing or redesigning Parzi itself: content is a JSON string such as {\"component\":\"omnibar\",\"variant\":1} (1 is the agent composer, 2 is web), with optional tokens (bg, text, accent, panel as #hex; radius and space as Npx) and props (input, permission, folder, branch, model, hero). Never send the person to a tab or window to see it.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "slug [a-z0-9-], reused across versions"},
                    "title": {"type": "string"},
                    "kind": {"type": "string", "enum": ["code","markdown","json","csv","diff","text","preview","image"]},
                    "language": {"type": "string"},
                    "content": {"type": "string"},
                },
                "required": ["content"],
            }),
        },
        ToolDef {
            name: "todo.write".into(),
            description: "This session's checklist, shown live under your replies: for work of three steps or more. Send the whole list each time (it replaces the last one): each item a short imperative title and a state, pending, active or done. Keep one item active while you work, mark each done as it is, and finish with every item done or removed; an empty list clears it. It is this session's own list; a project's long-lived plan is plan.write.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "maxItems": 20,
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": {"type": "string", "description": "a short imperative, under 80 characters"},
                                "state": {"type": "string", "enum": ["pending", "active", "done"]},
                            },
                            "required": ["title", "state"],
                        },
                    },
                },
                "required": ["items"],
            }),
        },
        ToolDef {
            name: "ui.show_file".into(),
            description: "Put a file in front of the person: it opens in the side bar beside the conversation, markdown rendered. For something they should read now, such as a plan, a doc you wrote, or a file you are pointing at. path: an absolute path, a path relative to this session's folder, or a note in the brain (projects/parzi.md). Say in a sentence what it is; do not paste it into chat too. A file to keep that is not on disk yet is ui_show_artifact.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
            }),
        },
        ToolDef {
            name: "ui.offer_replies".into(),
            description: "End a turn that leaves the person a decision with one to three reply buttons under your reply: a go-ahead, a choice between two ways, approve or not. Call it last, after your final text. Labels are one to four words (\"Release 0.1.36\", \"Not yet\"); your recommendation goes first. A click sends its message (or the label) as the person's reply. Optional question: the decision in one short line. Skip it when nothing waits on them, and never for questions you need answered mid-turn (that is ask.user).".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "question": {"type": "string", "description": "the decision, one short line"},
                    "replies": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 3,
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": {"type": "string", "description": "1-4 words, max 30 characters"},
                                "message": {"type": "string", "description": "what the click sends, when the label alone is too short"},
                            },
                            "required": ["label"],
                        },
                    },
                },
                "required": ["replies"],
            }),
        },
        ToolDef {
            name: "ui.propose_task".into(),
            description: "Propose follow-up work as a card the person can start with one click as a side task: a new session beside this one that works on its own. Use it for real, independent work your answer turned up (a bug spotted in passing, the same change for a sibling project, a follow-up worth its own session), never for what you are about to do anyway. One card per piece, at most three a turn. The brief is all the new session gets: what to do, where, and what done looks like. Name a project slug to run it in that project's folder.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "under 60 characters"},
                    "brief": {"type": "string", "description": "what to do, where, and what done looks like"},
                    "project": {"type": "string", "description": "a project slug, to work in its folder"},
                },
                "required": ["title", "brief"],
            }),
        },
    ]
}

pub(crate) fn is_ui_tool(name: &str) -> bool {
    matches!(
        name,
        "ui.show_artifact" | "ui.show_file" | "ui.offer_replies" | "ui.propose_task" | "todo.write"
    )
}

fn tab_arg() -> serde_json::Value {
    serde_json::json!({"type": "string", "description": "a tab id from browser.open; default: the tab you used last"})
}

fn browser_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "browser.open".into(),
            description: "Open a URL in one of your page tabs and return its tab id. Without tab or new, your last tab navigates (a first call opens one). new=true opens another tab; tab=<id> navigates that one. Tabs always open in the background, so they never pull the human away from what they are doing. A bare host is https, a phrase is a search.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string"},
                    "new": {"type": "boolean"},
                    "tab": tab_arg(),
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
            description: "Open something in the Parzi window for the human: what=session (id; default this session), settings (section: general, providers, connections, appearance), or home. A session opens as a background tab and never switches the human to it.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "what": {"type": "string", "enum": ["session", "settings", "home"]},
                    "id": {"type": "string"},
                    "section": {"type": "string"},
                },
                "required": ["what"],
            }),
        },
        ToolDef {
            name: "app.actions".into(),
            description: "What you can do in Parzi's own window right now, as named actions with their arguments: tabs, sessions, the composer (model, effort, permission), the side bar and its files, settings, the theme and more. query narrows the list. Run one with app_do. For anything not listed, use app_screen and app_click.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"query": {"type": "string"}},
            }),
        },
        ToolDef {
            name: "app.do".into(),
            description: "Run one of Parzi's named actions (see app_actions) with its arguments, the same way the person's click would. Returns what it did.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "description": "an id from app_actions, such as tabs.new"},
                    "args": {"type": "object"},
                },
                "required": ["action"],
            }),
        },
        ToolDef {
            name: "app.screen".into(),
            description: "Everything the person can press or fill in Parzi's window right now: buttons, tabs, switches, menu items, fields and their values, each with a ref and the card or page it sits in. Use the ref with app_click or app_type. Open a page first (app_do settings.open) to see its controls.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"query": {"type": "string", "description": "only items whose label or place contains this"}},
            }),
        },
        ToolDef {
            name: "app.click".into(),
            description: "Click a control in Parzi's window by its ref from app_screen. A control that deletes, removes, resets or signs out waits for the person's yes. Look again with app_screen after a click that opens a menu or a page.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"ref": {"type": "integer"}},
                "required": ["ref"],
            }),
        },
        ToolDef {
            name: "app.type".into(),
            description: "Fill a field in Parzi's window by its ref from app_screen, replacing what is there. enter: true presses Enter after.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "ref": {"type": "integer"},
                    "text": {"type": "string"},
                    "enter": {"type": "boolean"},
                },
                "required": ["ref", "text"],
            }),
        },
        ToolDef {
            name: "desk.theme".into(),
            description: "Change how Parzi looks, right away: a theme by name (pack, such as graphite, light or dark), single colours as #hex (accent, background, text, panel; panel \"auto\" mixes it from text and background), how the Home wallpaper shows (wallpaper: band or full), how cards look across Parzi, an outline, frosted glass or solid panels (cards: clear, glass or solid) and the dither over it (dither: strength 0 to 100 percent, or off; dither_size: dot size 1 to 6 pixels). Leave out what should stay. With nothing given it lists the themes and the current look. Do this yourself when the person asks; do not send them to Settings.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pack": {"type": "string", "description": "theme name"},
                    "accent": {"type": "string"},
                    "background": {"type": "string"},
                    "text": {"type": "string"},
                    "panel": {"type": "string"},
                    "wallpaper": {"type": "string", "enum": ["band", "full"]},
                    "cards": {"type": "string", "enum": ["clear", "glass", "solid"]},
                    "dither": {"type": ["string", "number"], "description": "strength, 0 to 100 percent, or off"},
                    "dither_size": {"type": ["string", "number"], "description": "dot size, 1 to 6 pixels"},
                },
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
        "desk.state" | "desk.open" | "desk.notify" | "desk.screenshot" | "desk.theme"
    ) || is_app_tool(name)
}

/// Agents using Parzi's own window: its named actions and its controls.
pub(crate) fn is_app_tool(name: &str) -> bool {
    matches!(
        name,
        "app.actions" | "app.do" | "app.screen" | "app.click" | "app.type"
    )
}

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
    "desk.theme",
    "app.actions",
    "app.do",
    "app.screen",
    "app.click",
    "app.type",
];

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
            description: "Search the user's notes vault (the brain) by title and text: a note matches when it holds every word of the query, title matches first, up to 20 with a snippet each. With no query it lists the vault instead: projects with their folders, and each note's path, title, summary and whether it is pinned (attached to sessions in full); pass project (a slug, or all) to list only those.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"query": {"type": "string"}, "project": {"type": "string"}},
            }),
        },
        ToolDef {
            name: "brain.read".into(),
            description: "Read one note from the user's notes vault (the brain) by its path relative to the vault, e.g. projects/parzi.md, as given by brain.search or the on-demand notes listed in your instructions. Returns the raw markdown, frontmatter included.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
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
            description: "Delete a note from the user's notes vault (path relative to the vault, as given by brain.search). Use it for notes that are wrong or obsolete; a project goes with project.delete.".into(),
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
        "brain.search" | "brain.read" | "brain.write" | "brain.delete"
    )
}

pub(crate) fn is_image_tool(name: &str) -> bool {
    matches!(name, "image.generate")
}

pub(crate) fn is_github_tool(name: &str) -> bool {
    matches!(name, "github.issue")
}

fn github_defs() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "github.issue".into(),
        description: "File a GitHub issue, through the person's GitHub CLI (gh). They approve each one first: it is public. repo is owner/name (Parzi's own bugs: lucas19919/Parzi); without it, the repository of this session's folder. The body is markdown: what happened, the steps, what was expected, and the version. Images cannot be attached here: describe them, and name their files so the person can drop them into the issue. Answers the issue's URL, or, when gh is missing or signed out, a prefilled link to open for the person with browser.open.".into(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "body": {"type": "string", "description": "markdown"},
                "repo": {"type": "string", "description": "owner/name"},
                "labels": {"type": "array", "items": {"type": "string"}},
            },
            "required": ["title", "body"],
        }),
    }]
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
            description: "Start a long-running command (dev server, watcher, build) in the background. Returns a shell id immediately; wait for it with wait, peek with shell.logs, stop with shell.kill.".into(),
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

fn wait_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "wait".into(),
            description: "Sleep until background work finishes, then wake with the result — never poll. Waits for background shells (ids from shell.start), sessions (ids from session.spawn or session.list), a plain delay in seconds, or any mix. Wakes when everything watched is done, when any=true the first finish, or at timeout_ms (default 5 minutes, max 10). Returns each thing watched with done or still running.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "shells": {"type": "array", "items": {"type": "string"}, "description": "Background shell ids from shell.start."},
                    "sessions": {"type": "array", "items": {"type": "string"}, "description": "Session ids from session.spawn or session.list."},
                    "seconds": {"type": "number", "description": "Also just wait this long."},
                    "timeout_ms": {"type": "number", "description": "Give up after this long (default 300000, max 600000)."},
                    "any": {"type": "boolean", "description": "Wake on the first finish instead of all of them."},
                },
            }),
        },
    ]
}

pub(crate) fn is_question_tool(name: &str) -> bool {
    name == "ask.user"
}

pub(crate) fn is_project_tool(name: &str) -> bool {
    matches!(
        name,
        "project.create" | "project.switch" | "project.archive" | "project.delete"
    )
}

fn question_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "ask.user".into(),
            description: "Ask the user mid-turn and wait for the answer. At a real fork (which approach, which scope, proceed or stop) give up to six options, each a short label, with a description when the choice needs a line of why or what follows; put your recommendation first and say so; multi: true when several may be picked. They may also answer in their own words, or skip (then decide yourself and say what you assumed). When you need them to give you something instead (a picture, a file, text only they have), leave out options and set kind to image, file or text; reserve that for real needs.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "question": {"type": "string"},
                    "options": {
                        "type": "array",
                        "maxItems": 6,
                        "items": {
                            "anyOf": [
                                {"type": "string"},
                                {
                                    "type": "object",
                                    "properties": {
                                        "label": {"type": "string"},
                                        "description": {"type": "string"},
                                    },
                                    "required": ["label"],
                                },
                            ],
                        },
                    },
                    "multi": {"type": "boolean"},
                    "kind": {"type": "string", "enum": ["text", "file", "image"]},
                },
                "required": ["question"],
            }),
        },
    ]
}

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
            description: "Change this project's plan. Every field is optional and what you leave out stays: title, goal, phases (ordered names such as Now and Next), and items, merged by id: [{id, title?, detail?, phase?, needs?, state?, branch?, note?}]. An id is 1 to 24 of a-z, 0-9 and -; a new item needs a title. needs names other items of the plan, never in a circle. state is planned, active, done or dropped; marking an item active links this session to it. Limits: 40 items, title 80 chars, detail and note 400. remove: true on an item takes out a mistake (finished or abandoned work is done or dropped). replace: true starts a new plan and keeps the old one in the brain as history. The plan is the person's: a write that adds, drops or rewords work is shown to them and saved only when they approve it, and their answer is the result (a change they ask for means nothing was saved yet). Progress (state, note, branch) is saved at once.".into(),
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
            description: "Create a real project: makes the folder, registers it in the brain, and moves this session into it. Use it when the work deserves a home instead of scratch, then build inside it. `folder` as a bare name (\"strohmann\") goes beside the person's other projects; a full path elsewhere asks them first.".into(),
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
            name: "project.switch".into(),
            description: "Move this session into another existing project when the work turns out to belong there, such as being asked about project y from project x. The human approves it unless they gave you full access. slug comes from brain.search with no query. Shell commands run in the new folder right away.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "slug": {"type": "string"},
                },
                "required": ["slug"],
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

fn team_defs() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "models.list".into(),
            description: "List the models you can staff a subsession with: the real model ids of every agent that can run now, and which agents can't and why. Check this before passing `model` to session.spawn.".into(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {},
        }),
    }]
}

pub fn is_session_tool(name: &str) -> bool {
    matches!(
        name,
        "session.spawn" | "session.send" | "session.read" | "session.list" | "session.rename"
    )
}

fn session_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "session.spawn".into(),
            description: "Start a subsession: a background agent working its own prompt with the same tools you have. It inherits your project, folder and model; to use another model, pass an id from models.list. Anything else is refused and nothing starts. wait=true (default) blocks until it finishes and returns its result; wait=false returns its session id at once, and Parzi messages you when it ends (waking you if you are idle), so never poll.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "prompt": {"type": "string"},
                    "is_subsession": {"type": "boolean"},
                    "model": {"type": "string", "description": "An id from models.list, such as claude/opus. Leave it out to use your own model."},
                    "effort": {"type": "string"},
                    "wait": {"type": "boolean"},
                },
                "required": ["prompt"],
            }),
        },
        ToolDef {
            name: "session.send".into(),
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
            name: "session.read".into(),
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
            name: "session.list".into(),
            description: "List this project's sessions with their status (check what is running before staffing, so work is not done twice). only_subsessions=true shows only the subsessions of the calling session.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {"only_subsessions": {"type": "boolean"}},
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
        "session.send" => format!(
            "Messaging {}",
            session_label(&str_arg("session_id").unwrap_or_else(|| "session".into()))
        ),
        "session.read" => "Checking on a helper".into(),
        "session.list" => "Listing helpers".into(),
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
        "browser.open" => format!(
            "Opening {}",
            one_line(&str_arg("url").unwrap_or_else(|| "a page".into()), 60)
        ),
        "browser.tabs" if bool_arg(args, "all") == Some(true) => "Listing every open tab".into(),
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
        "app.actions" => "Looking at what Parzi can do".into(),
        "app.screen" => "Looking at Parzi's window".into(),
        "app.do" => match str_arg("confirm") {
            Some(what) => format!("{what} in Parzi"),
            None => format!("Doing {} in Parzi", str_arg("action").unwrap_or_default()),
        },
        "app.click" => match str_arg("confirm") {
            Some(what) => format!("{what} in Parzi"),
            None => "Clicking in Parzi".into(),
        },
        "app.type" => "Typing in Parzi".into(),
        "desk.theme" => match str_arg("pack") {
            Some(p) => format!("Changing the theme to {p}"),
            None if args.as_object().is_some_and(|o| {
                o.values()
                    .any(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
            }) =>
            {
                "Changing the look".into()
            }
            None => "Reading the theme".into(),
        },
        "desk.notify" => format!(
            "Notifying: {}",
            one_line(&str_arg("text").unwrap_or_default(), 60)
        ),
        "desk.screenshot" => "Taking a screenshot of the window".into(),
        "github.issue" => format!(
            "Filing an issue: {}",
            one_line(&str_arg("title").unwrap_or_default(), 60)
        ),
        "image.generate" => format!(
            "Drawing {}",
            one_line(&str_arg("prompt").unwrap_or_else(|| "an image".into()), 60)
        ),
        "models.list" => "Listing models".into(),
        "shell.start" => format!(
            "Starting {}",
            one_line(&command_arg(args).unwrap_or_else(|| "a command".into()), 60)
        ),
        "shell.logs" => "Reading command output".into(),
        "shell.kill" => "Stopping a command".into(),
        "wait" => {
            let count = ["sessions", "shells"]
                .iter()
                .filter_map(|k| args.get(k).and_then(|v| v.as_array()))
                .map(|a| a.len())
                .sum::<usize>();
            if count > 0 {
                format!("Waiting on {count}")
            } else if let Some(s) = args.get("seconds").and_then(lenient_u64) {
                format!("Waiting {s}s")
            } else {
                "Waiting".into()
            }
        }
        "ask.user" if str_arg("kind").is_some() && args.get("options").is_none() => format!(
            "Asking for {}",
            one_line(
                &str_arg("question").unwrap_or_else(|| "something".into()),
                60
            )
        ),
        "ask.user" => format!(
            "Asking {}",
            one_line(
                &str_arg("question").unwrap_or_else(|| "a question".into()),
                60
            )
        ),
        "project.create" => format!(
            "Creating project {}",
            one_line(&str_arg("title").unwrap_or_else(|| "untitled".into()), 40)
        ),
        "project.switch" => format!(
            "Moving to project {}",
            one_line(&str_arg("slug").unwrap_or_else(|| "?".into()), 40)
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
        "brain.search" => match (str_arg("query"), str_arg("project")) {
            (Some(q), _) => format!("Searching notes for {}", one_line(&q, 60)),
            (None, Some(p)) => format!("Listing notes for {}", one_line(&p, 60)),
            (None, None) => "Listing notes".into(),
        },
        "brain.read" => format!(
            "Reading note {}",
            one_line(&str_arg("path").unwrap_or_else(|| "?".into()), 60)
        ),
        "brain.write" => format!(
            "Writing note {}",
            one_line(&str_arg("path").unwrap_or_else(|| "?".into()), 60)
        ),
        "ui.show_file" => format!(
            "Showing {}",
            one_line(&str_arg("path").unwrap_or_else(|| "a file".into()), 60)
        ),
        "ui.show_artifact" => format!(
            "Publishing {}",
            one_line(
                &str_arg("title")
                    .or_else(|| str_arg("id"))
                    .unwrap_or_else(|| "an artifact".into()),
                60
            )
        ),
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

const NAMESPACES: &[&str] = &[
    "ui", "browser", "desk", "brain", "session", "plan", "shell", "project", "image", "models",
    "ask", "todo", "github", "app",
];

/// The MCP server agents reach Parzi's own tools on.
pub const PARZI_SERVER: &str = "parzi";

/// A name Parzi would serve, such as `shell.run`, whether or not a tool has it.
pub(crate) fn in_parzi_namespace(name: &str) -> bool {
    name.split_once('.')
        .is_some_and(|(ns, _)| NAMESPACES.contains(&ns))
}

pub fn to_mcp(name: &str) -> String {
    name.replace('.', "_")
}

/// How the agent on the other end names Parzi's tools, so a message names one
/// it can call as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallNames {
    prefix: Option<String>,
}

impl CallNames {
    /// Claude Code calls them `mcp__parzi__<tool>`. Other agents name them in
    /// ways Parzi never sees, so for them a message names the MCP tool and its server.
    #[must_use]
    pub fn for_agent(provider: &str) -> Self {
        let prefix = match parzi_providers::canonical_id(provider) {
            Some("claude") => Some(format!("mcp__{PARZI_SERVER}__")),
            _ => None,
        };
        Self { prefix }
    }

    /// `shell.exec` reads `mcp__parzi__shell_exec` to Claude and
    /// `shell_exec` on the `parzi` server to everyone else.
    #[must_use]
    pub fn tool(&self, name: &str) -> String {
        match &self.prefix {
            Some(_) => self.short(name),
            None => format!("{} on the `{PARZI_SERVER}` server", self.short(name)),
        }
    }

    /// The name alone, for a list after the server was named once.
    #[must_use]
    pub fn short(&self, name: &str) -> String {
        let mcp = to_mcp(current_name(&display_name(name)));
        format!("`{}{mcp}`", self.prefix.as_deref().unwrap_or(""))
    }
}

pub(crate) fn from_mcp(tool: &str) -> String {
    // A connector tool relayed by Parzi reads `<server>__<tool>`; Parzi's own
    // names never hold `__`, so a connector called `github` stays itself.
    if let Some((server, rest)) = tool.split_once("__") {
        if !server.is_empty() && !rest.is_empty() {
            return format!("{server}.{rest}");
        }
    }
    for ns in NAMESPACES {
        if let Some(rest) = tool.strip_prefix(ns).and_then(|r| r.strip_prefix('_')) {
            return format!("{ns}.{rest}");
        }
    }
    tool.to_string()
}

#[must_use]
pub fn current_name(name: &str) -> &str {
    match name {
        "session.send_message" => "session.send",
        "session.read_session" => "session.read",
        "session.list_sessions" => "session.list",
        "brain.list" => "brain.search",
        "request.user" => "ask.user",
        other => other,
    }
}

pub fn display_name(name: &str) -> String {
    name.strip_prefix("mcp__parzi__")
        .or_else(|| name.strip_prefix("parzi__"))
        .or_else(|| name.strip_prefix("parzi."))
        .or_else(|| name.strip_prefix("parzi_"))
        .map_or_else(|| name.to_string(), from_mcp)
}

fn one_line(s: &str, n: usize) -> String {
    parzi_core::text::clip(s.lines().next().unwrap_or("").trim(), n)
}

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
        for d in all_defs() {
            assert_eq!(
                from_mcp(&to_mcp(&d.name)),
                d.name,
                "{} lost its name",
                d.name
            );
        }
        assert_eq!(display_name("parzi__ui_propose_task"), "ui.propose_task");
        assert_eq!(display_name("search_tool"), "search_tool");
    }

    #[test]
    fn relayed_connector_tools_read_back_as_their_connector() {
        assert_eq!(display_name("mcp__parzi__github__get_me"), "github.get_me");
        assert_eq!(
            display_name("mcp__parzi__diagonaldiagrams__render"),
            "diagonaldiagrams.render"
        );
        assert_eq!(display_name("mcp__parzi__github_issue"), "github.issue");
        assert!(!is_parzi_tool(&display_name(
            "mcp__parzi__github__issue_read"
        )));
        let tools = ToolExecutor {
            cwd: String::new(),
            mcp: std::sync::Arc::new(McpManager::new(Default::default(), 60)),
            allowed: vec![connector_entry("github"), "shell.exec".into()],
        };
        assert!(tools.connector_allowed("github"));
        assert!(!tools.connector_allowed("linear"));
        assert!(
            !tools.is_allowed("github.issue"),
            "a connector entry is no Parzi tool"
        );
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
            "session.read",
        ] {
            assert_eq!(display_name(&format!("mcp__parzi__{}", to_mcp(name))), name);
            assert!(is_parzi_tool(name), "{name}");
        }
        assert!(is_plan_tool("plan.write") && is_session_tool("session.rename"));
    }

    #[test]
    fn counts_and_switches_read_however_the_agent_spells_them() {
        use serde_json::json;
        for (v, want) in [
            (json!(120_000), Some(120_000)),
            (json!(120_000.0), Some(120_000)),
            (json!("120000"), Some(120_000)),
            (json!(" 5000 "), Some(5000)),
            (json!("2.5"), Some(2)),
            (json!(0), Some(0)),
            (json!(-1), None),
            (json!("-3"), None),
            (json!("soon"), None),
            (json!(true), None),
            (json!(null), None),
            (json!(f64::MAX), Some(u64::MAX)),
        ] {
            assert_eq!(lenient_u64(&v), want, "{v}");
        }
        for (v, want) in [
            (json!(true), Some(true)),
            (json!(false), Some(false)),
            (json!("false"), Some(false)),
            (json!("True"), Some(true)),
            (json!("no"), Some(false)),
            (json!("on"), Some(true)),
            (json!(1), Some(true)),
            (json!(0.0), Some(false)),
            (json!(0.5), None),
            (json!(2), None),
            (json!("maybe"), None),
            (json!(null), None),
        ] {
            assert_eq!(lenient_bool(&v), want, "{v}");
        }
        let args = json!({"wait": "false", "timeout_ms": "9000"});
        assert_eq!(
            bool_arg(&args, "wait"),
            Some(false),
            "\"false\" does not block"
        );
        assert_eq!(int_arg(&args, "timeout_ms"), Some(9000));
        assert_eq!(int_arg(&args, "missing"), None);
    }

    #[test]
    fn a_shell_command_comes_as_cmd_or_as_command() {
        use serde_json::json;
        assert_eq!(command_arg(&json!({"cmd": "ls"})).as_deref(), Some("ls"));
        assert_eq!(
            command_arg(&json!({"command": "cargo test"})).as_deref(),
            Some("cargo test")
        );
        assert_eq!(
            command_arg(&json!({"command": ["git", "status", "-s"]})).as_deref(),
            Some("git status -s")
        );
        assert_eq!(
            command_arg(&json!({"cmd": "", "command": "pwd"})).as_deref(),
            Some("pwd"),
            "an empty cmd does not hide the command"
        );
        assert_eq!(command_arg(&json!({"cmd": 7})), None);
        assert_eq!(
            shell_command("shell.start", &json!({"command": "npm run dev"})).as_deref(),
            Some("npm run dev")
        );
        assert_eq!(
            humanize_tool_call("shell.start", &json!({"command": "npm run dev"})),
            "Starting npm run dev"
        );
    }

    #[test]
    fn messages_name_parzis_tools_the_way_the_agent_calls_them() {
        let claude = CallNames::for_agent("claude");
        assert_eq!(claude.tool("shell.exec"), "`mcp__parzi__shell_exec`");
        assert_eq!(
            claude.tool("mcp__parzi__ask_user"),
            "`mcp__parzi__ask_user`"
        );
        assert_eq!(
            claude.tool("session.send_message"),
            "`mcp__parzi__session_send`",
            "an old name reads as the one it is now"
        );
        for other in ["codex", "opencode", "grok", "nobody"] {
            assert_eq!(
                CallNames::for_agent(other).tool("shell.exec"),
                "`shell_exec` on the `parzi` server",
                "{other}"
            );
        }
        assert_eq!(CallNames::default(), CallNames::for_agent("codex"));
    }

    #[test]
    fn labels_take_the_first_line_within_a_cap() {
        assert_eq!(one_line("  a\nb", 5), "a");
        assert_eq!(one_line("héllo world", 3), "hé…");
        assert_eq!(session_label("0123456789-abcdef"), "01234567");
        assert_eq!(session_label("my helper"), "my helper");
    }
}
