use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::SystemTime;

use chrono::{DateTime, Utc};

use super::model::{Event, SessionMeta};
use crate::error::Result;

static IO_READS: AtomicU64 = AtomicU64::new(0);
static IO_WRITES: AtomicU64 = AtomicU64::new(0);
static IO_STATS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn count_read(n: u64) {
    IO_READS.fetch_add(n, Relaxed);
}
pub(crate) fn count_write(n: u64) {
    IO_WRITES.fetch_add(n, Relaxed);
}
pub(crate) fn count_stat(n: u64) {
    IO_STATS.fetch_add(n, Relaxed);
}

pub fn io_counts() -> (u64, u64, u64) {
    (
        IO_READS.load(Relaxed),
        IO_WRITES.load(Relaxed),
        IO_STATS.load(Relaxed),
    )
}

pub(crate) const MAX_SESSIONS: usize = 16;

#[derive(Debug, Default)]
pub(crate) struct SessionCache {
    pub events: Vec<Event>,
    pub len: u64,
    pub pending_updated: Option<DateTime<Utc>>,
    pub md_dirty: bool,
    /// Bumped on every mark, so a render only clears marks it has seen.
    pub md_seq: u64,
    pub used: u64,
}

impl SessionCache {
    pub fn mark_md(&mut self) {
        self.md_dirty = true;
        self.md_seq += 1;
    }
}

#[derive(Debug)]
pub(crate) struct IndexEntry {
    pub dir_mtime: Option<SystemTime>,
    /// None: meta.json did not parse; remembered so `list()` skips it quietly.
    pub meta: Option<SessionMeta>,
}

#[derive(Debug, Default)]
pub(crate) struct StoreCache {
    pub sessions: HashMap<String, SessionCache>,
    pub index: HashMap<String, IndexEntry>,
    /// Bumped by every meta write; `list()` skips indexing reads it raced.
    pub meta_writes: u64,
    tick: u64,
}

impl StoreCache {
    pub fn forget(&mut self, id: &str) {
        self.sessions.remove(id);
        self.index.remove(id);
    }

    pub fn session_mut(&mut self, id: &str) -> &mut SessionCache {
        self.tick += 1;
        let tick = self.tick;
        if self.sessions.len() >= MAX_SESSIONS && !self.sessions.contains_key(id) {
            self.evict_one();
        }
        let entry = self.sessions.entry(id.to_string()).or_default();
        entry.used = tick;
        entry
    }

    fn evict_one(&mut self) {
        let victim = self
            .sessions
            .iter()
            .filter(|(_, s)| s.pending_updated.is_none())
            .min_by_key(|(_, s)| s.used)
            .map(|(id, _)| id.clone());
        if let Some(id) = victim {
            self.sessions.remove(&id);
        }
    }

    pub fn overlay(&self, meta: &mut SessionMeta) {
        if let Some(p) = self.sessions.get(&meta.id).and_then(|s| s.pending_updated) {
            if p > meta.updated {
                meta.updated = p;
            }
        }
    }
}

pub(crate) fn fold_lines(bytes: &[u8], path: &Path, out: &mut Vec<Event>) {
    out.extend(parse_lines(bytes, path).into_iter().map(|(_, e)| e));
}

/// Events with the byte offset just past their line, so a caller can drop
/// the ones another thread already cached.
pub(crate) fn parse_lines(bytes: &[u8], path: &Path) -> Vec<(usize, Event)> {
    let mut out = vec![];
    let mut end = 0;
    for line in bytes.split(|b| *b == b'\n') {
        end += line.len() + 1;
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<Event>(line) {
            Ok(e) => out.push((end, e)),
            Err(err) => tracing::warn!("skipping unreadable event in {}: {err}", path.display()),
        }
    }
    out
}

/// The shared atomic writer, counted for the bench (tmp write + rename).
pub(crate) fn atomic_write_sync(path: &Path, bytes: &[u8]) -> Result<()> {
    count_write(2);
    crate::error::atomic_write(path, bytes)
}
