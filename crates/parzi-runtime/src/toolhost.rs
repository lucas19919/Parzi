//! One run's side of the table: Parzi's own tools — widgets, teamwork,
//! plans, knowledge, leases, the role's project tools and the connectors
//! the lane allows — and the gate every action of the vendor agent passes.
//!
//! The agent reaches the tools over MCP (`mcp_host`). Its own actions
//! (edits, shell commands) arrive as permission requests; the same policy
//! decides both: lane lockdown, lane allowlist, leases, workspace hooks,
//! then the approval mode.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use parzi_core::context::InterKind;
use parzi_core::error::Result;
use parzi_core::store::{Event, SessionStore};
use parzi_core::{artifacts, widgets};
use parzi_providers::{PermissionDecision, PermissionGate, PermissionRequest};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::handler::{HarnessBridge, RunEvent, RunSink};
use crate::lease_tools::ShellAudit;
use crate::roles::RoleBinding;
use crate::tools::{
    display_name, is_lane_tool, is_session_tool, is_ui_tool, Approval, ApprovalMode, Approver,
    ToolCallInfo, ToolDef, ToolExecutor,
};

/// Everything a run's tools need, gathered once at launch.
pub struct ToolHostParts {
    pub session_id: String,
    pub lane: String,
    pub mode: ApprovalMode,
    /// The composer's "edits" pill: in Ask mode, file edits run without a
    /// prompt while everything else still asks. Never lifts Deny.
    pub edits_auto: bool,
    pub store: SessionStore,
    pub tools: Arc<ToolExecutor>,
    pub approver: Arc<dyn Approver>,
    pub harness: Option<Arc<dyn HarnessBridge>>,
    /// §1.2: the project role this run is, when it is one.
    pub role: Option<RoleBinding>,
    pub sink: RunSink,
    pub cancel: CancellationToken,
}

pub struct ToolHost {
    p: ToolHostParts,
    /// Shell commands in flight: vendor tool id → the worktree before it.
    audits: std::sync::Mutex<HashMap<String, ShellAudit>>,
    /// The tool list, built once per run: listing a connector can take
    /// seconds, and agents ask for the list more than once.
    defs: tokio::sync::OnceCell<Vec<ToolDef>>,
}

/// What an agent's own tool does, in the names lane allowlists have always
/// used. `None`: fetches, sub-agents, anything else.
fn category(tool: &str) -> Option<&'static str> {
    match tool {
        // Claude Code, Codex (`shell`), ACP kinds (`execute`).
        "Bash" | "BashOutput" | "KillShell" | "shell" | "execute" => Some("shell.exec"),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "edit" | "delete" | "move" => {
            Some("fs.write")
        }
        "Read" | "Glob" | "Grep" | "LS" | "NotebookRead" | "read" | "search" => Some("fs.read"),
        _ => None,
    }
}

/// `path` relative to `cwd`, with forward slashes, when it names a file
/// inside it as the file system will resolve it, not only as it reads. Two
/// passes: the text ([`relative_text`]), then the disk: the deepest part of
/// the path that exists is resolved through links, junctions and short
/// names and must still be inside `cwd`, resolved the same way. The answer
/// is spelled as resolved, so a link inside the folder is leased by where
/// it points. No `cwd`, or one that cannot be resolved: no inside.
fn worktree_relative(cwd: &str, path: &str) -> Option<String> {
    let rel = relative_text(cwd, path)?;
    let root = std::fs::canonicalize(cwd).ok()?;
    let parts: Vec<&str> = rel.split('/').collect();
    let mut here = root.clone();
    let mut known = 0;
    for seg in &parts {
        let next = here.join(seg);
        // A dangling link exists too; it just does not resolve below.
        if std::fs::symlink_metadata(&next).is_err() {
            break;
        }
        here = next;
        known += 1;
    }
    let real = std::fs::canonicalize(&here).ok()?;
    let mut out: Vec<String> = real
        .strip_prefix(&root)
        .ok()?
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    out.extend(parts[known..].iter().map(|s| (*s).to_string()));
    (!out.is_empty()).then(|| out.join("/"))
}

/// The text pass of [`worktree_relative`]. Relative paths are taken from
/// `cwd`; `..` is resolved on the text, so a climb out is outside. Anything
/// rooted — a drive, `\foo`, `C:foo`, a share, a verbatim path — must start
/// with `cwd`, without case where the file system ignores it. Whatever a
/// shell or Windows would read differently from the text is outside too: a
/// leading `~`, surrounding spaces, and on Windows a `:` inside a name (a
/// stream), a name ending in a dot or a space, or a device name.
fn relative_text(cwd: &str, path: &str) -> Option<String> {
    let base = cwd.trim().replace('\\', "/");
    let base = base.trim_end_matches('/');
    if base.is_empty() || path.is_empty() || path.trim() != path || path.starts_with('~') {
        return None;
    }
    let p = path.replace('\\', "/");
    let raw = Path::new(path);
    let rooted = raw.has_root()
        || p.starts_with('/')
        || matches!(
            raw.components().next(),
            Some(std::path::Component::Prefix(_))
        );
    let rel = if rooted {
        strip_prefix_fold(&p, base)?.strip_prefix('/')?
    } else {
        p.as_str()
    };
    let mut parts: Vec<&str> = vec![];
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s if cfg!(windows) && !windows_plain_name(s) => return None,
            s => parts.push(s),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `s` without the leading `prefix`, compared without case where the file
/// system ignores it. The rest keeps its own spelling.
fn strip_prefix_fold<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let mut rest = s.char_indices();
    for want in prefix.chars() {
        let (_, got) = rest.next()?;
        let same = want == got
            || (parzi_core::project::PATHS_IGNORE_CASE
                && want.to_lowercase().eq(got.to_lowercase()));
        if !same {
            return None;
        }
    }
    Some(rest.next().map_or("", |(i, _)| &s[i..]))
}

/// A name Windows keeps as written: no stream (`a.rs:x`), no trailing dot
/// or space (Windows drops them, so `.. ` climbs), and not a device.
fn windows_plain_name(seg: &str) -> bool {
    const DEVICES: [&str; 6] = ["con", "prn", "aux", "nul", "conin$", "conout$"];
    let stem = seg
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_lowercase();
    let port = |p: &str| {
        stem.strip_prefix(p).is_some_and(|n| {
            n.chars().count() == 1 && n.chars().all(|c| c.is_ascii_digit() || "¹²³".contains(c))
        })
    };
    !seg.contains(':')
        && !seg.ends_with(['.', ' '])
        && !DEVICES.contains(&stem.as_str())
        && !port("com")
        && !port("lpt")
}

/// The files a write names: the request's paths, or the file field of its
/// input when an agent sent none.
fn write_targets(req: &PermissionRequest) -> Vec<String> {
    if !req.paths.is_empty() {
        return req.paths.clone();
    }
    ["file_path", "filePath", "notebook_path", "path"]
        .iter()
        .filter_map(|k| req.input.get(*k).and_then(Value::as_str))
        .map(str::to_string)
        .take(1)
        .collect()
}

impl ToolHost {
    pub fn new(parts: ToolHostParts) -> Self {
        Self {
            p: parts,
            audits: std::sync::Mutex::new(HashMap::new()),
            defs: tokio::sync::OnceCell::new(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.p.session_id
    }

    /// The tools this run offers over MCP, by Parzi's dotted names: the
    /// always-on ones, the lease tools for a project lane, the role's
    /// project tools, and the connectors the lane allows.
    pub async fn defs(&self) -> Vec<ToolDef> {
        self.defs
            .get_or_init(|| async {
                let mut defs = self.p.tools.defs_with_mcp().await;
                if let Some(binding) = &self.p.role {
                    defs.extend(crate::project_flow::project_defs_for(binding.role));
                }
                defs
            })
            .await
            .clone()
    }

    /// Run one of Parzi's tools for the agent. Workspace hooks wrap every
    /// call except rendering: pre-hooks can refuse before the approval gate,
    /// post-hooks observe after.
    pub async fn call(&self, name: &str, args: &Value) -> (bool, String) {
        let id = uuid::Uuid::new_v4().to_string();
        if is_ui_tool(name) {
            return self.execute_ui_tool(name, args);
        }
        let (denied, warnings) =
            crate::hooks::pre_tool(&self.p.store, &self.p.session_id, name, args).await;
        for w in warnings {
            self.p.sink.emit(RunEvent::Notice { text: w });
        }
        if let Some(reason) = denied {
            return (false, format!("blocked by a workspace hook: {reason}"));
        }
        let (ok, output) = self.execute_inner(&id, name, args).await;
        for n in crate::hooks::post_tool(&self.p.store, &self.p.session_id, name, args, ok, &output)
            .await
        {
            self.p.sink.emit(RunEvent::Notice { text: n });
        }
        (ok, output)
    }

    async fn execute_inner(&self, id: &str, name: &str, args: &Value) -> (bool, String) {
        if is_session_tool(name) {
            // H-5: session tools are tools: the same gate as everything else.
            if !self.approved(id, name, args).await {
                return (
                    false,
                    format!("tool `{name}` denied (lane mode / approver)"),
                );
            }
            return self.execute_session_tool(name, args).await;
        }
        if is_lane_tool(name) {
            if !self.approved(id, name, args).await {
                return (
                    false,
                    format!("tool `{name}` denied (lane mode / approver)"),
                );
            }
            return self.execute_lane_tool(name, args).await;
        }
        if !self.approved(id, name, args).await {
            return (
                false,
                format!("tool `{name}` denied (lane mode / approver)"),
            );
        }
        // `project.*` belongs to the run's role, not to the lane cwd.
        if let Some(binding) = &self.p.role {
            if crate::project_flow::is_project_tool(name) {
                return crate::project_flow::execute_project_tool(
                    binding.role,
                    &binding.ctx,
                    name,
                    args,
                );
            }
        }
        self.p.tools.execute(name, args).await
    }

    async fn approved(&self, id: &str, name: &str, args: &Value) -> bool {
        // R-8: lane Deny is absolute — no per-tool override can lift it.
        // The allowlist is checked before any prompt so nobody approves a
        // tool that is then refused anyway.
        if self.p.mode == ApprovalMode::Deny {
            return false;
        }
        if !self.p.tools.is_allowed(name) {
            return false;
        }
        // Per-tool connector override wins over lane Ask/Auto.
        match self.p.tools.approval_override(name) {
            Some(ApprovalMode::Deny) => return false,
            Some(ApprovalMode::Auto) => return true,
            Some(ApprovalMode::Ask) => return self.ask(id, name, args).await,
            None => {}
        }
        match self.p.mode {
            ApprovalMode::Auto => true,
            ApprovalMode::Deny => false,
            ApprovalMode::Ask => self.ask(id, name, args).await,
        }
    }

    async fn ask(&self, id: &str, name: &str, args: &Value) -> bool {
        let info = ToolCallInfo {
            id: id.into(),
            name: name.into(),
            args: args.clone(),
            lane: self.p.lane.clone(),
        };
        // Single approval path: the Approver is the gate. The event is for
        // observers; a slow human really does block here.
        self.p
            .sink
            .emit(RunEvent::ApprovalRequest { call: info.clone() });
        tokio::select! {
            () = self.p.cancel.cancelled() => false,
            r = self.p.approver.approve(&info) => matches!(r, Approval::Allow),
        }
    }

    /// The gate for the agent's own actions. See the module note for order.
    async fn gate(&self, req: &PermissionRequest) -> PermissionDecision {
        // Parzi's own tools come in over MCP and pass Parzi's gate when they
        // run (`call`): asking here as well would make the person approve twice.
        let parzi = display_name(&req.tool);
        if parzi != req.tool {
            return PermissionDecision::Allow;
        }
        let kind = category(&req.tool);
        if self.p.mode == ApprovalMode::Deny {
            if kind == Some("fs.read") {
                return PermissionDecision::Allow;
            }
            return PermissionDecision::Deny("this lane is locked down: read-only".into());
        }
        if let Some(k) = kind {
            // A lane that lists file or shell kinds and leaves this one out
            // refuses it. Parzi adds its own tool names to every lane, so
            // only the kinds say whether the lane meant to restrict these.
            let lists_kinds = self
                .p
                .tools
                .allowed
                .iter()
                .any(|a| crate::tools::is_vendor_category(a) || a == "fs.*" || a == "shell.*");
            if lists_kinds && !self.p.tools.is_allowed(k) {
                return PermissionDecision::Deny(format!("this lane does not allow {k}"));
            }
        }
        // A write outside the worktree, or to a file nobody named, is never
        // approved on the lane's behalf: a person decides, and a run with
        // nobody to ask is refused. Reads go anywhere, as under Codex's
        // workspace sandbox.
        let mut outside = false;
        if kind == Some("fs.write") {
            let targets = write_targets(req);
            let rel: Vec<Option<String>> = targets
                .iter()
                .map(|p| worktree_relative(&self.p.tools.cwd, p))
                .collect();
            outside = targets.is_empty() || rel.iter().any(Option::is_none);
            let inside: Vec<String> = rel.into_iter().flatten().collect();
            if let Some(why) = self.p.tools.lease_gate_paths(&inside).await {
                return PermissionDecision::Deny(why);
            }
        }
        let hook_name = kind.unwrap_or(req.tool.as_str());
        let (denied, warnings) =
            crate::hooks::pre_tool(&self.p.store, &self.p.session_id, hook_name, &req.input).await;
        for w in warnings {
            self.p.sink.emit(RunEvent::Notice { text: w });
        }
        if let Some(reason) = denied {
            return PermissionDecision::Deny(format!("blocked by a workspace hook: {reason}"));
        }
        let allowed = match self.p.mode {
            ApprovalMode::Auto if !outside => true,
            ApprovalMode::Ask if self.p.edits_auto && kind == Some("fs.write") && !outside => true,
            _ => {
                let mut args = req.input.clone();
                if let Value::Object(o) = &mut args {
                    if outside {
                        let title = format!("{} — outside this lane's folder", req.title);
                        o.insert("title".into(), Value::String(title));
                    } else {
                        o.entry("title")
                            .or_insert_with(|| Value::String(req.title.clone()));
                    }
                }
                self.ask(&req.id, &req.tool, &args).await
            }
        };
        if !allowed {
            return PermissionDecision::Deny("declined".into());
        }
        if kind == Some("shell.exec") {
            self.begin_audit(&req.id).await;
        }
        PermissionDecision::Allow
    }

    /// A shell command is about to run: take the lease layer's before-picture
    /// (once per command; the permission request usually got there first).
    pub async fn tool_started(&self, id: &str, tool: &str) {
        if category(tool) == Some("shell.exec") {
            self.begin_audit(id).await;
        }
    }

    async fn begin_audit(&self, id: &str) {
        if self.audits.lock().is_ok_and(|a| a.contains_key(id)) {
            return;
        }
        if let Some(audit) = self.p.tools.shell_audit_start().await {
            if let Ok(mut a) = self.audits.lock() {
                a.entry(id.to_string()).or_insert(audit);
            }
        }
    }

    /// A tool finished. For a shell command, anything it wrote into a file
    /// another lane holds is put back; the refusal comes back for the result.
    pub async fn tool_finished(&self, id: &str) -> Option<String> {
        let audit = self.audits.lock().ok()?.remove(id)?;
        self.p.tools.shell_audit_finish(audit).await
    }

    /// Dispatch `session.*` over the bridge. Spawns and messages also land in
    /// this session's transcript so the thread shows the teamwork.
    async fn execute_session_tool(&self, name: &str, args: &Value) -> (bool, String) {
        let Some(bridge) = &self.p.harness else {
            return (false, "session tools unavailable in this run".into());
        };
        let sid = &self.p.session_id;
        let str_arg = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let bool_arg = |k: &str, dflt: bool| args.get(k).and_then(Value::as_bool).unwrap_or(dflt);
        let out: Result<String> = match name {
            "session.spawn" => {
                let title = str_arg("title").unwrap_or_else(|| "subsession".into());
                let prompt = str_arg("prompt").unwrap_or_default();
                if prompt.trim().is_empty() {
                    return (false, "session.spawn needs a `prompt`".into());
                }
                bridge
                    .spawn_session(
                        sid,
                        &title,
                        &prompt,
                        bool_arg("is_subsession", true),
                        str_arg("model").filter(|s| !s.trim().is_empty()),
                        str_arg("lane").filter(|s| !s.trim().is_empty()),
                        bool_arg("wait", true),
                    )
                    .await
            }
            "session.send_message" => {
                let target = str_arg("session_id").unwrap_or_default();
                let message = str_arg("message").unwrap_or_default();
                if target.trim().is_empty() || message.trim().is_empty() {
                    return (
                        false,
                        "session.send_message needs `session_id` + `message`".into(),
                    );
                }
                let kind = InterKind::parse(&str_arg("kind").unwrap_or_default());
                bridge
                    .send_message(sid, &target, &message, kind, bool_arg("wait", false))
                    .await
            }
            "session.read_session" => {
                let target = str_arg("session_id").unwrap_or_default();
                if target.trim().is_empty() {
                    return (false, "session.read_session needs `session_id`".into());
                }
                let tail = args
                    .get("tail_events")
                    .and_then(Value::as_u64)
                    .map(|n| (n.min(60)) as usize);
                bridge.read_session(sid, &target, tail).await
            }
            "session.list_sessions" => {
                bridge
                    .list_sessions(sid, bool_arg("only_subsessions", false))
                    .await
            }
            _ => return (false, format!("unknown session tool `{name}`")),
        };
        match out {
            Ok(text) => {
                if matches!(name, "session.spawn" | "session.send_message") {
                    self.p.sink.emit(RunEvent::Notice {
                        text: text.chars().take(240).collect(),
                    });
                    let _ = self.p.store.append(
                        sid,
                        &Event::System {
                            text: format!("{name}: {text}"),
                        },
                    );
                }
                (true, text)
            }
            Err(e) => (false, e.to_string()),
        }
    }

    /// `lane.dispatch`: a worker subsession on the project's implementation
    /// role settings; the caller's lane and model when the roster is empty.
    async fn execute_lane_tool(&self, name: &str, args: &Value) -> (bool, String) {
        let Some(bridge) = &self.p.harness else {
            return (false, "lane tools unavailable in this run".into());
        };
        if name != "lane.dispatch" {
            return (false, format!("unknown lane tool `{name}`"));
        }
        let str_arg = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let title = str_arg("title").unwrap_or_else(|| "lane worker".into());
        let prompt = str_arg("prompt").unwrap_or_default();
        if prompt.trim().is_empty() {
            return (false, "lane.dispatch needs a `prompt`".into());
        }
        let lane = str_arg("lane").filter(|s| !s.trim().is_empty());
        let wait = args.get("wait").and_then(Value::as_bool).unwrap_or(true);
        let mut role_model: Option<String> = None;
        if let Ok(meta) = self.p.store.get(&self.p.session_id) {
            if let Ok(roster) = parzi_core::lanes::get_project_roster(&meta.project) {
                role_model = roster.implementation.model.filter(|s| !s.trim().is_empty());
            }
        }
        match bridge
            .spawn_session(
                &self.p.session_id,
                &title,
                &prompt,
                true,
                role_model,
                lane,
                wait,
            )
            .await
        {
            Ok(text) => {
                self.p.sink.emit(RunEvent::Notice {
                    text: text.chars().take(240).collect(),
                });
                let _ = self.p.store.append(
                    &self.p.session_id,
                    &Event::System {
                        text: format!("{name}: {text}"),
                    },
                );
                (true, text)
            }
            Err(e) => (false, e.to_string()),
        }
    }

    fn execute_ui_tool(&self, name: &str, args: &Value) -> (bool, String) {
        let widget = |fence: &str, payload: Value| {
            let _ = self.p.store.append(
                &self.p.session_id,
                &Event::Widget {
                    fence: fence.into(),
                    payload,
                },
            );
            (true, "rendered".to_string())
        };
        match name {
            "ui.show_markdown" => {
                let md = args.get("markdown").and_then(|m| m.as_str()).unwrap_or("");
                if let Some(lang) = crate::handler::ascii_diagram_fence(md) {
                    return (
                        false,
                        format!(
                            "rejected: ASCII/box-drawing diagram in ```{lang} fence renders as a dead console window — \
                             call ui.show_diagram with nodes[]/edges[] for diagrams, ui.show_widget for charts/tables, never ASCII boxes"
                        ),
                    );
                }
                let payload = serde_json::json!({"widget": 1, "type": "markdown", "text": md});
                match widgets::validate_widget(&payload) {
                    Ok(_) => widget("parzi-widget", payload),
                    Err(e) => (false, format!("invalid markdown: {e}")),
                }
            }
            "ui.show_widget" => match widgets::validate_widget(args) {
                Ok(_) => widget("parzi-widget", args.clone()),
                Err(e) => (false, format!("invalid widget: {e}")),
            },
            "ui.show_diagram" => match widgets::validate_diagram(args) {
                Ok(_) => widget("parzi-diagram", args.clone()),
                Err(e) => (false, format!("invalid diagram: {e}")),
            },
            "ui.show_artifact" => match self.store_artifact(args) {
                Ok(msg) => (true, msg),
                Err(e) => (false, e),
            },
            _ => (false, format!("unknown ui tool `{name}`")),
        }
    }

    /// Validate + version + dedup + append an artifact.
    fn store_artifact(&self, args: &Value) -> std::result::Result<String, String> {
        let mut candidate = artifacts::validate_artifact(args).map_err(|e| e.to_string())?;
        let existing = self.existing_artifacts();
        if artifacts::is_same_content(&candidate.id, &candidate.content, &existing) {
            return Ok(format!(
                "artifact {} unchanged (no new version)",
                candidate.id
            ));
        }
        candidate.version = artifacts::next_version(&candidate.id, &existing);
        let payload = serde_json::to_value(&candidate).map_err(|e| e.to_string())?;
        self.p
            .store
            .append(
                &self.p.session_id,
                &Event::Artifact {
                    id: candidate.id.clone(),
                    title: candidate.title.clone(),
                    artifact_kind: candidate.kind.clone(),
                    version: candidate.version,
                    payload,
                },
            )
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "rendered artifact {} v{}",
            candidate.id, candidate.version
        ))
    }

    fn existing_artifacts(&self) -> Vec<artifacts::ArtifactV1> {
        let events = self.p.store.events(&self.p.session_id).unwrap_or_default();
        events
            .into_iter()
            .filter_map(|e| match e {
                Event::Artifact { payload, .. } => serde_json::from_value(payload).ok(),
                _ => None,
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl PermissionGate for ToolHost {
    async fn decide(&self, request: PermissionRequest) -> PermissionDecision {
        self.gate(&request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_tools_fall_into_the_lane_categories() {
        assert_eq!(category("Bash"), Some("shell.exec"));
        assert_eq!(category("shell"), Some("shell.exec"));
        assert_eq!(category("execute"), Some("shell.exec"));
        assert_eq!(category("Edit"), Some("fs.write"));
        assert_eq!(category("edit"), Some("fs.write"));
        assert_eq!(category("Grep"), Some("fs.read"));
        assert_eq!(category("WebFetch"), None);
    }

    #[test]
    fn paths_become_worktree_relative() {
        let (cwd, inside) = if cfg!(windows) {
            (r"C:\repo", r"c:\Repo\src\a.rs")
        } else {
            ("/repo", "/repo/src/a.rs")
        };
        let rel = relative_text(cwd, inside).unwrap();
        assert!(rel.eq_ignore_ascii_case("src/a.rs"), "{rel}");
        assert_eq!(relative_text(cwd, "src/b.rs").as_deref(), Some("src/b.rs"));
        assert_eq!(
            relative_text(cwd, "./src/../src/c.rs").as_deref(),
            Some("src/c.rs")
        );
        let outside = if cfg!(windows) {
            r"C:\other\x.rs"
        } else {
            "/other/x.rs"
        };
        assert_eq!(relative_text(cwd, outside), None);
        // A sibling that merely shares the prefix is outside too.
        let sibling = if cfg!(windows) {
            r"C:\repo2\x.rs"
        } else {
            "/repo2/x.rs"
        };
        assert_eq!(relative_text(cwd, sibling), None);
        assert_eq!(relative_text("", "src/a.rs"), None, "no folder, no inside");
        assert_eq!(
            relative_text(cwd, "src/.."),
            None,
            "the folder is not a file"
        );
        // Case folding never cuts a name in half, whatever it lower-cases to.
        let odd = if cfg!(windows) {
            r"C:\İrepo"
        } else {
            "/İrepo"
        };
        assert_eq!(relative_text(odd, "x.rs").as_deref(), Some("x.rs"));
        assert_eq!(relative_text(cwd, "src/İ.rs").as_deref(), Some("src/İ.rs"));
    }

    /// B3's escape table, now for the agent's own writes: absolute, rooted,
    /// drive-relative, UNC, verbatim and deep `..` climbs are all outside,
    /// and so is whatever a shell or Windows reads differently from the text.
    #[test]
    fn escapes_are_outside() {
        let cwd = if cfg!(windows) { r"C:\repo" } else { "/repo" };
        let mut table = vec![
            r"\foo",
            "/etc/passwd",
            r"\\server\share\x",
            "//server/share/x",
            r"\\?\C:\x",
            r"\\?\C:\repo\x",
            "../../secret",
            "sub/../../..",
            "a/../../../../etc",
            "~/.ssh/authorized_keys",
            "~root/x",
            " src/a.rs",
            "src/a.rs ",
            "",
        ];
        // Drive letters, streams and dropped dots only mean something where
        // Windows reads the path.
        if cfg!(windows) {
            table.extend([
                r"C:\Windows\System32\x",
                "C:foo",
                r"C:\repo\..\secret",
                "src/a.rs:hidden",
                "src/a.rs::$DATA",
                "src/.. /../x",
                "src/a.rs.",
                "src/NUL",
                "src/con.txt",
                "COM1",
                "lpt9.log",
            ]);
        }
        for evil in table {
            assert_eq!(relative_text(cwd, evil), None, "{evil:?} must be outside");
        }
    }

    /// The disk pass: a link inside the folder that points outside it is
    /// outside, one that points inside is leased by its target, and a file
    /// that does not exist yet is judged by the folders that do.
    #[test]
    fn links_are_judged_by_where_they_point() {
        let base = std::env::temp_dir().join(format!("parzi-fence-{}", std::process::id()));
        let (repo, away) = (base.join("repo"), base.join("away"));
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(&away).unwrap();
        let cwd = repo.to_str().unwrap();
        assert_eq!(
            worktree_relative(cwd, "src/new/deep.rs").as_deref(),
            Some("src/new/deep.rs")
        );
        let inside = repo.join("src").join("a.rs");
        assert_eq!(
            worktree_relative(cwd, inside.to_str().unwrap()).as_deref(),
            Some("src/a.rs")
        );
        let linked =
            link_dir(&away, &repo.join("out")) && link_dir(&repo.join("src"), &repo.join("alias"));
        assert!(
            linked,
            "the test needs a directory link (a junction on Windows)"
        );
        assert_eq!(
            worktree_relative(cwd, "out/x.rs"),
            None,
            "a link out is out"
        );
        assert_eq!(
            worktree_relative(cwd, "alias/x.rs").as_deref(),
            Some("src/x.rs"),
            "a link in is leased where it points"
        );
        assert_eq!(worktree_relative("/no/such/folder", "x.rs"), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    fn link_dir(target: &std::path::Path, link: &std::path::Path) -> bool {
        #[cfg(windows)]
        {
            // A junction needs no privilege, unlike a symlink.
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .stdout(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
    }
}
