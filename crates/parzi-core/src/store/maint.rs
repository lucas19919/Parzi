use std::collections::HashSet;

use chrono::Utc;

use super::cache::{count_read, count_write};
use super::{cascade_kill_ids, subtree_ids, SessionMeta, SessionStore};
use crate::error::{ParziError, Result};

impl SessionStore {
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
            entry.mark_md();
        }
        // create() starts in a scratch folder; a fork keeps working where its
        // source did. Tokens and cost stay with the source that spent them.
        self.update_meta(&meta.id, |m| {
            m.cwd.clone_from(&src.cwd);
            m.context_limit = src.context_limit;
            m.updated = Utc::now();
        })?;
        self.flush(&meta.id)?;
        self.get(&meta.id)
    }

    pub fn purge_finished(&self) -> Result<usize> {
        let all = self.list().unwrap_or_default();
        Ok(self.remove_dirs(cascade_kill_ids(&all)))
    }

    pub fn delete_thread(&self, id: &str) -> Result<usize> {
        self.get(id)?;
        let all = self.list().unwrap_or_default();
        Ok(self.remove_dirs(subtree_ids(&all, id)))
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
