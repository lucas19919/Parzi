//! `STATUS.md` — "what's happening?" computed, not generated (§15.7).
//!
//! Board + journal in, text out, no model, no clock: the header agent only
//! phrases this, and the same text is the Activity summary in the deck. Any
//! two machines with the same state render the same bytes.

use crate::journal::JournalLine;
use crate::lease::{Lease, LeaseTable};
use crate::plan::Plan;
use crate::project::Project;

/// How many journal lines the tail shows.
const TAIL: usize = 10;

/// "1 file", "2 files" — a person reads this line, not a machine.
fn files(n: usize) -> String {
    if n == 1 {
        "1 file".to_string()
    } else {
        format!("{n} files")
    }
}

/// Timestamps are rendered to the second, in UTC, so the text is stable.
fn stamp(at: chrono::DateTime<chrono::Utc>) -> String {
    at.format("%Y-%m-%d %H:%M:%SZ").to_string()
}

#[must_use]
pub fn render_status(
    project: &Project,
    plan: &Plan,
    leases: &LeaseTable,
    journal: &[JournalLine],
) -> String {
    let mut out = String::new();
    push_project(&mut out, project);
    push_sprints(&mut out, plan);
    push_lanes(&mut out, plan, leases);
    push_requests(&mut out, leases);
    push_journal(&mut out, journal);
    out
}

fn push_project(out: &mut String, p: &Project) {
    out.push_str(&format!("# Status — {}\n\n", p.title));
    out.push_str("## Project\n\n");
    out.push_str(&format!(
        "- {} · workspace {} · status {}\n",
        p.slug,
        p.workspace,
        p.status.as_str()
    ));
    if !p.repos.is_empty() {
        out.push_str(&format!("- repos: {}\n", p.repos.join(", ")));
    }
    if let Some(b) = p.budget_usd {
        out.push_str(&format!("- budget: {b} USD\n"));
    }
    let done = p.what.iter().filter(|c| c.done).count();
    out.push_str(&format!("- acceptance: {done}/{} done\n", p.what.len()));
}

fn push_sprints(out: &mut String, plan: &Plan) {
    out.push_str("\n## Sprint progress\n\n");
    if plan.sprints.is_empty() {
        out.push_str("- no plan yet\n");
        return;
    }
    for sprint in &plan.sprints {
        let tasks: Vec<_> = sprint.lanes.iter().flat_map(|l| l.tasks.iter()).collect();
        let done = tasks.iter().filter(|t| t.done).count();
        let target = if sprint.target.is_empty() {
            String::new()
        } else {
            format!(" (target: {})", sprint.target)
        };
        out.push_str(&format!(
            "- {}{target} — {done}/{} tasks\n",
            sprint.title,
            tasks.len()
        ));
        for lane in &sprint.lanes {
            let lane_done = lane.tasks.iter().filter(|t| t.done).count();
            out.push_str(&format!(
                "  - lane {} — {lane_done}/{} tasks\n",
                lane.name,
                lane.tasks.len()
            ));
        }
    }
}

fn push_lanes(out: &mut String, plan: &Plan, leases: &LeaseTable) {
    out.push_str("\n## Live lanes\n\n");
    let mut any = false;
    for lease in leases.leases() {
        any = true;
        out.push_str(&format!(
            "- lane {} · {} · {} · {} · last seen {}\n",
            lane_name(plan, lease),
            lease.task,
            lease.holder.user,
            files(lease.paths.len()),
            stamp(lease.last_seen)
        ));
    }
    if !any {
        out.push_str("- none\n");
    }
}

/// The plan is the authority on which lane a task belongs to; the lease's own
/// `lane` is the fallback for a task the plan no longer mentions.
fn lane_name(plan: &Plan, lease: &Lease) -> String {
    plan.lane_of(&lease.task)
        .unwrap_or(&lease.holder.lane)
        .to_string()
}

fn push_requests(out: &mut String, leases: &LeaseTable) {
    out.push_str("\n## Pending requests\n\n");
    let mut any = false;
    for req in leases.pending() {
        any = true;
        let held = leases.holder_of(&req.path).map_or_else(
            || "free".to_string(),
            |l| format!("{} · {}", l.holder, l.task),
        );
        out.push_str(&format!(
            "- {} wants {} for {} (held by {held}) — asked {}\n",
            req.from.lane,
            req.path,
            req.for_task,
            stamp(req.at)
        ));
    }
    if !any {
        out.push_str("- none\n");
    }
}

fn push_journal(out: &mut String, journal: &[JournalLine]) {
    out.push_str(&format!("\n## Last {TAIL} journal lines\n\n"));
    let start = journal.len().saturating_sub(TAIL);
    if journal.is_empty() {
        out.push_str("- none\n");
        return;
    }
    for line in &journal[start..] {
        let task = line
            .task
            .as_ref()
            .map_or_else(String::new, |t| format!(" {t}"));
        out.push_str(&format!(
            "- {} {} {}{task}: {}\n",
            stamp(line.at),
            line.kind.as_str(),
            line.who,
            line.text
        ));
    }
}
