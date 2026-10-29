use std::collections::HashSet;

use chrono::Utc;

use super::cache::{count_read, count_write};
use super::{cascade_kill_ids, subtree_ids, SessionMeta, SessionStore};
use crate::error::{ParziError, Result};

impl SessionStore {
    pub fn fork(&self, id: &str, at_step: Option<usize>) -> Result<SessionMeta> {
        let title = format!("{} (fork)", self.get(id)?.title);
        self.fork_as(id, at_step, &title)
    }

    pub fn rewind(&self, id: &str, nth: usize) -> Result<SessionMeta> {
        let src = self.get(id)?;
        count_read(1);
        let text = std::fs::read_to_string(self.events_path(id))
            .map_err(|_| ParziError::Store(format!("session not found: {id}")))?;
        let mut seen = 0;
        let mut keep = None;
        for (i, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
            if is_user_line(line) {
                if seen == nth {
                    keep = Some(i);
                    break;
                }
                seen += 1;
            }
        }
        let keep = keep
            .ok_or_else(|| ParziError::Validation("that message is not in this session".into()))?;
        self.fork_as(id, Some(keep), &src.title)
    }

    fn fork_as(&self, id: &str, at_step: Option<usize>, title: &str) -> Result<SessionMeta> {
        let src = self.get(id)?;
        count_read(1);
        let text = std::fs::read_to_string(self.events_path(id))
            .map_err(|_| ParziError::Store(format!("session not found: {id}")))?;
        let meta = self.create(title, &src.project, &src.model)?;
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
            entry.times.clear();
            entry.len = 0;
            entry.mark_md();
        }
        self.update_meta(&meta.id, |m| {
            m.cwd.clone_from(&src.cwd);
            m.context_limit = src.context_limit;
            m.plan_item.clone_from(&src.plan_item);
            m.branch.clone_from(&src.branch);
            m.effort.clone_from(&src.effort);
            m.updated = Utc::now();
        })?;
        self.flush(&meta.id)?;
        self.get(&meta.id)
    }

    pub fn purge_finished(&self) -> Result<usize> {
        self.purge_finished_ids().map(|ids| ids.len())
    }

    pub fn purge_finished_ids(&self) -> Result<Vec<String>> {
        let all = self.list().unwrap_or_default();
        Ok(self.remove_dirs(cascade_kill_ids(&all)))
    }

    pub fn delete_thread(&self, id: &str) -> Result<usize> {
        self.get(id)?;
        let all = self.list().unwrap_or_default();
        Ok(self.remove_dirs(subtree_ids(&all, id)).len())
    }

    fn remove_dirs(&self, ids: HashSet<String>) -> Vec<String> {
        let mut gone = vec![];
        for id in ids {
            if std::fs::remove_dir_all(self.dir(&id)).is_ok() {
                gone.push(id.clone());
            }
            self.cache().forget(&id);
        }
        gone.sort();
        gone
    }
}

fn is_user_line(line: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    let kind = v.get("kind").or_else(|| v.pointer("/ev/kind"));
    kind.and_then(serde_json::Value::as_str) == Some("user")
}
