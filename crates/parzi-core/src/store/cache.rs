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
    pub used: u64,
}

#[derive(Debug)]
pub(crate) struct IndexEntry {
    pub dir_mtime: Option<SystemTime>,
    pub meta: SessionMeta,
}

#[derive(Debug, Default)]
pub(crate) struct StoreCache {
    pub sessions: HashMap<String, SessionCache>,
    pub index: HashMap<String, IndexEntry>,
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
    for line in bytes.split(|b| *b == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        match serde_json::from_slice::<Event>(line) {
            Ok(e) => out.push(e),
            Err(err) => tracing::warn!("skipping unreadable event in {}: {err}", path.display()),
        }
    }
}

pub(crate) fn atomic_write_sync(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .map_or("store", |n| n.to_str().unwrap_or("store"));
    let tmp = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Relaxed)
    ));
    count_write(2);
    let write = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(e) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}
