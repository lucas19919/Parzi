//! The project directory as the flow sees it: role sessions, drafts,
//! capsules, the journal and the rendered status. Every write goes through
//! `parzi-core` (render/save/atomic_write) — this module only decides which
//! file, never what a file means.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use parzi_core::error::{ParziError, Result};
use parzi_core::journal::{self, JournalLine, Kind};
use parzi_core::plan::{parse_plan_v1, Plan, TaskId};
use parzi_core::{capsule, project, status as status_render};

use crate::roles::Role;

/// Who the journal says did it when the act is the app's, not a person's.
pub const SYSTEM_ACTOR: &str = "parzi";

/// Session ids of a project's role runs, next to PROJECT.md. One header and
/// one orchestrator per project (§1.2); one coder session per lane per sprint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sessions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestrator: Option<String>,
    /// lane name → session id of the coder that is running it.
    #[serde(default)]
    pub lanes: BTreeMap<String, String>,
}

#[must_use]
pub fn sessions_file(workspace: &str, slug: &str) -> PathBuf {
    project::dir(workspace, slug).join("sessions.toml")
}

/// Never fails: a missing or unreadable `sessions.toml` is "no sessions yet",
/// which is exactly what `open` then fixes.
#[must_use]
pub fn sessions(workspace: &str, slug: &str) -> Sessions {
    std::fs::read_to_string(sessions_file(workspace, slug))
        .ok()
        .and_then(|raw| toml::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn set_session(
    workspace: &str,
    slug: &str,
    role: Role,
    lane: &str,
    session_id: &str,
) -> Result<()> {
    let mut s = sessions(workspace, slug);
    match role {
        Role::Header => s.header = Some(session_id.to_string()),
        Role::Orchestrator => s.orchestrator = Some(session_id.to_string()),
        Role::Coder => {
            s.lanes.insert(lane.to_string(), session_id.to_string());
        }
    }
    let text = toml::to_string_pretty(&s)?;
    parzi_core::atomic_write(&sessions_file(workspace, slug), text.as_bytes())
}

// ------------------------------------------------------------------ drafts

/// A rough plan the header wrote. Not PLAN.md and never parsed as one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    /// File name inside `drafts/`, e.g. `1.md`.
    pub name: String,
    pub title: String,
    pub body: String,
}

#[must_use]
pub fn drafts_dir(workspace: &str, slug: &str) -> PathBuf {
    project::dir(workspace, slug).join("drafts")
}

/// Drafts in writing order (`1.md`, `2.md`, …). A missing directory is none.
pub fn drafts(workspace: &str, slug: &str) -> Result<Vec<Draft>> {
    let dir = drafts_dir(workspace, slug);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(vec![]);
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".md"))
        .collect();
    names.sort_by_key(|n| draft_number(n).unwrap_or(u32::MAX));
    let mut out = vec![];
    for name in names {
        let body = std::fs::read_to_string(dir.join(&name))?;
        out.push(Draft {
            title: draft_title(&body).unwrap_or_else(|| name.trim_end_matches(".md").to_string()),
            name,
            body,
        });
    }
    Ok(out)
}

pub fn draft(workspace: &str, slug: &str, name: &str) -> Result<Draft> {
    drafts(workspace, slug)?
        .into_iter()
        .find(|d| d.name == name)
        .ok_or_else(|| ParziError::Store(format!("no draft `{name}`")))
}

/// Write the next numbered draft and return it. The header's only write.
pub fn write_draft(workspace: &str, slug: &str, title: &str, body: &str) -> Result<Draft> {
    if body.trim().is_empty() {
        return Err(ParziError::Validation("a draft cannot be empty".into()));
    }
    if body.len() > 256_000 {
        return Err(ParziError::Validation("draft too large (256k cap)".into()));
    }
    let next = drafts(workspace, slug)?
        .iter()
        .filter_map(|d| draft_number(&d.name))
        .max()
        .map_or(1, |n| n + 1);
    let name = format!("{next}.md");
    let title = title.trim();
    let text = if title.is_empty() || body.trim_start().starts_with("# ") {
        body.to_string()
    } else {
        format!("# {title}\n\n{body}")
    };
    parzi_core::atomic_write(&drafts_dir(workspace, slug).join(&name), text.as_bytes())?;
    Ok(Draft {
        title: draft_title(&text).unwrap_or_else(|| name.trim_end_matches(".md").to_string()),
        name,
        body: text,
    })
}

fn draft_number(name: &str) -> Option<u32> {
    name.trim_end_matches(".md").parse().ok()
}

fn draft_title(body: &str) -> Option<String> {
    body.lines()
        .find(|l| l.trim_start().starts_with("# "))
        .map(|l| l.trim_start().trim_start_matches("# ").trim().to_string())
}

// ------------------------------------------------------------------- plan

#[must_use]
pub fn plan_file(workspace: &str, slug: &str) -> PathBuf {
    project::dir(workspace, slug).join("PLAN.md")
}

/// The project's plan in the hub grammar. A project with no PLAN.md yet has
/// an empty plan — that is a state, not a failure.
pub fn plan(workspace: &str, slug: &str) -> Result<Plan> {
    match std::fs::read_to_string(plan_file(workspace, slug)) {
        Ok(raw) => parse_plan_v1(&raw),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Plan::default()),
        Err(e) => Err(ParziError::Io(e)),
    }
}

/// Write PLAN.md from the parsed plan, so what lands on disk is always what
/// the parser accepted (§15.6: the plan is either the old or the new one).
pub fn write_plan(workspace: &str, slug: &str, plan: &Plan) -> Result<()> {
    let text = parzi_core::plan::render_plan(plan);
    parzi_core::atomic_write(&plan_file(workspace, slug), text.as_bytes())
}

// --------------------------------------------------------------- capsules

/// Task ids that have a capsule, in plan order where the plan knows them and
/// alphabetically for the rest.
#[must_use]
pub fn capsule_ids(workspace: &str, slug: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(capsule::dir(workspace, slug)) else {
        return vec![];
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter_map(|n| n.strip_suffix(".json").map(str::to_string))
        .collect();
    ids.sort();
    ids
}

pub fn capsule(workspace: &str, slug: &str, task: &str) -> Result<capsule::HandoffCapsule> {
    capsule::load(workspace, slug, &TaskId::from(task))
}

// ---------------------------------------------------------------- journal

/// Append one line to `<project dir>/journal.jsonl`. Never `let _ =`: a
/// journal that silently drops a claim is worse than no journal.
pub fn note(
    workspace: &str,
    slug: &str,
    who: &str,
    kind: Kind,
    task: Option<&str>,
    text: &str,
) -> Result<()> {
    journal::append(
        workspace,
        slug,
        &JournalLine::now(who, kind, task.map(TaskId::from), text),
    )
}

/// The last `n` journal lines, oldest first.
#[must_use]
pub fn journal(workspace: &str, slug: &str, n: usize) -> Vec<JournalLine> {
    journal::tail(workspace, slug, n)
}

// ---------------------------------------------------------------- summary

/// The orchestrator's ≤ 15-line summary of the plan it just wrote. Kept next
/// to PLAN.md so the deck can show "what changed" without a model call.
#[must_use]
pub fn summary_file(workspace: &str, slug: &str) -> PathBuf {
    project::dir(workspace, slug).join("PLAN-SUMMARY.md")
}

pub fn write_summary(workspace: &str, slug: &str, text: &str) -> Result<()> {
    let capped: String = text
        .lines()
        .take(SUMMARY_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    parzi_core::atomic_write(&summary_file(workspace, slug), capped.as_bytes())
}

#[must_use]
pub fn read_summary(workspace: &str, slug: &str) -> Option<String> {
    std::fs::read_to_string(summary_file(workspace, slug)).ok()
}

/// §5: "a summary of at most 15 lines" — enforced on write, not requested.
const SUMMARY_LINES: usize = 15;

// ----------------------------------------------------------------- status

/// STATUS.md for a project: deterministic, no model (§15.7). The lease table
/// is the live one when the caller has it; an empty table renders "nobody
/// holds anything", which is the truth for a project nobody is running.
pub fn status_with(
    workspace: &str,
    slug: &str,
    leases: &parzi_core::lease::LeaseTable,
) -> Result<String> {
    let project = project::load(workspace, slug)?;
    let plan = plan(workspace, slug)?;
    let journal = journal(workspace, slug, 40);
    Ok(status_render::render_status(
        &project, &plan, leases, &journal,
    ))
}
