//! Session maintenance: forking a transcript and removing sessions (with
//! their subsession subtrees). Split out of `mod.rs` to keep it readable.

use std::collections::HashSet;

use chrono::Utc;

use super::cache::{count_read, count_write};
use super::{cascade_kill_ids, subtree_ids, SessionMeta, SessionStore};
use crate::error::{ParziError, Result};

impl SessionStore {
    /// Clone transcript up to `at_step` (None = all) into a fresh session.
    pub fn fork(&self, id: &str, at_step: Option<usize>) -> Result<SessionMeta> {
        let src = self.get(id)?;
        count_read(1);
        let text = std::fs::read_to_string(self.events_path(id))
            .map_err(|_| ParziError::Store(format!("session not found: {id}")))?;
        let meta = self.create(
            &format!("{} (fork)", src.title),
            &src.project,
            &src.lane,
            &src.model,
        )?;
        // Lines are copied verbatim: an event kind this build does not know
        // survives the fork instead of being dropped by a re-serialize (C-2).
        let mut kept = String::new();
        for line in text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(at_step.unwrap_or(usize::MAX))
        {
            kept.push_str(line);
            kept.push('\n');
        }
        count_write(1);
        std::fs::write(self.events_path(&meta.id), kept.as_bytes())?;
        {
            let mut c = self.cache();
            let entry = c.session_mut(&meta.id);
            entry.events.clear();
            entry.len = 0;
            entry.md_dirty = true;
            entry.pending_updated = Some(Utc::now());
        }
        self.flush(&meta.id)?;
        self.get(&meta.id)
    }

    /// Delete finished (Done/Killed) sessions. Returns count removed.
    /// Cascade: purging a parent also removes its whole subsession subtree
    /// (children, grandchildren, …), even if a child is still Idle/Active —
    /// a child without its parent is meaningless. Finished child sessions
    /// whose parent survives are purged on their own as before.
    pub fn purge_finished(&self) -> Result<usize> {
        let all = self.list().unwrap_or_default();
        Ok(self.remove_dirs(cascade_kill_ids(&all)))
    }

    /// Delete one session plus its whole subsession subtree at any depth.
    /// Returns the number of session dirs removed. Errors when `id` is unknown.
    pub fn delete_thread(&self, id: &str) -> Result<usize> {
        // Fail loudly on unknown ids so the UI can report, not silently no-op.
        self.get(id)?;
        let all = self.list().unwrap_or_default();
        Ok(self.remove_dirs(subtree_ids(&all, id)))
    }

    /// Delete every session belonging to `project` (plus their subtrees).
    /// Used when a workspace is removed; children are followed even when
    /// they carry a different project tag, so no orphan keeps the name alive.
    pub fn delete_project_threads(&self, project: &str) -> Result<usize> {
        let all = self.list().unwrap_or_default();
        let mut doomed = HashSet::new();
        for m in all.iter().filter(|m| m.project == project) {
            doomed.extend(subtree_ids(&all, &m.id));
        }
        Ok(self.remove_dirs(doomed))
    }

    fn remove_dirs(&self, ids: HashSet<String>) -> usize {
        let mut n = 0;
        for id in ids {
            if std::fs::remove_dir_all(self.dir(&id)).is_ok() {
                n += 1;
            }
            self.cache().forget(&id);
        }
        n
    }
}
