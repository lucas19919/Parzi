//! In-memory side of the store: the per-session event cache (E5), the `list()`
//! index (C-6), the durable replace used for `meta.json` (C-5) and the I/O
//! counters the E5 benchmark reads.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::SystemTime;

use chrono::{DateTime, Utc};

use super::model::{Event, SessionMeta};
use crate::error::Result;

// ---------------------------------------------------------------------------
// Test-only I/O accounting (E5 benchmark). Relaxed atomics; nothing reads them
// on the hot path, so the cost is one increment per filesystem call.
// ---------------------------------------------------------------------------
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

/// (file reads, file writes, stats) since process start. Benchmarks only.
pub fn io_counts() -> (u64, u64, u64) {
    (
        IO_READS.load(Relaxed),
        IO_WRITES.load(Relaxed),
        IO_STATS.load(Relaxed),
    )
}

/// How many transcripts stay resident. Everything above this is dropped
/// least-recently-used first; a dropped session costs one re-read of its
/// `events.jsonl`, and nothing else.
pub(crate) const MAX_SESSIONS: usize = 16;

/// One open session, held in memory so a run never re-reads its transcript.
#[derive(Debug, Default)]
pub(crate) struct SessionCache {
    /// Events folded in from `events.jsonl`, in file order.
    pub events: Vec<Event>,
    /// Bytes of `events.jsonl` that `events` covers. A torn tail line is left
    /// out, so the next read picks it up once the writer finished it.
    pub len: u64,
    /// `updated` bump not yet in `meta.json` (writes coalesce to turn ends).
    pub pending_updated: Option<DateTime<Utc>>,
    /// `session.md` is behind `events`.
    pub md_dirty: bool,
    /// LRU stamp: the value of `StoreCache::tick` at the last touch.
    pub used: u64,
}

/// One `meta.json`, valid as long as its session directory keeps its mtime.
#[derive(Debug)]
pub(crate) struct IndexEntry {
    pub dir_mtime: Option<SystemTime>,
    pub meta: SessionMeta,
}

#[derive(Debug, Default)]
pub(crate) struct StoreCache {
    pub sessions: HashMap<String, SessionCache>,
    pub index: HashMap<String, IndexEntry>,
    /// Monotonic counter behind the LRU: two touches in the same microsecond
    /// still order, which an `Instant` does not guarantee.
    tick: u64,
}

impl StoreCache {
    pub fn forget(&mut self, id: &str) {
        self.sessions.remove(id);
        self.index.remove(id);
    }

    /// The entry for `id`, created when new and marked most recently used.
    /// Every insertion goes through here, so `sessions` stays bounded instead
    /// of holding every transcript ever opened.
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

    /// Drop the least recently used session that the files already describe.
    /// An entry with an unflushed `updated` stamp is the cache's only copy of
    /// a mid-turn (live) run, so it stays even if that leaves us over the cap;
    /// `md_dirty` is not a reason to stay, because a missing entry makes
    /// `transcript_md` re-render anyway.
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

    /// The `updated` stamp a reader should see: what is on disk, unless an
    /// append since the last flush moved it forward.
    pub fn overlay(&self, meta: &mut SessionMeta) {
        if let Some(p) = self.sessions.get(&meta.id).and_then(|s| s.pending_updated) {
            if p > meta.updated {
                meta.updated = p;
            }
        }
    }
}

/// C-2: one line at a time, a bad line is skipped with a warning instead of
/// bricking the whole transcript for every later run, fork and render.
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

/// C-5: atomic replace with a tmp name no other writer can collide with (the
/// GUI and the CLI write the same session) and `sync_all`, so a crash leaves
/// either the old file or the new one, never a half-written one.
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
