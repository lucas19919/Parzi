use super::cache::atomic_write_sync;
use super::model::{Event, SessionMeta};
use super::SessionStore;
use crate::error::Result;

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
            Event::Error { message, class } => {
                let class = if class.is_empty() {
                    "error"
                } else {
                    class.as_str()
                };
                md.push_str(&format!("> **{class}:** {message}\n\n"));
            }
        }
    }
    md
}

impl SessionStore {
    pub fn transcript_md(&self, id: &str) -> Result<String> {
        // Snapshot the marks before reading, so an append that lands while
        // this renders keeps the flag for the next flush.
        let (dirty, seq) = match self.cache().sessions.get(id) {
            Some(s) => (s.md_dirty, Some(s.md_seq)),
            None => (true, None),
        };
        let meta = self.get(id)?;
        let events = self.events(id)?;
        let md = session_md(&meta, &events);
        let path = self.dir(id).join("session.md");
        if dirty || !path.exists() {
            atomic_write_sync(&path, md.as_bytes())?;
            if let Some(s) = self.cache().sessions.get_mut(id) {
                if Some(s.md_seq) == seq {
                    s.md_dirty = false;
                }
            }
        }
        Ok(md)
    }

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
            self.update_meta(id, |_| {})?;
        }
        if dirty {
            self.transcript_md(id)?;
        }
        Ok(())
    }
}
