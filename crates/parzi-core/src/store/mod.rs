mod cache;
mod maint;
mod model;
mod render;
mod tree;

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::Utc;
use uuid::Uuid;

use crate::error::{ParziError, Result};
use crate::paths;
use cache::{atomic_write_sync, count_read, count_stat, count_write, fold_lines};
use cache::{IndexEntry, StoreCache};

pub use cache::io_counts;
pub use model::{Event, SessionMeta, SessionStatus};
pub use tree::{cascade_kill_ids, subtree_ids};

#[derive(Debug, Clone)]
pub struct SessionStore {
    root: std::path::PathBuf,
    cache: Arc<Mutex<StoreCache>>,
}

impl SessionStore {
    pub fn open() -> Result<Self> {
        let root = paths::sessions_dir()?;
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            cache: Arc::default(),
        })
    }

    fn dir(&self, id: &str) -> std::path::PathBuf {
        if uuid::Uuid::parse_str(id).is_ok() {
            self.root.join(id)
        } else {
            self.root.join("__invalid-session-id__")
        }
    }

    fn events_path(&self, id: &str) -> std::path::PathBuf {
        self.dir(id).join("events.jsonl")
    }

    fn cache(&self) -> MutexGuard<'_, StoreCache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
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
            parent_id: parent_id.map(std::string::ToString::to_string),
            status: SessionStatus::Active,
            pinned: false,
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
            context_tokens: 0,
            context_limit: 0,
            cwd: String::new(),
            created: now,
            updated: now,
        };
        std::fs::create_dir_all(self.dir(&meta.id))?;
        self.write_meta(&meta)?;
        count_write(2);
        std::fs::write(self.events_path(&meta.id), "")?;
        std::fs::write(
            self.dir(&meta.id).join("session.md"),
            render::session_md(&meta, &[]),
        )?;
        Ok(meta)
    }

    pub fn get(&self, id: &str) -> Result<SessionMeta> {
        count_read(1);
        let text = std::fs::read_to_string(self.dir(id).join("meta.json"))
            .map_err(|_| ParziError::Store(format!("session not found: {id}")))?;
        let mut meta: SessionMeta = serde_json::from_str(&text)?;
        self.cache().overlay(&mut meta);
        Ok(meta)
    }

    pub fn resolve_id(&self, id: &str) -> Result<String> {
        let id = id.trim();
        if id.is_empty() {
            return Err(ParziError::Store("a session id is required".into()));
        }
        let rows = self.list()?;
        if let Some(m) = rows.iter().find(|m| m.id == id) {
            return Ok(m.id.clone());
        }
        let mut hits = rows.into_iter().filter(|m| m.id.starts_with(id));
        match (hits.next(), hits.next()) {
            (Some(m), None) => Ok(m.id),
            (Some(_), Some(_)) => Err(ParziError::Store(format!(
                "`{id}` matches more than one session"
            ))),
            (None, _) => Err(ParziError::Store(format!("no session matching `{id}`"))),
        }
    }

    pub fn list(&self) -> Result<Vec<SessionMeta>> {
        count_read(1);
        let entries = std::fs::read_dir(&self.root)?;
        let mut c = self.cache();
        let mut out: Vec<SessionMeta> = vec![];
        let mut seen: HashSet<String> = HashSet::new();
        for e in entries.flatten() {
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let Some(id) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            count_stat(1);
            let dir_mtime = e.metadata().ok().and_then(|m| m.modified().ok());
            seen.insert(id.clone());
            if let Some(hit) = c.index.get(&id) {
                if dir_mtime.is_some() && hit.dir_mtime == dir_mtime {
                    out.push(hit.meta.clone());
                    continue;
                }
            }
            let path = e.path().join("meta.json");
            count_read(1);
            match std::fs::read_to_string(&path) {
                Ok(text) => match serde_json::from_str::<SessionMeta>(&text) {
                    Ok(m) => {
                        c.index.insert(
                            id,
                            IndexEntry {
                                dir_mtime,
                                meta: m.clone(),
                            },
                        );
                        out.push(m);
                    }
                    Err(err) => {
                        tracing::warn!("unreadable meta.json at {}: {err}", path.display());
                    }
                },
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => tracing::warn!("cannot read {}: {err}", path.display()),
            }
        }
        c.index.retain(|k, _| seen.contains(k));
        for m in &mut out {
            c.overlay(m);
        }
        drop(c);
        out.sort_by_key(|m| std::cmp::Reverse(m.updated));
        Ok(out)
    }

    pub fn list_children(&self, parent_id: &str) -> Result<Vec<SessionMeta>> {
        let mut out: Vec<SessionMeta> = self
            .list()?
            .into_iter()
            .filter(|m| m.parent_id.as_deref() == Some(parent_id))
            .collect();
        out.sort_by_key(|m| std::cmp::Reverse(m.updated));
        Ok(out)
    }

    pub fn set_cwd(&self, id: &str, cwd: &str) -> Result<()> {
        let mut meta = self.get(id)?;
        meta.cwd = cwd.to_string();
        meta.updated = Utc::now();
        self.write_meta(&meta)
    }

    pub fn events(&self, id: &str) -> Result<Vec<Event>> {
        let path = self.events_path(id);
        count_stat(1);
        let len = std::fs::metadata(&path)
            .map(|m| m.len())
            .map_err(|_| ParziError::Store(format!("session not found: {id}")))?;
        let mut c = self.cache();
        let entry = c.session_mut(id);
        if entry.len == len {
            return Ok(entry.events.clone());
        }
        if len < entry.len {
            entry.events.clear();
            entry.len = 0;
        }
        let from = entry.len;
        count_read(1);
        let bytes = read_from(&path, from)?;
        let cut = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        fold_lines(&bytes[..cut], &path, &mut entry.events);
        entry.len = from + cut as u64;
        Ok(entry.events.clone())
    }

    pub fn append(&self, id: &str, event: &Event) -> Result<()> {
        use std::io::{Seek, Write};
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');
        count_write(2);
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.events_path(id))
            .map_err(|_| ParziError::Store(format!("session not found: {id}")))?;
        f.write_all(&line)?;
        let end = f.stream_position().unwrap_or(0);

        let mut c = self.cache();
        let entry = c.session_mut(id);
        if entry.len + line.len() as u64 == end {
            entry.events.push(event.clone());
            entry.len = end;
        }
        entry.pending_updated = Some(Utc::now());
        entry.md_dirty = true;
        Ok(())
    }

    pub fn set_model(&self, id: &str, model: &str) -> Result<()> {
        let mut meta = self.get(id)?;
        meta.model = model.to_string();
        meta.updated = Utc::now();
        self.write_meta(&meta)?;
        self.mark_md_dirty(id);
        Ok(())
    }

    pub fn set_title(&self, id: &str, title: &str) -> Result<()> {
        let mut meta = self.get(id)?;
        meta.title = title.chars().take(120).collect();
        meta.updated = Utc::now();
        self.write_meta(&meta)?;
        self.mark_md_dirty(id);
        Ok(())
    }

    pub fn set_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        let mut meta = self.get(id)?;
        meta.status = status;
        meta.updated = Utc::now();
        self.write_meta(&meta)?;
        self.mark_md_dirty(id);
        if status.is_terminal() {
            if let Err(e) = self.transcript_md(id) {
                tracing::warn!("session.md not rendered for {id}: {e}");
            }
        }
        Ok(())
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
        self.write_meta(&meta)?;
        self.mark_md_dirty(id);
        Ok(())
    }

    pub fn set_context(&self, id: &str, tokens: u64, limit: u64) -> Result<()> {
        let mut meta = self.get(id)?;
        meta.context_tokens = tokens;
        if limit > 0 {
            meta.context_limit = limit;
        }
        self.write_meta(&meta)
    }

    fn write_meta(&self, meta: &SessionMeta) -> Result<()> {
        atomic_write_sync(
            &self.dir(&meta.id).join("meta.json"),
            serde_json::to_string_pretty(meta)?.as_bytes(),
        )?;
        let mut c = self.cache();
        if let Some(s) = c.sessions.get_mut(&meta.id) {
            s.pending_updated = None;
        }
        c.index.remove(&meta.id);
        Ok(())
    }

    fn mark_md_dirty(&self, id: &str) {
        self.cache().session_mut(id).md_dirty = true;
    }

    pub fn cached_sessions(&self) -> usize {
        self.cache().sessions.len()
    }
}

fn read_from(path: &std::path::Path, from: u64) -> Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![];
    if from > 0 {
        f.seek(SeekFrom::Start(from))?;
    }
    f.read_to_end(&mut buf)?;
    Ok(buf)
}
