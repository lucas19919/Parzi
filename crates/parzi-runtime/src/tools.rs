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
        d.extend(session_defs());
        d.extend(lane_defs());
        d.extend(image_defs());
        d.extend(doc_defs());
        d.extend(team_defs());
        d.extend(question_defs());
        d.extend(plan_defs());
        d.extend(project_defs());
        d.extend(shell_defs());
        d.retain(|t| self.is_allowed(&t.name));
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

/// Every tool Parzi itself offers. Used to tell lane-allowlist entries
/// that name our own tools apart from entries that speak in vendor
/// categories (e.g. "fs.*") — "shell.exec" is unfortunately both.
pub(crate) fn is_parzi_tool(name: &str) -> bool {
    is_ui_tool(name)
        || is_browser_tool(name)
        || is_session_tool(name)
        || is_lane_tool(name)
        || is_image_tool(name)
        || is_doc_tool(name)
        || is_models_tool(name)
        || is_question_tool(name)
        || is_plan_tool(name)
        || is_project_tool(name)
        || is_shell_tool(name)
        || is_brain_tool(name)
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
            description: "Save/update a versioned artifact card with copy and save actions. html and svg render live in a sandboxed frame with no network access (inline everything; images only as data: URIs), with a show-source toggle. Use for code over ~15 lines, full files, markdown docs, html/svg previews, json/csv data, diffs. Diagrams (architecture, flow, sequence) go here as an svg artifact, or html when they need layout or interactivity. Kinds: code|markdown|html|svg|json|csv|diff|text|preview. The preview kind renders a live UI-registered component inline in the thread: content is JSON like {\"component\":\"omnibar\",\"variant\":2} plus optional props. Reuse the same id to bump the version. Example: {\"artifact\":1,\"id\":\"auth-middleware\",\"title\":\"Auth middleware\",\"kind\":\"code\",\"language\":\"typescript\",\"content\":\"...\"}.".into(),
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
        ToolDef {
            name: "browser.shot".into(),
            description: "Screenshot the session's browser tab and save it as a JPEG file. Returns the file path — open it with your own vision (Read) to actually see the pixels and verify layouts, diagrams, and rendered pages.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {},
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
            | "browser.shot"
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
                    "title": {"type": "string"},
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
    matches!(name, "ask.user")
}

pub(crate) fn is_plan_tool(name: &str) -> bool {
    matches!(name, "plan.write" | "plan.read")
}

pub(crate) fn is_project_tool(name: &str) -> bool {
    matches!(name, "project.create")
}

fn question_defs() -> Vec<ToolDef> {
    vec![ToolDef {
        name: "ask.user".into(),
        description: "Ask the user a question mid-turn and wait for their answer. Use it at real forks instead of guessing: which approach, which scope, proceed/stop. Keep options short (≤6 words each); the user may also type free text. Never ask about something already decided in the thread.".into(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "question": {"type": "string"},
                "options": {"type": "array", "items": {"type": "string"}},
            },
            "required": ["question"],
        }),
    }]
}

fn plan_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "plan.write".into(),
            description: "Write or replace this session's build plan: goal, architecture decisions (each with why — scalability, structure, trade-offs), and steps with status (todo/doing/done). Write the plan BEFORE building anything non-trivial, update step statuses as you go, and record every load-bearing decision. Read it back with plan.read when resuming.".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "goal": {"type": "string"},
                    "decisions": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "decision": {"type": "string"},
                                "why": {"type": "string"},
                            },
                        },
                    },
                    "steps": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": {"type": "string"},
                                "status": {"type": "string"},
                                "note": {"type": "string"},
                            },
                            "required": ["title"],
                        },
                    },
                },
            }),
        },
        ToolDef {
            name: "plan.read".into(),
            description: "Read this session's build plan back (goal, decisions, step statuses).".into(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {},
            }),
        },
    ]
}

fn project_defs() -> Vec<ToolDef> {
    vec![ToolDef {
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
    }]
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
        description: "List the agent bench: enabled providers in routing order with display names and default models. Check this before staffing subagents, then pass an explicit model to session.spawn so each job runs on the right strength (quick lookups on fast models, builds on strong ones).".into(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {},
        }),
    }]
}

pub fn is_session_tool(name: &str) -> bool {
    matches!(
        name,
        "session.spawn" | "session.send_message" | "session.read_session" | "session.list_sessions"
    )
}

fn session_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "session.spawn".into(),
            description: "Start a subsession: a background agent working its own prompt with the same tools you have. It inherits your project, folder, lane, and model unless overridden. wait=true (default) blocks until it finishes and returns its result; wait=false returns its session id immediately for background work you check later with session.read_session. For independent chunks prefer wait=false so they run in parallel, then collect each with session.read_session. Brief the outcome — what must become true and how to verify — never a list of edits.".into(),
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
    vec![ToolDef {
        name: "lane.dispatch".into(),
        description: "Dispatch a lane worker: like session.spawn but onto a named lane (for example a research lane) instead of inheriting yours. Pass an explicit model to staff by strength.".into(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "prompt": {"type": "string"},
                "lane": {"type": "string"},
                "model": {"type": "string"},
                "wait": {"type": "boolean"},
            },
            "required": ["prompt"],
        }),
    }]
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
        "browser.shot" => "Looking at the open page".into(),
        "image.generate" => format!(
            "Drawing {}",
            one_line(&str_arg("prompt").unwrap_or_else(|| "an image".into()), 60)
        ),
        "doc.read" => format!(
            "Reading {}",
            one_line(&str_arg("source").unwrap_or_else(|| "a document".into()), 60)
        ),
        "models.list" => "Checking the bench".into(),
        "shell.start" => format!(
            "Starting {}",
            one_line(&str_arg("cmd").unwrap_or_else(|| "a command".into()), 60)
        ),
        "shell.logs" => "Reading shell output".into(),
        "shell.kill" => "Stopping a shell".into(),
        "ask.user" => format!(
            "Asking {}",
            one_line(&str_arg("question").unwrap_or_else(|| "a question".into()), 60)
        ),
        "plan.write" => "Writing the plan".into(),
        "plan.read" => "Reading the plan".into(),
        "project.create" => format!(
            "Creating project {}",
            one_line(&str_arg("title").unwrap_or_else(|| "untitled".into()), 40)
        ),        "browser.click" => format!(
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
