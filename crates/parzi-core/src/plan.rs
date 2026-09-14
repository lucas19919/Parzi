//! Living project plan (`PLAN.md`): milestones, checklist tasks, lane tags.
//! Pure parsing + safe section updates. No I/O except via the helpers here,
//! so the Header/Orchestrator agents and the UI share one implementation.

use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::{lanes, paths};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTask {
    pub title: String,
    pub status: TaskStatus,
    pub lane: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanMilestone {
    pub title: String,
    pub line: usize,
    pub tasks: Vec<PlanTask>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectPlan {
    pub milestones: Vec<PlanTask>,
    pub loose_tasks: Vec<PlanTask>,
    pub raw: String,
}

impl ProjectPlan {
    pub fn milestones_grouped(&self) -> Vec<PlanMilestone> {
        // The flat parser keeps milestone headers as tasks with a `milestone:`
        // lane tag; regroup here for the UI matrix without re-parsing.
        let mut out: Vec<PlanMilestone> = vec![];
        let mut cur: Option<PlanMilestone> = None;
        for t in &self.milestones {
            if t.lane.as_deref() == Some("milestone") {
                if let Some(m) = cur.take() {
                    out.push(m);
                }
                cur = Some(PlanMilestone {
                    title: t.title.clone(),
                    line: t.line,
                    tasks: vec![],
                });
            } else if let Some(m) = cur.as_mut() {
                m.tasks.push(t.clone());
            } else {
                // Tasks before the first milestone header belong to an implicit group.
                cur = Some(PlanMilestone {
                    title: "Plan".into(),
                    line: t.line,
                    tasks: vec![t.clone()],
                });
            }
        }
        if let Some(m) = cur.take() {
            out.push(m);
        }
        out
    }
}

/// Parse `PLAN.md` text: `##` headers become milestones, `- [ ]`/`- [x]`
/// become tasks, `[lane:core]` tags assign lanes, `[wip]`/`(in progress)`
/// marks InProgress.
pub fn parse_plan(text: &str) -> ProjectPlan {
    let mut milestones = vec![];
    let mut loose = vec![];
    let mut in_milestone = false;
    for (idx, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.starts_with("## ") {
            let title = t.trim_start_matches('#').trim().to_string();
            milestones.push(PlanTask {
                title,
                status: TaskStatus::Pending,
                lane: Some("milestone".into()),
                line: idx,
            });
            in_milestone = true;
            continue;
        }
        if let Some(task) = parse_task_line(t, idx) {
            if in_milestone {
                milestones.push(task);
            } else {
                loose.push(task);
            }
        }
    }
    ProjectPlan {
        milestones,
        loose_tasks: loose,
        raw: text.to_string(),
    }
}

fn parse_task_line(t: &str, line: usize) -> Option<PlanTask> {
    let (status, rest) = if let Some(r) = t.strip_prefix("- [ ]") {
        (TaskStatus::Pending, r)
    } else if let Some(r) = t.strip_prefix("- [x]").or_else(|| t.strip_prefix("- [X]")) {
        (TaskStatus::Done, r)
    } else if let Some(r) = t.strip_prefix("- [/]").or_else(|| t.strip_prefix("- [-]")) {
        (TaskStatus::InProgress, r)
    } else {
        return None;
    };
    let mut rest = rest.trim().to_string();
    let mut lane: Option<String> = None;
    // `[lane:core]` tag anywhere in the line.
    if let Some(s) = rest.find("[lane:") {
        let after = &rest[s + 6..];
        if let Some(e) = after.find(']') {
            lane = Some(after[..e].trim().to_string());
            rest = format!("{} {}", rest[..s].trim(), after[e + 1..].trim())
                .trim()
                .to_string();
        }
    }
    let lower = rest.to_lowercase();
    let status = if status == TaskStatus::Pending
        && (lower.contains("[wip]")
            || lower.contains("(in progress)")
            || lower.contains("in_progress"))
    {
        rest = rest
            .replace("[wip]", "")
            .replace("(in progress)", "")
            .trim()
            .to_string();
        TaskStatus::InProgress
    } else {
        status
    };
    Some(PlanTask {
        title: rest,
        status,
        lane,
        line,
    })
}

/// Resolve the living plan path: `<project_root>/PLAN.md` when the project
/// has a disk root, else `~/.parzi/projects/<project>/PLAN.md`.
pub fn plan_path(project: &str) -> Result<std::path::PathBuf> {
    let clean = lanes::safe_name(project)?;
    if let Some(root) = lanes::lane_root(&clean, "") {
        if !root.trim().is_empty() {
            return Ok(std::path::PathBuf::from(root).join("PLAN.md"));
        }
    }
    Ok(paths::projects_dir()?.join(&clean).join("PLAN.md"))
}

pub fn read_plan(project: &str) -> Result<String> {
    let p = plan_path(project)?;
    match std::fs::read_to_string(&p) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(default_plan(project)),
        Err(e) => Err(ParziError::Io(e)),
    }
}

pub fn write_plan(project: &str, content: &str) -> Result<()> {
    let p = plan_path(project)?;
    if content.len() > 512_000 {
        return Err(ParziError::Config("plan too large (512k cap)".into()));
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::atomic_write(&p, content.as_bytes())
}

/// Update one task's checkbox by title match (first hit). Returns true when
/// something changed. Never touches unrelated sections.
pub fn set_task_status(project: &str, title_match: &str, done: bool) -> Result<bool> {
    let raw = read_plan(project)?;
    let needle = title_match.trim().to_lowercase();
    let mut changed = false;
    let mut out = vec![];
    for line in raw.lines() {
        let t = line.trim();
        let is_task = t.starts_with("- [ ]") || t.starts_with("- [x]") || t.starts_with("- [X]");
        if !changed && is_task && t.to_lowercase().contains(&needle) {
            let body = t[5..].trim();
            out.push(format!("- [{}] {}", if done { "x" } else { " " }, body));
            changed = true;
        } else {
            out.push(line.to_string());
        }
    }
    if changed {
        write_plan(project, &out.join("\n"))?;
    }
    Ok(changed)
}

fn default_plan(project: &str) -> String {
    format!(
        "# {project} — living plan\n\n_This file is maintained by the Header agent and the orchestrator. Edit freely; lane workers sync task checkboxes back here._\n\n## Milestone 1\n\n- [ ] Define the first milestone [lane:core]\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_milestones_tasks_and_lanes() {
        let raw = "# P\n\n## Alpha\n\n- [ ] build core [lane:core]\n- [x] ship ui [lane:ui]\n- [/] docs wip\n";
        let p = parse_plan(raw);
        assert_eq!(p.milestones.len(), 4); // header + 3 tasks
        let tasks: Vec<&PlanTask> = p
            .milestones
            .iter()
            .filter(|t| t.lane.as_deref() != Some("milestone"))
            .collect();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].lane.as_deref(), Some("core"));
        assert_eq!(tasks[1].status, TaskStatus::Done);
        assert_eq!(tasks[2].status, TaskStatus::InProgress);
        assert_eq!(p.milestones_grouped().len(), 1);
    }

    #[test]
    fn loose_tasks_before_first_header() {
        let p = parse_plan("- [ ] inbox item\n\n## M\n\n- [ ] a\n");
        assert_eq!(p.loose_tasks.len(), 1);
    }
}
