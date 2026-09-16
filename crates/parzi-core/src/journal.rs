//! The project journal: one append-only JSON line per thing that happened —
//! claims, grants, denials, convenes, handoffs, blocks, plan changes,
//! approvals (PLAN §8 Activity). It is the record the header agent reads and
//! the deck filters; nothing here costs a token to produce.
//!
//! `<project dir>/journal.jsonl`, appended whole-line and fsync'd, like the
//! session store does for events: a crash leaves whole lines or nothing.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::plan::TaskId;
use crate::project;

/// What happened. One flat vocabulary for the whole hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Claim,
    Release,
    Request,
    Grant,
    Deny,
    Convene,
    Handoff,
    Block,
    PlanChanged,
    Approve,
    Note,
}

/// Spelled-out alias for call sites that already say `journal::`.
pub type JournalKind = Kind;

impl Kind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Claim => "claim",
            Kind::Release => "release",
            Kind::Request => "request",
            Kind::Grant => "grant",
            Kind::Deny => "deny",
            Kind::Convene => "convene",
            Kind::Handoff => "handoff",
            Kind::Block => "block",
            Kind::PlanChanged => "plan_changed",
            Kind::Approve => "approve",
            Kind::Note => "note",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalLine {
    pub at: DateTime<Utc>,
    /// The user, or the lane that acted: whoever a person would name.
    pub who: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskId>,
    pub text: String,
}

impl JournalLine {
    /// Stamped now. Timestamps come from here so every writer agrees.
    #[must_use]
    pub fn now(
        who: impl Into<String>,
        kind: Kind,
        task: Option<TaskId>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            at: Utc::now(),
            who: who.into(),
            kind,
            task,
            text: text.into(),
        }
    }
}

#[must_use]
pub fn path(workspace: &str, slug: &str) -> PathBuf {
    project::dir(workspace, slug).join("journal.jsonl")
}

/// One `write_all` of json + newline, then `sync_all`: never a torn line,
/// never a line that is lost when the machine goes down mid-sprint.
pub fn append(workspace: &str, slug: &str, line: &JournalLine) -> Result<()> {
    use std::io::Write;
    let path = path(workspace, slug);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec(line)?;
    bytes.push(b'\n');
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(ParziError::Io)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    Ok(())
}

/// Every readable line, oldest first. A bad line is skipped with a warning
/// instead of bricking the Activity tab (the store's C-2 discipline).
#[must_use]
pub fn read(workspace: &str, slug: &str) -> Vec<JournalLine> {
    let path = path(workspace, slug);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    let mut out = vec![];
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<JournalLine>(line) {
            Ok(l) => out.push(l),
            Err(e) => tracing::warn!(
                "skipping unreadable journal line in {}: {e}",
                path.display()
            ),
        }
    }
    out
}

/// The last `n` lines, oldest first — what STATUS.md and the deck show.
#[must_use]
pub fn tail(workspace: &str, slug: &str, n: usize) -> Vec<JournalLine> {
    let all = read(workspace, slug);
    let start = all.len().saturating_sub(n);
    all[start..].to_vec()
}
