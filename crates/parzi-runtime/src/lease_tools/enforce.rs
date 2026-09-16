//! The write gate (PLAN §4). Every write passes here before the sandbox
//! resolves it: a file another lane holds is a refusal naming the holder, a
//! file outside this lane's own scope is allowed with a notice and a journal
//! line (`lease_mode = "strict"` turns that into a refusal), and a read is
//! never gated at all.
//!
//! `shell.exec` is the one write nobody can resolve in advance, so it is
//! audited instead: `audit_writes` takes the paths the command actually
//! changed (`super::audit`) and holds them to the same rule.

use parzi_core::journal::JournalKind;
use parzi_core::lease::{Holder, Lease};
use parzi_core::project::{self, glob_match, normalize_path};

use crate::handler::RunEvent;

use super::hub::LeaseHub;
use super::naming::{held_by, now, same_holder};

/// How many held files one `shell.exec` violation names before the message
/// stops being readable.
const MAX_NAMED: usize = 10;

impl LeaseHub {
    /// `Some((false, msg))` refuses the write; `None` lets it through. A run
    /// with no repo binding is not in the lease world at all.
    pub async fn gate_write(&self, run: &str, path: &str) -> Option<(bool, String)> {
        let (holder, repo) = {
            let runs = self.runs.lock().await;
            let e = runs.get(run)?;
            (e.holder.clone(), e.repo.clone()?)
        };
        let key = format!("{repo}/{}", normalize_path(path));
        self.heartbeat_run(run).await;
        if let Some(lease) = self.other_holder(&holder, &key).await {
            return Some((
                false,
                format!(
                    "{key} is held by {} — ask with lease.request {{path, for_task, reason}}",
                    held_by(&lease)
                ),
            ));
        }
        // Free path, or this lane's own. It is only scope creep once the lane
        // has checked a task out: before the first claim there is no scope to
        // be outside of.
        let mine = self.leases_of(&holder).await;
        if mine.is_empty() || mine.iter().any(|l| covers(l, &key)) {
            return None;
        }
        let task = mine.first().map(|l| l.task.clone());
        let text = format!("scope_creep: {key} is outside this lane's claimed scope");
        self.journal(run, JournalKind::Note, task.as_ref(), &text)
            .await;
        if self.strict_mode(run).await {
            return Some((false, format!("{text} (workspace lease_mode = strict)")));
        }
        self.notify(run, RunEvent::Notice { text }).await;
        None
    }

    /// The other half of the gate: `shell.exec` has already run, so the check
    /// is on what it changed (`super::audit`). A changed file another lane
    /// holds is always a refusal naming the holder — the same rule as
    /// `fs.write` — and the caller restores those paths from the pre-command
    /// backup. `Some((msg, rels))` is that refusal plus the worktree-relative
    /// paths to put back.
    pub async fn audit_writes(
        &self,
        run: &str,
        changed: &[String],
    ) -> Option<(String, Vec<String>)> {
        let (holder, repo) = {
            let runs = self.runs.lock().await;
            let e = runs.get(run)?;
            (e.holder.clone(), e.repo.clone()?)
        };
        self.heartbeat_run(run).await;
        let mut named: Vec<String> = vec![];
        let mut rels: Vec<String> = vec![];
        for rel in changed {
            let key = format!("{repo}/{}", normalize_path(rel));
            if let Some(lease) = self.other_holder(&holder, &key).await {
                rels.push(rel.clone());
                if named.len() < MAX_NAMED {
                    named.push(format!("{key} (held by {})", held_by(&lease)));
                }
            }
        }
        if named.is_empty() {
            return None;
        }
        let task = self
            .leases_of(&holder)
            .await
            .first()
            .map(|l| l.task.clone());
        let text = format!(
            "lease_violation: shell.exec changed files this lane does not hold: {}",
            named.join(", ")
        );
        self.journal(run, JournalKind::Note, task.as_ref(), &text)
            .await;
        self.notify(run, RunEvent::Notice { text: text.clone() })
            .await;
        Some((text, rels))
    }

    /// Worktree-relative paths another lane currently holds, expanded from
    /// claimed globs against `cwd`. Used to snapshot those files *before*
    /// `shell.exec` so a violation can be undone.
    pub async fn foreign_files(&self, run: &str, cwd: &str) -> Vec<String> {
        let Some((holder, repo)) = ({
            let runs = self.runs.lock().await;
            runs.get(run).map(|e| (e.holder.clone(), e.repo.clone()))
        }) else {
            return vec![];
        };
        let patterns = {
            let mut t = self.table.lock().await;
            t.expire(now());
            t.leases()
                .filter(|l| !same_holder(&l.holder, &holder))
                .flat_map(|l| l.paths.iter().cloned())
                .collect::<Vec<_>>()
        };
        let mut out = Vec::new();
        for p in patterns {
            let rel = strip_repo(&p, repo.as_deref());
            if rel.is_empty() {
                continue;
            }
            if rel.contains(['*', '?']) {
                out.extend(super::audit::files_matching(cwd, &rel));
            } else {
                out.push(rel);
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// The live lease covering `key` when it belongs to another lane — the one
    /// case §4 calls a violation. This lane's own lease is not one.
    async fn other_holder(&self, holder: &Holder, key: &str) -> Option<Lease> {
        self.holder_of_path(key)
            .await
            .filter(|l| !same_holder(&l.holder, holder))
    }

    /// Every live lease this lane holds, expired ones swept first.
    async fn leases_of(&self, holder: &Holder) -> Vec<Lease> {
        let mut t = self.table.lock().await;
        t.expire(now());
        t.leases()
            .filter(|l| same_holder(&l.holder, holder))
            .cloned()
            .collect()
    }

    /// `critical:` globs from the run's PROJECT.md. No project bound means
    /// nothing is critical; an unreadable PROJECT.md is logged, because it
    /// means a transfer that should have asked a human did not.
    pub(super) async fn is_critical(&self, run: &str, path: &str) -> bool {
        let Some(key) = self.project_of_run(run).await else {
            return false;
        };
        match project::load(&key.workspace, &key.slug) {
            Ok(p) => project::is_critical(&p, path),
            Err(e) => {
                tracing::warn!("critical check without PROJECT.md ({}): {e}", key.slug);
                false
            }
        }
    }

    /// `lease_mode = "strict"` in the workspace's own `workspace.toml` turns
    /// the scope-creep notice into a refusal (§4).
    async fn strict_mode(&self, run: &str) -> bool {
        let Some(key) = self.project_of_run(run).await else {
            return false;
        };
        let file = parzi_core::workspace::workspace_file(&key.workspace);
        let Ok(raw) = std::fs::read_to_string(file) else {
            return false;
        };
        raw.parse::<toml::Table>()
            .ok()
            .and_then(|t| {
                t.get("lease_mode")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .is_some_and(|m| m == "strict")
    }
}

/// Does this lease's claimed scope cover `path`? A claim is concrete paths or
/// the globs the plan wrote, so both spellings have to match.
fn covers(lease: &Lease, path: &str) -> bool {
    lease.paths.iter().any(|p| glob_match(p, path))
}

/// Lease keys are `<repo>/<rel>`. The worktree is one repo, so the backup
/// walks `<rel>`. A path that does not start with this run's repo is left
/// alone — it is not in this worktree.
fn strip_repo(path: &str, repo: Option<&str>) -> String {
    let n = normalize_path(path);
    match repo {
        Some(r) if n == r => String::new(),
        Some(r) => n
            .strip_prefix(&format!("{r}/"))
            .map(str::to_string)
            .unwrap_or(n),
        None => n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parzi_core::plan::TaskId;

    #[test]
    fn a_repo_prefix_strips_to_the_worktree_path() {
        assert_eq!(
            strip_repo("api/src/routes.rs", Some("api")),
            "src/routes.rs"
        );
        assert_eq!(strip_repo("api", Some("api")), "");
        assert_eq!(strip_repo("src/routes.rs", None), "src/routes.rs");
        // A path from another repo is not rewritten into this worktree.
        assert_eq!(strip_repo("web/src/app.ts", Some("api")), "web/src/app.ts");
    }

    #[test]
    fn a_claimed_glob_covers_the_files_under_it() {
        let lease = Lease {
            task: TaskId("TSK-8".into()),
            holder: Holder::default(),
            paths: ["api/src/checkout/**".to_string()].into_iter().collect(),
            granted_at: now(),
            last_seen: now(),
            ttl_secs: 90,
        };
        assert!(covers(&lease, "api/src/checkout/session.rs"));
        assert!(!covers(&lease, "api/src/payments/intent.rs"));
    }
}
