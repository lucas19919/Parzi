//! `session.md`: the human-readable view other harnesses can read without
//! tooling. Rendered lazily (on read and at run end), never per append.

use super::cache::count_write;
use super::model::{Event, SessionMeta};
use super::SessionStore;
use crate::error::Result;

/// Whole-session markdown. Pure, so it stays unit-testable and the caller
/// decides when it is worth the write.
pub(crate) fn session_md(meta: &SessionMeta, events: &[Event]) -> String {
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
    for e in events {
        match e {
            Event::System { text } => md.push_str(&format!("> system: {text}\n\n")),
            Event::User { text } => md.push_str(&format!("## user\n\n{text}\n\n")),
            Event::Assistant { text, .. } => md.push_str(&format!("## assistant\n\n{text}\n\n")),
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
            // C-5: the fence is opened *and* closed — everything after the
            // first widget used to live inside one never-ending code block.
            Event::Widget { fence, payload } => {
                md.push_str(&format!(
                    "```{fence}\n{}\n```\n\n",
                    serde_json::to_string_pretty(payload).unwrap_or_default()
                ));
            }
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
    md
}

impl SessionStore {
    /// `session.md`, re-rendered first when the transcript moved on. This is
    /// the read path other harnesses should use instead of the raw file.
    pub fn transcript_md(&self, id: &str) -> Result<String> {
        let meta = self.get(id)?;
        // Strict: a transcript we cannot read must not overwrite the rendered
        // view with an empty one (on Windows a scanner can hold the file).
        let events = self.events(id)?;
        let md = session_md(&meta, &events);
        let path = self.dir(id).join("session.md");
        let dirty = match self.cache().sessions.get(id) {
            Some(s) => s.md_dirty,
            None => true,
        };
        if dirty || !path.exists() {
            count_write(1);
            std::fs::write(&path, md.as_bytes())?;
            if let Some(s) = self.cache().sessions.get_mut(id) {
                s.md_dirty = false;
            }
        }
        Ok(md)
    }

    /// Turn boundary: push the coalesced `updated` stamp and re-render
    /// `session.md`. A no-op when nothing was appended since the last call.
    pub fn flush(&self, id: &str) -> Result<()> {
        let (pending, dirty) = {
            let c = self.cache();
            let s = c.sessions.get(id);
            (
                s.and_then(|s| s.pending_updated),
                s.is_some_and(|s| s.md_dirty),
            )
        };
        if pending.is_some() {
            let meta = self.get(id)?;
            self.write_meta(&meta)?;
        }
        if dirty {
            self.transcript_md(id)?;
        }
        Ok(())
    }
}
