use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ParziError, Result};
use crate::{atomic_write, paths};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    #[default]
    Idle,
    Done,
    Killed,
    /// Waiting for a run slot (queue mode). Pumped automatically.
    Queued,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub lane: String,
    #[serde(default)]
    pub model: String,
    /// Hierarchy link for teamwork subsessions. `None` = top-level session.
    /// Backwards-compatible: old `meta.json` files without this key parse as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub status: SessionStatus,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub cwd: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

/// Append-only truth. `session.md` is the rendered human view.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    System {
        text: String,
    },
    User {
        text: String,
    },
    Assistant {
        text: String,
        #[serde(default)]
        done: bool,
    },
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    ToolResult {
        id: String,
        name: String,
        #[serde(default)]
        ok: bool,
        output: String,
        /// Execution time in milliseconds, for the run cards.
        #[serde(default)]
        ms: u128,
    },
    Widget {
        /// Raw fenced payload (`parzi-widget` / `parzi-diagram` JSON).
        fence: String,
        payload: serde_json::Value,
    },
    Artifact {
        id: String,
        title: String,
        artifact_kind: String,
        version: u32,
        payload: serde_json::Value,
    },
    Checkpoint {
        summary: String,
    },
    Reasoning {
        text: String,
    },
    RouteTransition {
        from_provider: String,
        to_provider: String,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cooldown_secs: Option<u64>,
    },
}

#[derive(Debug, Clone)]
pub struct SessionStore {
    root: std::path::PathBuf,
}

impl SessionStore {
    pub fn open() -> Result<Self> {
        let root = paths::sessions_dir()?;
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn dir(&self, id: &str) -> std::path::PathBuf {
        self.root.join(id)
    }

    pub fn create(
        &self,
        title: &str,
        project: &str,
        lane: &str,
        model: &str,
    ) -> Result<SessionMeta> {
        self.create_with_parent(title, project, lane, model, None)
    }

    /// Create a session, optionally nested under a parent session.
    /// `parent_id = Some(..)` makes a subsession; `None` is a top-level session.
    /// Supports arbitrary depth (subsessions may spawn sub-subsessions).
    pub fn create_with_parent(
        &self,
        title: &str,
        project: &str,
        lane: &str,
        model: &str,
        parent_id: Option<&str>,
    ) -> Result<SessionMeta> {
        let now = Utc::now();
        let meta = SessionMeta {
            id: Uuid::new_v4().to_string(),
            title: title.to_string(),
            project: project.to_string(),
            lane: lane.to_string(),
            model: model.to_string(),
            parent_id: parent_id.map(|s| s.to_string()),
            status: SessionStatus::Active,
            pinned: false,
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
            cwd: String::new(),
            created: now,
            updated: now,
        };
        std::fs::create_dir_all(self.dir(&meta.id))?;
        self.write_meta(&meta)?;
        std::fs::write(self.dir(&meta.id).join("events.jsonl"), "")?;
        self.render_md(&meta.id)?;
        Ok(meta)
    }

    pub fn get(&self, id: &str) -> Result<SessionMeta> {
        let text = std::fs::read_to_string(self.dir(id).join("meta.json"))
            .map_err(|_| ParziError::Store(format!("session not found: {id}")))?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Newest first. Reads only meta.json files — never full transcripts.
    pub fn list(&self) -> Result<Vec<SessionMeta>> {
        let mut out = vec![];
        let entries = std::fs::read_dir(&self.root)?;
        for e in entries.flatten() {
            let meta = e.path().join("meta.json");
            if meta.exists() {
                if let Ok(text) = std::fs::read_to_string(&meta) {
                    if let Ok(m) = serde_json::from_str::<SessionMeta>(&text) {
                        out.push(m);
                    }
                }
            }
        }
        out.sort_by_key(|m| std::cmp::Reverse(m.updated));
        Ok(out)
    }

    /// Direct children of a session (one level). For full subtrees, walk this.
    pub fn list_children(&self, parent_id: &str) -> Result<Vec<SessionMeta>> {
        let mut out: Vec<SessionMeta> = self
            .list()?
            .into_iter()
            .filter(|m| m.parent_id.as_deref() == Some(parent_id))
            .collect();
        out.sort_by_key(|m| std::cmp::Reverse(m.updated));
        Ok(out)
    }

    /// Re-parent a session (or detach to top-level with `None`).
    /// Refuses self-parenting and parenting to a missing session.
    pub fn set_parent(&self, id: &str, parent_id: Option<&str>) -> Result<()> {
        if parent_id == Some(id) {
            return Err(ParziError::Store("session cannot be its own parent".into()));
        }
        if let Some(pid) = parent_id {
            // Must exist (and read cleanly) before linking.
            self.get(pid)?;
        }
        let mut meta = self.get(id)?;
        meta.parent_id = parent_id.map(|s| s.to_string());
        meta.updated = Utc::now();
        self.write_meta(&meta)?;
        self.render_md(id)?;
        Ok(())
    }

    pub fn set_cwd(&self, id: &str, cwd: &str) -> Result<()> {
        let mut meta = self.get(id)?;
        meta.cwd = cwd.to_string();
        meta.updated = Utc::now();
        self.write_meta(&meta)
    }

    pub fn events(&self, id: &str) -> Result<Vec<Event>> {
        let text = std::fs::read_to_string(self.dir(id).join("events.jsonl"))
            .map_err(|_| ParziError::Store(format!("session not found: {id}")))?;
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).map_err(ParziError::Json))
            .collect()
    }

    pub fn append(&self, id: &str, event: &Event) -> Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir(id).join("events.jsonl"))
            .map_err(|_| ParziError::Store(format!("session not found: {id}")))?;
        writeln!(f, "{}", serde_json::to_string(event)?)?;
        let mut meta = self.get(id)?;
        meta.updated = Utc::now();
        self.write_meta(&meta)?;
        self.render_md(id)?;
        Ok(())
    }

    pub fn set_model(&self, id: &str, model: &str) -> Result<()> {
        let mut meta = self.get(id)?;
        meta.model = model.to_string();
        meta.updated = Utc::now();
        self.write_meta(&meta)
    }

    pub fn set_title(&self, id: &str, title: &str) -> Result<()> {
        let mut meta = self.get(id)?;
        meta.title = title.chars().take(120).collect();
        meta.updated = Utc::now();
        self.write_meta(&meta)
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        let mut meta = self.get(id)?;
        meta.pinned = pinned;
        meta.updated = Utc::now();
        self.write_meta(&meta)
    }

    pub fn set_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        let mut meta = self.get(id)?;
        meta.status = status;
        meta.updated = Utc::now();
        self.write_meta(&meta)
    }

    pub fn add_usage(
        &self,
        id: &str,
        tokens_in: u64,
        tokens_out: u64,
        cost_usd: f64,
    ) -> Result<()> {
        let mut meta = self.get(id)?;
        meta.tokens_in += tokens_in;
        meta.tokens_out += tokens_out;
        meta.cost_usd += cost_usd;
        meta.updated = Utc::now();
        self.write_meta(&meta)
    }

    /// Clone transcript up to `at_step` (None = all) into a fresh session.
    pub fn fork(&self, id: &str, at_step: Option<usize>) -> Result<SessionMeta> {
        let src = self.get(id)?;
        let events = self.events(id)?;
        let take = at_step.map_or(events.len(), |n| n.min(events.len()));
        let mut meta = self.create(
            &format!("{} (fork)", src.title),
            &src.project,
            &src.lane,
            &src.model,
        )?;
        for e in events.into_iter().take(take) {
            self.append(&meta.id, &e)?;
        }
        meta = self.get(&meta.id)?;
        Ok(meta)
    }

    /// Delete finished (Done/Killed) sessions. Returns count removed.
    /// Cascade: purging a parent also removes its whole subsession subtree
    /// (children, grandchildren, …), even if a child is still Idle/Active —
    /// a child without its parent is meaningless. Finished child sessions
    /// whose parent survives are purged on their own as before.
    pub fn purge_finished(&self) -> Result<usize> {
        let all = self.list().unwrap_or_default();
        let kill = cascade_kill_ids(&all);
        let mut n = 0;
        for id in kill {
            if std::fs::remove_dir_all(self.dir(&id)).is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Delete one session plus its whole subsession subtree at any depth.
    /// Returns the number of session dirs removed. Errors when `id` is unknown.
    pub fn delete_thread(&self, id: &str) -> Result<usize> {
        // Fail loudly on unknown ids so the UI can report, not silently no-op.
        self.get(id)?;
        let all = self.list().unwrap_or_default();
        let ids = subtree_ids(&all, id);
        let mut n = 0;
        for sid in ids {
            if std::fs::remove_dir_all(self.dir(&sid)).is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Delete every session belonging to `project` (plus their subtrees).
    /// Used when a workspace is removed; children are followed even when
    /// they carry a different project tag, so no orphan keeps the name alive.
    pub fn delete_project_threads(&self, project: &str) -> Result<usize> {
        let all = self.list().unwrap_or_default();
        let mut doomed = std::collections::HashSet::new();
        for m in all.iter().filter(|m| m.project == project) {
            for sid in subtree_ids(&all, &m.id) {
                doomed.insert(sid);
            }
        }
        let mut n = 0;
        for sid in doomed {
            if std::fs::remove_dir_all(self.dir(&sid)).is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }

    fn write_meta(&self, meta: &SessionMeta) -> Result<()> {
        atomic_write(
            &self.dir(&meta.id).join("meta.json"),
            serde_json::to_string_pretty(meta)?.as_bytes(),
        )
    }

    /// Render the human-readable view other harnesses can read without tooling.
    fn render_md(&self, id: &str) -> Result<()> {
        let meta = self.get(id)?;
        let events = self.events(id).unwrap_or_default();
        let parent_line = meta
            .parent_id
            .as_deref()
            .map(|p| format!("- parent_id: {p}\n"))
            .unwrap_or_default();
        let mut md = format!(
            "# {}\n\n- id: {}\n{parent_line}- project: {} / lane: {}\n- model: {}\n- status: {:?}\n- tokens: {} in / {} out (${:.4})\n\n---\n\n",
            meta.title, meta.id, meta.project, meta.lane, meta.model,
            meta.status, meta.tokens_in, meta.tokens_out, meta.cost_usd,
        );
        for e in &events {
            match e {
                Event::System { text } => md.push_str(&format!("> system: {text}\n\n")),
                Event::User { text } => md.push_str(&format!("## user\n\n{text}\n\n")),
                Event::Assistant { text, .. } => {
                    md.push_str(&format!("## assistant\n\n{text}\n\n"))
                }
                Event::ToolCall { name, args, .. } => {
                    md.push_str(&format!(
                        "### tool: {name}\n\n```json\n{}\n```\n\n",
                        serde_json::to_string_pretty(args).unwrap_or_default()
                    ));
                }
                Event::ToolResult {
                    name, ok, output, ..
                } => {
                    md.push_str(&format!(
                        "result({name}, ok={ok}):\n\n```\n{output}\n```\n\n"
                    ));
                }
                Event::Widget { fence, .. } => md.push_str(&format!("```{fence}\n\n")),
                Event::Artifact {
                    id,
                    title,
                    artifact_kind,
                    version,
                    payload,
                } => {
                    let content = payload
                        .get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("");
                    let lang = payload
                        .get("language")
                        .and_then(|l| l.as_str())
                        .unwrap_or("");
                    let fence_lang = if artifact_kind == "code" && !lang.is_empty() {
                        lang.to_string()
                    } else {
                        artifact_kind.clone()
                    };
                    md.push_str(&format!(
                        "### artifact: {title} ({id} v{version})\n\n```{fence_lang}\n{content}\n```\n\n"
                    ));
                }
                Event::Checkpoint { summary } => {
                    md.push_str(&format!("> checkpoint: {summary}\n\n"));
                }
                Event::Reasoning { text } => {
                    md.push_str(&format!("> reasoning:\n>\n> {text}\n\n"));
                }
                Event::RouteTransition {
                    from_provider,
                    to_provider,
                    reason,
                    cooldown_secs,
                } => {
                    let cd = cooldown_secs.map_or(String::new(), |s| format!(" (cooldown: {s}s)"));
                    md.push_str(&format!(
                        "> route: {from_provider} -> {to_provider} ({reason}{cd})\n\n"
                    ));
                }
            }
        }
        std::fs::write(self.dir(id).join("session.md"), md)?;
        Ok(())
    }
}

/// Pure subtree computation (kept free of IO so it stays unit-testable):
/// `root` plus all of its descendants at any depth. Unknown roots yield
/// just themselves so callers still remove the dir when listing raced.
pub fn subtree_ids(all: &[SessionMeta], root: &str) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};
    let mut by_parent: HashMap<&str, Vec<&str>> = HashMap::new();
    for m in all {
        if let Some(pid) = &m.parent_id {
            by_parent
                .entry(pid.as_str())
                .or_default()
                .push(m.id.as_str());
        }
    }
    let mut out: HashSet<String> = HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !out.insert(id.to_string()) {
            continue;
        }
        if let Some(kids) = by_parent.get(id) {
            for k in kids {
                stack.push(k);
            }
        }
    }
    out
}

/// Pure cascade-set computation (kept free of IO so it stays unit-testable):
/// every finished session plus all of its descendants at any depth.
pub fn cascade_kill_ids(all: &[SessionMeta]) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};
    let mut by_parent: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut done: HashSet<&str> = HashSet::new();
    for m in all {
        if matches!(m.status, SessionStatus::Done | SessionStatus::Killed) {
            done.insert(m.id.as_str());
        }
        if let Some(pid) = &m.parent_id {
            by_parent
                .entry(pid.as_str())
                .or_default()
                .push(m.id.as_str());
        }
    }
    let mut kill: HashSet<String> = done.iter().map(|s| s.to_string()).collect();
    let mut stack: Vec<&str> = done.into_iter().collect();
    while let Some(id) = stack.pop() {
        if let Some(kids) = by_parent.get(id) {
            for k in kids {
                if kill.insert((*k).to_string()) {
                    stack.push(k);
                }
            }
        }
    }
    kill
}
