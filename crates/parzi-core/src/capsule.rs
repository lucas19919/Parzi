//! Handoff capsules (PLAN §5): the ~25 lines a finished task leaves behind
//! for the tasks that come after it. A capsule is the only thing one coder
//! ever sees of another — no transcripts cross sessions.
//!
//! Validated like widgets are (`validate` on a `serde_json::Value`), stored
//! at `<project dir>/capsules/<TSK>.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::plan::TaskId;
use crate::project;

/// Caps, so one bad capsule cannot blow up every later lane's context.
const MAX_SUMMARY: usize = 2_000;
const MAX_LIST: usize = 200;
const MAX_ITEM: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffCapsule {
    pub task: TaskId,
    pub summary: String,
    #[serde(default)]
    pub touched_files: Vec<String>,
    #[serde(default)]
    pub exported_symbols: Vec<String>,
    /// What was run and what it said (§15.5) — required, not prose.
    pub verification: String,
    #[serde(default)]
    pub invariants: Vec<String>,
    #[serde(default)]
    pub gotchas: Vec<String>,
}

/// Schema gate for `board.handoff`. Strict where it matters: a task id, a
/// summary and a verification line; tolerant about list order and length
/// only up to the caps above.
pub fn validate(v: &serde_json::Value) -> Result<HandoffCapsule> {
    let c: HandoffCapsule = serde_json::from_value(v.clone())
        .map_err(|e| ParziError::Validation(format!("bad capsule: {e}")))?;
    if c.task.as_str().trim().is_empty() {
        return Err(ParziError::Validation("capsule has no task id".into()));
    }
    if c.summary.trim().is_empty() {
        return Err(ParziError::Validation("capsule summary is empty".into()));
    }
    if c.summary.len() > MAX_SUMMARY {
        return Err(ParziError::Validation(format!(
            "capsule summary {} chars exceeds {MAX_SUMMARY}",
            c.summary.len()
        )));
    }
    if c.verification.trim().is_empty() {
        return Err(ParziError::Validation(
            "capsule verification is required: name the command that proves the task (§15.5)"
                .into(),
        ));
    }
    for (field, list) in [
        ("touched_files", &c.touched_files),
        ("exported_symbols", &c.exported_symbols),
        ("invariants", &c.invariants),
        ("gotchas", &c.gotchas),
    ] {
        if list.len() > MAX_LIST {
            return Err(ParziError::Validation(format!(
                "capsule {field} has {} entries, max {MAX_LIST}",
                list.len()
            )));
        }
        if let Some(bad) = list.iter().find(|s| s.len() > MAX_ITEM) {
            return Err(ParziError::Validation(format!(
                "capsule {field} entry is {} chars, max {MAX_ITEM}",
                bad.len()
            )));
        }
    }
    Ok(c)
}

#[must_use]
pub fn dir(workspace: &str, slug: &str) -> PathBuf {
    project::dir(workspace, slug).join("capsules")
}

/// `<project dir>/capsules/<TSK>.json`. `None` for a task id that is not a
/// plain file name — a capsule never writes outside its project.
#[must_use]
pub fn path(workspace: &str, slug: &str, task: &TaskId) -> Option<PathBuf> {
    let id = task.as_str().trim();
    let safe = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    safe.then(|| dir(workspace, slug).join(format!("{id}.json")))
}

pub fn save(workspace: &str, slug: &str, capsule: &HandoffCapsule) -> Result<()> {
    let path = path(workspace, slug, &capsule.task).ok_or_else(|| {
        ParziError::Validation(format!("bad task id `{}` for a capsule", capsule.task))
    })?;
    let bytes = serde_json::to_vec_pretty(capsule)?;
    crate::atomic_write(&path, &bytes)
}

pub fn load(workspace: &str, slug: &str, task: &TaskId) -> Result<HandoffCapsule> {
    let path = path(workspace, slug, task)
        .ok_or_else(|| ParziError::Validation(format!("bad task id `{task}` for a capsule")))?;
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ParziError::Store(format!("no capsule for {task}"))
        } else {
            ParziError::Io(e)
        }
    })?;
    validate(&serde_json::from_str(&raw)?)
}

/// The capsules a task's `[after:]` list points at, in that order. Missing
/// ones are skipped: a lane must still run when a predecessor left none.
#[must_use]
pub fn for_after(workspace: &str, slug: &str, after: &[TaskId]) -> Vec<HandoffCapsule> {
    after
        .iter()
        .filter_map(|t| load(workspace, slug, t).ok())
        .collect()
}

impl HandoffCapsule {
    /// The ~25 lines a later coder reads. Deterministic, no model.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = format!("### {} — {}\n", self.task, self.summary.trim());
        let sections: [(&str, &Vec<String>); 4] = [
            ("touched", &self.touched_files),
            ("exports", &self.exported_symbols),
            ("invariants", &self.invariants),
            ("gotchas", &self.gotchas),
        ];
        for (label, items) in sections {
            if !items.is_empty() {
                out.push_str(&format!("{label}: {}\n", items.join(", ")));
            }
        }
        out.push_str(&format!("verified: {}\n", self.verification.trim()));
        out
    }
}
