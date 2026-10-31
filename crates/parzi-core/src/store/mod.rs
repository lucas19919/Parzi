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
use cache::{atomic_write_sync, count_read, count_stat, count_write, parse_lines};
use cache::{IndexEntry, StoreCache};

pub use cache::io_counts;
pub use model::{
    Check, Event, FileTouch, RunCause, RunEndKind, RunOutcome, SessionMeta, SessionStatus, Via,
};
pub use tree::{cascade_kill_ids, subtree_ids};

static META_WRITE: Mutex<()> = Mutex::new(());

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

    /// Files a session's artifacts keep, so they outlive whatever they were made from.
    #[must_use]
    pub fn artifact_dir(&self, id: &str) -> std::path::PathBuf {
        self.dir(id).join("artifacts")
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

    pub fn create(&self, title: &str, project: &str, model: &str) -> Result<SessionMeta> {
        self.create_with_parent(title, project, model, None)
    }

    pub fn create_with_parent(
        &self,
        title: &str,
        project: &str,
        model: &str,
        parent_id: Option<&str>,
    ) -> Result<SessionMeta> {
        let now = Utc::now();
        let meta = SessionMeta {
            id: Uuid::new_v4().to_string(),
            title: title.to_string(),
            project: project.to_string(),
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
            last_run: None,
            plan_item: None,
            branch: String::new(),
            effort: String::new(),
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
        let text = crate::read_settled(&self.dir(id).join("meta.json"))
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
        let mut dirs = vec![];
        for e in std::fs::read_dir(&self.root)?.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let Some(id) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            count_stat(1);
            let dir_mtime = e.metadata().ok().and_then(|m| m.modified().ok());
            dirs.push((id, e.path(), dir_mtime));
        }
        let seen: HashSet<String> = dirs.iter().map(|(id, _, _)| id.clone()).collect();

        let mut out: Vec<SessionMeta> = vec![];
        let mut misses = vec![];
        let writes_before = {
            let c = self.cache();
            for (id, path, dir_mtime) in dirs {
                match c.index.get(&id) {
                    Some(hit) if dir_mtime.is_some() && hit.dir_mtime == dir_mtime => {
                        out.extend(hit.meta.clone());
                    }
                    _ => misses.push((id, path, dir_mtime)),
                }
            }
            c.meta_writes
        };

        let mut fresh = vec![];
        for (id, dir, dir_mtime) in misses {
            let path = dir.join("meta.json");
            count_read(1);
            match std::fs::read_to_string(&path) {
                Ok(text) => match serde_json::from_str::<SessionMeta>(&text) {
                    Ok(m) => fresh.push((id, dir_mtime, Some(m))),
                    Err(err) => {
                        tracing::warn!("unreadable meta.json at {}: {err}", path.display());
                        fresh.push((id, dir_mtime, None));
                    }
                },
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => tracing::warn!("cannot read {}: {err}", path.display()),
            }
        }

        let mut c = self.cache();
        let index = c.meta_writes == writes_before;
        for (id, dir_mtime, meta) in fresh {
            out.extend(meta.clone());
            if index {
                c.index.insert(id, IndexEntry { dir_mtime, meta });
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
        self.update_meta(id, |m| {
            m.cwd = cwd.to_string();
            m.updated = Utc::now();
        })?;
        Ok(())
    }

    pub fn set_parent(&self, id: &str, parent: &str) -> Result<()> {
        self.update_meta(id, |m| {
            m.parent_id = Some(parent.to_string());
            m.updated = Utc::now();
        })?;
        Ok(())
    }

    pub fn set_project(&self, id: &str, project: &str) -> Result<()> {
        self.update_meta(id, |m| {
            m.project = project.to_string();
            m.updated = Utc::now();
        })?;
        self.mark_md_dirty(id);
        Ok(())
    }

    pub fn set_effort(&self, id: &str, effort: &str) -> Result<()> {
        if self.get(id)?.effort != effort {
            self.update_meta(id, |m| m.effort = effort.to_string())?;
        }
        Ok(())
    }

    pub fn set_plan_item(&self, id: &str, item: Option<&str>) -> Result<()> {
        let item = item.map(str::trim).filter(|i| !i.is_empty());
        if self.get(id)?.plan_item.as_deref() != item {
            self.update_meta(id, |m| m.plan_item = item.map(str::to_string))?;
        }
        Ok(())
    }

    pub fn set_last_run(&self, id: &str, outcome: RunOutcome, branch: Option<&str>) -> Result<()> {
        let outcome = outcome.capped();
        self.update_meta(id, |m| {
            m.last_run = Some(outcome);
            if let Some(b) = branch {
                m.branch = b.to_string();
            }
        })?;
        Ok(())
    }

    pub fn events(&self, id: &str) -> Result<Vec<Event>> {
        self.with_events(id, |(all, _)| <[Event]>::to_vec(all))
    }

    pub fn events_with_times(&self, id: &str) -> Result<Vec<(Event, Option<i64>)>> {
        self.with_events(id, |(all, times)| {
            all.iter()
                .zip(times.iter().chain(std::iter::repeat(&None)))
                .map(|(e, t)| (e.clone(), *t))
                .collect()
        })
    }

    pub fn event_count(&self, id: &str) -> Result<usize> {
        self.with_events(id, |(all, _)| <[Event]>::len(all))
    }

    pub fn events_from(&self, id: &str, from: usize) -> Result<Vec<Event>> {
        self.with_events(id, |(all, _)| {
            all.get(from..).map_or_else(Vec::new, <[Event]>::to_vec)
        })
    }

    fn with_events<T>(
        &self,
        id: &str,
        f: impl FnOnce((&[Event], &[Option<i64>])) -> T,
    ) -> Result<T> {
        let path = self.events_path(id);
        let file_len = || {
            count_stat(1);
            std::fs::metadata(&path)
                .map(|m| m.len())
                .map_err(|_| ParziError::Store(format!("session not found: {id}")))
        };
        for _ in 0..3 {
            let len = file_len()?;
            let from = {
                let mut c = self.cache();
                let entry = c.session_mut(id);
                if entry.len == len {
                    return Ok(f((&entry.events, &entry.times)));
                }
                if len < entry.len {
                    entry.events.clear();
                    entry.times.clear();
                    entry.len = 0;
                }
                entry.len
            };
            count_read(1);
            let bytes = read_from(&path, from)?;
            let cut = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
            let parsed = parse_lines(&bytes[..cut], &path);
            let end = from + cut as u64;

            let mut c = self.cache();
            let entry = c.session_mut(id);
            if entry.len > end {
                return Ok(f((&entry.events, &entry.times)));
            }
            if let Some(skip) = entry.len.checked_sub(from) {
                let skip = usize::try_from(skip).unwrap_or(usize::MAX);
                if skip == 0 || bytes.get(skip - 1) == Some(&b'\n') {
                    for (at, e, t) in parsed {
                        if at > skip {
                            entry.events.push(e);
                            entry.times.push(t);
                        }
                    }
                    entry.len = end;
                    return Ok(f((&entry.events, &entry.times)));
                }
            }
        }
        let len = file_len()?;
        let mut c = self.cache();
        let entry = c.session_mut(id);
        if len < entry.len {
            entry.events.clear();
            entry.times.clear();
            entry.len = 0;
        }
        let from = entry.len;
        count_read(1);
        let bytes = read_from(&path, from)?;
        let cut = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        for (_, e, t) in parse_lines(&bytes[..cut], &path) {
            entry.events.push(e);
            entry.times.push(t);
        }
        entry.len = from + cut as u64;
        Ok(f((&entry.events, &entry.times)))
    }

    pub fn append(&self, id: &str, event: &Event) -> Result<()> {
        use std::io::{Read, Seek, SeekFrom, Write};
        let at = Utc::now().timestamp();
        let mut line = serde_json::to_vec(&serde_json::json!({ "at": at, "ev": event }))?;
        line.push(b'\n');
        count_write(2);
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(self.events_path(id))
            .map_err(|_| ParziError::Store(format!("session not found: {id}")))?;
        let known = self.cache().sessions.get(id).map(|s| s.len);
        count_stat(1);
        let size = f.metadata()?.len();
        if size > 0 && known != Some(size) {
            count_read(1);
            let mut last = [0u8; 1];
            f.seek(SeekFrom::End(-1))?;
            f.read_exact(&mut last)?;
            if last[0] != b'\n' {
                line.insert(0, b'\n');
            }
        }
        f.write_all(&line)?;
        let end = f.stream_position().unwrap_or(0);

        let mut c = self.cache();
        let entry = c.session_mut(id);
        if entry.len + line.len() as u64 == end {
            entry.events.push(event.clone());
            entry.times.push(Some(at));
            entry.len = end;
        }
        entry.pending_updated = Some(Utc::now());
        entry.mark_md();
        Ok(())
    }

    pub fn set_model(&self, id: &str, model: &str) -> Result<()> {
        self.update_meta(id, |m| {
            m.model = model.to_string();
            m.updated = Utc::now();
        })?;
        self.mark_md_dirty(id);
        Ok(())
    }

    pub fn set_title(&self, id: &str, title: &str) -> Result<()> {
        self.update_meta(id, |m| {
            m.title = title.chars().take(120).collect();
            m.updated = Utc::now();
        })?;
        self.mark_md_dirty(id);
        Ok(())
    }

    pub fn set_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        self.update_meta(id, |m| {
            m.status = status;
            m.updated = Utc::now();
        })?;
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
        self.update_meta(id, |m| {
            m.tokens_in += tokens_in;
            m.tokens_out += tokens_out;
            m.cost_usd += cost_usd;
            m.updated = Utc::now();
        })?;
        self.mark_md_dirty(id);
        Ok(())
    }

    pub fn set_context(&self, id: &str, tokens: u64, limit: u64) -> Result<()> {
        self.update_meta(id, |m| {
            m.context_tokens = tokens;
            if limit > 0 {
                m.context_limit = limit;
            }
        })?;
        Ok(())
    }

    fn update_meta(&self, id: &str, edit: impl FnOnce(&mut SessionMeta)) -> Result<SessionMeta> {
        let _held = META_WRITE.lock().unwrap_or_else(PoisonError::into_inner);
        let mut meta = self.get(id)?;
        edit(&mut meta);
        self.write_meta(&meta)?;
        Ok(meta)
    }

    fn write_meta(&self, meta: &SessionMeta) -> Result<()> {
        atomic_write_sync(
            &self.dir(&meta.id).join("meta.json"),
            serde_json::to_string_pretty(meta)?.as_bytes(),
        )?;
        let mut c = self.cache();
        if let Some(s) = c.sessions.get_mut(&meta.id) {
            if s.pending_updated.is_some_and(|p| p <= meta.updated) {
                s.pending_updated = None;
            }
        }
        c.index.remove(&meta.id);
        c.meta_writes += 1;
        Ok(())
    }

    fn mark_md_dirty(&self, id: &str) {
        self.cache().session_mut(id).mark_md();
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
