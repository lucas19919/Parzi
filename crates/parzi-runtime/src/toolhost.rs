use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use parzi_core::context::InterKind;
use parzi_core::error::{ParziError, Result};
use parzi_core::store::{Event, SessionStore};
use parzi_core::{artifacts, brain, widgets};
use parzi_providers::{PermissionDecision, PermissionGate, PermissionRequest};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::handler::{HarnessBridge, RunEvent, RunSink};
use crate::tools::{
    display_name, is_brain_tool, is_browser_tool, is_lane_tool, is_session_tool, is_ui_tool,
    Approval, ApprovalMode, Approver, ToolCallInfo, ToolDef, ToolExecutor,
};

pub struct ToolHostParts {
    pub session_id: String,
    pub lane: String,
    pub mode: ApprovalMode,
    pub edits_auto: bool,
    pub store: SessionStore,
    pub tools: Arc<ToolExecutor>,
    pub approver: Arc<dyn Approver>,
    pub harness: Option<Arc<dyn HarnessBridge>>,
    pub sink: RunSink,
    pub cancel: CancellationToken,
}

pub struct ToolHost {
    p: ToolHostParts,
    defs: tokio::sync::OnceCell<Vec<ToolDef>>,
}

fn category(tool: &str) -> Option<&'static str> {
    match tool {
        "Bash" | "BashOutput" | "KillShell" | "shell" | "execute" => Some("shell.exec"),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "edit" | "delete" | "move" => {
            Some("fs.write")
        }
        "Read" | "Glob" | "Grep" | "LS" | "NotebookRead" | "read" | "search" => Some("fs.read"),
        _ => None,
    }
}

fn worktree_relative(cwd: &str, path: &str) -> Option<String> {
    let rel = relative_text(cwd, path)?;
    let root = std::fs::canonicalize(cwd).ok()?;
    let parts: Vec<&str> = rel.split('/').collect();
    let mut here = root.clone();
    let mut known = 0;
    for seg in &parts {
        let next = here.join(seg);
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

fn strip_prefix_fold<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let mut rest = s.char_indices();
    for want in prefix.chars() {
        let (_, got) = rest.next()?;
        let same = want == got
            || (cfg!(any(windows, target_os = "macos"))
                && want.to_lowercase().eq(got.to_lowercase()));
        if !same {
            return None;
        }
    }
    Some(rest.next().map_or("", |(i, _)| &s[i..]))
}

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

fn execute_brain(name: &str, args: &Value) -> (bool, String) {
    let arg = |k: &str| args.get(k).and_then(Value::as_str).map_or("", str::trim);
    let note = |p: &str| {
        if p.to_ascii_lowercase().ends_with(".md") {
            p.to_string()
        } else {
            format!("{p}.md")
        }
    };
    let vault = match brain::Vault::open() {
        Ok(v) => v,
        Err(e) => return (false, e.to_string()),
    };
    let out = match name {
        "brain.search" if arg("query").is_empty() => {
            return (false, "brain.search needs a `query`".into())
        }
        "brain.search" => {
            let hits = vault.search(arg("query"));
            Ok(if hits.is_empty() {
                format!("no notes match `{}`", arg("query"))
            } else {
                hits.iter()
                    .map(|h| format!("- {} — {}\n  {}", h.path, h.title, h.snippet))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        "brain.read" if arg("path").is_empty() => {
            return (false, "brain.read needs a `path`".into())
        }
        "brain.read" => vault.read(&note(arg("path"))),
        "brain.list" => brain_listing(&vault, arg("project")),
        "brain.write" => match args.get("content").and_then(Value::as_str) {
            Some(content) if !arg("path").is_empty() => vault
                .write(&note(arg("path")), content)
                .map(|m| format!("saved {} ({} bytes)", m.path, m.bytes)),
            _ => return (false, "brain.write needs `path` + `content`".into()),
        },
        _ => return (false, format!("unknown brain tool `{name}`")),
    };
    match out {
        Ok(text) => (true, text),
        Err(e) => (false, e.to_string()),
    }
}

fn note_line(n: &brain::NoteMeta, projects: bool) -> String {
    let mut line = format!("\n- {} — {}", n.path, n.title);
    if projects && !n.projects.is_empty() {
        line.push_str(&format!(" [{}]", n.projects.join(", ")));
    }
    if n.pinned {
        line.push_str(" (pinned)");
    }
    if !n.summary.is_empty() {
        line.push_str(": ");
        line.push_str(&n.summary);
    }
    line
}

fn brain_listing(vault: &brain::Vault, project: &str) -> Result<String> {
    const SHOWN: usize = 300;
    let (notes, projects) = vault.catalog();
    if project.eq_ignore_ascii_case(brain::EVERYWHERE) {
        let mut out = String::from("Notes for every session:");
        let before = out.len();
        for n in notes.iter().filter(|n| n.everywhere()) {
            out.push_str(&note_line(n, false));
        }
        if out.len() == before {
            out.push_str(" none");
        }
        return Ok(out);
    }
    if !project.is_empty() {
        let by_path: HashMap<&str, &brain::NoteMeta> =
            notes.iter().map(|n| (n.path.as_str(), n)).collect();
        let p = projects
            .iter()
            .find(|p| p.slug.eq_ignore_ascii_case(project))
            .ok_or_else(|| {
                ParziError::Validation(format!(
                    "no project `{project}` in the brain; brain.list without a project lists them"
                ))
            })?;
        let mut out = format!(
            "{} ({}), folder {}\n- {} — project note",
            p.title, p.slug, p.folder, p.note
        );
        for n in &p.notes {
            match by_path.get(n.as_str()) {
                Some(meta) => out.push_str(&note_line(meta, false)),
                None => out.push_str(&format!("\n- {n}")),
            }
        }
        return Ok(out);
    }
    if notes.is_empty() {
        return Ok(format!("the brain is empty ({})", vault.root().display()));
    }
    let mut out = String::from("Projects:");
    if projects.is_empty() {
        out.push_str(" none");
    }
    for p in &projects {
        out.push_str(&format!(
            "\n- {} ({}), folder {}, {} notes",
            p.slug,
            p.title,
            p.folder,
            p.notes.len()
        ));
    }
    out.push_str("\n\nNotes:");
    for n in notes.iter().take(SHOWN) {
        out.push_str(&note_line(n, true));
    }
    if notes.len() > SHOWN {
        out.push_str(&format!(
            "\n- …and {} more; use brain.search",
            notes.len() - SHOWN
        ));
    }
    Ok(out)
}

impl ToolHost {
    pub fn new(parts: ToolHostParts) -> Self {
        Self {
            p: parts,
            defs: tokio::sync::OnceCell::new(),
        }
    }

    pub(crate) async fn defs(&self) -> Vec<ToolDef> {
        self.defs
            .get_or_init(|| async { self.p.tools.defs() })
            .await
            .clone()
    }

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
        if is_browser_tool(name) {
            let allowed = if name == "browser.open" {
                self.approved(id, name, args).await
            } else {
                self.p.tools.is_allowed(name)
            };
            if !allowed {
                return (
                    false,
                    format!("tool `{name}` denied (lane mode / approver)"),
                );
            }
            return self.execute_browser(name, args).await;
        }
        if is_brain_tool(name) {
            let allowed = if name == "brain.write" {
                self.approved(id, name, args).await
            } else {
                self.p.tools.is_allowed(name)
            };
            if !allowed {
                return (
                    false,
                    format!("tool `{name}` denied (lane mode / approver)"),
                );
            }
            let (name, args) = (name.to_string(), args.clone());
            return tokio::task::spawn_blocking(move || execute_brain(&name, &args))
                .await
                .unwrap_or_else(|e| (false, e.to_string()));
        }
        if is_session_tool(name) {
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
        self.p.tools.execute(name, args).await
    }

    async fn approved(&self, id: &str, name: &str, args: &Value) -> bool {
        if self.p.mode == ApprovalMode::Deny {
            return false;
        }
        if !self.p.tools.is_allowed(name) {
            return false;
        }
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
            session: self.p.session_id.clone(),
        };
        self.p
            .sink
            .emit(RunEvent::ApprovalRequest { call: info.clone() });
        tokio::select! {
            () = self.p.cancel.cancelled() => false,
            r = self.p.approver.approve(&info) => matches!(r, Approval::Allow),
        }
    }

    async fn gate(&self, req: &PermissionRequest) -> PermissionDecision {
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
        let mut outside = false;
        if kind == Some("fs.write") {
            let targets = write_targets(req);
            outside = targets.is_empty()
                || targets
                    .iter()
                    .any(|p| worktree_relative(&self.p.tools.cwd, p).is_none());
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
        PermissionDecision::Allow
    }

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
        match bridge
            .spawn_session(&self.p.session_id, &title, &prompt, true, None, lane, wait)
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

    async fn execute_browser(&self, name: &str, args: &Value) -> (bool, String) {
        match name {
            "browser.open" => {
                let raw = args
                    .get("url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .trim();
                if raw.is_empty() {
                    return (false, "browser.open needs a url".into());
                }
                let url = crate::desk::normalize_url(raw);
                match crate::desk::call("tab.open", serde_json::json!({ "url": url })).await {
                    Ok(v) => {
                        let shown = v.get("url").and_then(|u| u.as_str()).unwrap_or(&url);
                        (true, format!("opened {shown}"))
                    }
                    Err(e) => (false, e),
                }
            }
            "browser.tabs" => match crate::desk::call("tab.list", serde_json::json!({})).await {
                Ok(v) => (
                    true,
                    v.get("tabs")
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "[]".into()),
                ),
                Err(e) => (false, e),
            },
            "browser.read" => match crate::desk::call("tab.read", serde_json::json!({})).await {
                Ok(v) => {
                    let title = v.get("title").and_then(|t| t.as_str()).unwrap_or("");
                    let url = v.get("url").and_then(|t| t.as_str()).unwrap_or("");
                    if url.is_empty() {
                        (true, "no page is open".into())
                    } else {
                        (true, format!("{title} {url}").trim().to_string())
                    }
                }
                Err(e) => (false, e),
            },
            _ => (false, format!("unknown browser tool `{name}`")),
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
            "ui.show_artifact" => match self.store_artifact(args) {
                Ok(msg) => (true, msg),
                Err(e) => (false, e),
            },
            _ => (false, format!("unknown ui tool `{name}`")),
        }
    }

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
        let odd = if cfg!(windows) {
            r"C:\İrepo"
        } else {
            "/İrepo"
        };
        assert_eq!(relative_text(odd, "x.rs").as_deref(), Some("x.rs"));
        assert_eq!(relative_text(cwd, "src/İ.rs").as_deref(), Some("src/İ.rs"));
    }

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
