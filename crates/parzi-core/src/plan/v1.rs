//! `PLAN.md` in the hub grammar (PLAN §3): sprints of lanes of tasks.
//!
//! The orchestrator is the only writer (§15.2); everything here is pure, so
//! the same text is what a person reads in the deck and what the runtime
//! dispatches from. Parsing is tolerant of spacing, blank lines and tag
//! order; an unknown grammar major is refused, never guessed (§15.10).

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::project::{strip_version, GRAMMAR_VERSION};

/// Same `parzi: N` line as PROJECT.md — one grammar version for both files.
pub const PLAN_GRAMMAR_VERSION: u32 = GRAMMAR_VERSION;

/// `TSK-7`. A newtype so a task reference cannot be confused with a title.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TaskId(pub String);

impl TaskId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for TaskId {
    fn from(s: &str) -> Self {
        TaskId(s.trim().to_string())
    }
}

impl From<String> for TaskId {
    fn from(s: String) -> Self {
        TaskId(s.trim().to_string())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    /// Which repo of the workspace the task works in.
    #[serde(default)]
    pub repo: String,
    /// Globs, repo-relative: what the lane leases when it checks the task out.
    #[serde(default)]
    pub scope: Vec<String>,
    #[serde(default)]
    pub after: Vec<TaskId>,
    /// Marked `[critical]`: a transfer of its files needs a human (§4).
    #[serde(default)]
    pub critical: bool,
    #[serde(default)]
    pub done: bool,
    /// Indented lines under the task: what "done" means for it (§15.5).
    #[serde(default)]
    pub acceptance: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanePlan {
    pub name: String,
    #[serde(default)]
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sprint {
    pub title: String,
    /// The `(target: 1 day)` suffix, empty when the sprint states none.
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub lanes: Vec<LanePlan>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    #[serde(default)]
    pub sprints: Vec<Sprint>,
}

impl Plan {
    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.sprints
            .iter()
            .flat_map(|s| s.lanes.iter().flat_map(|l| l.tasks.iter()))
    }

    #[must_use]
    pub fn task(&self, id: &TaskId) -> Option<&Task> {
        self.tasks().find(|t| &t.id == id)
    }

    /// Which lane holds a task, for the board and the journal.
    #[must_use]
    pub fn lane_of(&self, id: &TaskId) -> Option<&str> {
        self.sprints.iter().find_map(|s| {
            s.lanes
                .iter()
                .find(|l| l.tasks.iter().any(|t| &t.id == id))
                .map(|l| l.name.as_str())
        })
    }
}

/// Lane a task line lands in when a sprint has tasks before its first
/// `### lane` header.
const IMPLICIT_LANE: &str = "main";

pub fn parse_plan_v1(text: &str) -> Result<Plan> {
    let body = strip_version(text)?;
    let mut plan = Plan::default();
    for line in body.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("### ") {
            push_lane(&mut plan, rest);
            continue;
        }
        if let Some(rest) = t.strip_prefix("## ") {
            plan.sprints.push(parse_sprint_header(rest));
            continue;
        }
        if let Some(task) = parse_task(t) {
            lane_mut(&mut plan).tasks.push(task);
            continue;
        }
        // Indented bullet right under a task: an acceptance line.
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(rest) = t.strip_prefix("- ") {
                let text = rest
                    .trim()
                    .strip_prefix("acceptance:")
                    .unwrap_or(rest.trim())
                    .trim();
                if !text.is_empty() {
                    if let Some(task) = lane_mut(&mut plan).tasks.last_mut() {
                        task.acceptance.push(text.to_string());
                    }
                }
            }
        }
    }
    Ok(plan)
}

fn parse_sprint_header(rest: &str) -> Sprint {
    let rest = rest.trim();
    let (title, target) = match (rest.rfind("(target:"), rest.ends_with(')')) {
        (Some(i), true) => (
            rest[..i].trim(),
            rest[i + "(target:".len()..rest.len() - 1].trim(),
        ),
        _ => (rest, ""),
    };
    Sprint {
        title: title.to_string(),
        target: target.to_string(),
        lanes: vec![],
    }
}

fn push_lane(plan: &mut Plan, header: &str) {
    let name = header
        .trim()
        .strip_prefix("lane ")
        .unwrap_or(header.trim())
        .trim()
        .to_string();
    sprint_mut(plan).lanes.push(LanePlan {
        name,
        tasks: vec![],
    });
}

/// A task line without a sprint header above it still belongs somewhere.
fn sprint_mut(plan: &mut Plan) -> &mut Sprint {
    if plan.sprints.is_empty() {
        plan.sprints.push(Sprint {
            title: "Sprint 1".to_string(),
            target: String::new(),
            lanes: vec![],
        });
    }
    plan.sprints.last_mut().expect("just pushed")
}

fn lane_mut(plan: &mut Plan) -> &mut LanePlan {
    let sprint = sprint_mut(plan);
    if sprint.lanes.is_empty() {
        sprint.lanes.push(LanePlan {
            name: IMPLICIT_LANE.to_string(),
            tasks: vec![],
        });
    }
    sprint.lanes.last_mut().expect("just pushed")
}

fn parse_task(t: &str) -> Option<Task> {
    let (done, rest) = if let Some(r) = t.strip_prefix("- [ ]") {
        (false, r)
    } else if let Some(r) = t.strip_prefix("- [x]").or_else(|| t.strip_prefix("- [X]")) {
        (true, r)
    } else {
        return None;
    };
    let mut task = Task {
        done,
        ..Task::default()
    };
    let mut body = rest.trim().to_string();
    while let Some((tag, stripped)) = take_tag(&body) {
        body = stripped;
        apply_tag(&tag, &mut task);
    }
    let mut words = body.split_whitespace();
    let first = words.next().unwrap_or_default();
    if is_task_id(first) {
        task.id = TaskId(first.to_string());
        task.title = words.collect::<Vec<_>>().join(" ");
    } else {
        task.title = body.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    Some(task)
}

/// `TSK-7`: letters, a dash, digits. Anything else is part of the title.
fn is_task_id(token: &str) -> bool {
    let Some((head, tail)) = token.split_once('-') else {
        return false;
    };
    !head.is_empty()
        && head.chars().all(|c| c.is_ascii_alphabetic())
        && !tail.is_empty()
        && tail.chars().all(|c| c.is_ascii_digit())
}

/// Pull the first `[...]` tag out of the line, returning it and the rest.
fn take_tag(body: &str) -> Option<(String, String)> {
    let start = body.find('[')?;
    let end = body[start..].find(']')? + start;
    let tag = body[start + 1..end].trim().to_string();
    let rest = format!(
        "{} {}",
        body[..start].trim_end(),
        body[end + 1..].trim_start()
    );
    Some((tag, rest.trim().to_string()))
}

fn apply_tag(tag: &str, task: &mut Task) {
    let (key, value) = tag.split_once(':').unwrap_or((tag, ""));
    match key.trim().to_ascii_lowercase().as_str() {
        "repo" => task.repo = value.trim().to_string(),
        "scope" => task.scope = split_list(value),
        "after" => task.after = split_list(value).into_iter().map(TaskId).collect(),
        "critical" => task.critical = true,
        _ => {}
    }
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Render PLAN.md. `parse_plan_v1(render_plan(&p)) == p` for every plan.
#[must_use]
pub fn render_plan(plan: &Plan) -> String {
    let mut out = format!("parzi: {PLAN_GRAMMAR_VERSION}\n");
    for sprint in &plan.sprints {
        out.push_str("\n## ");
        out.push_str(&sprint.title);
        if !sprint.target.is_empty() {
            out.push_str(&format!(" (target: {})", sprint.target));
        }
        out.push('\n');
        for lane in &sprint.lanes {
            out.push_str(&format!("\n### lane {}\n\n", lane.name));
            for task in &lane.tasks {
                out.push_str(&render_task(task));
            }
        }
    }
    out
}

fn render_task(task: &Task) -> String {
    let mut line = format!("- [{}] ", if task.done { "x" } else { " " });
    if !task.id.0.is_empty() {
        line.push_str(&task.id.0);
        line.push_str("  ");
    }
    line.push_str(&task.title);
    if !task.repo.is_empty() {
        line.push_str(&format!(" [repo:{}]", task.repo));
    }
    if !task.scope.is_empty() {
        line.push_str(&format!(" [scope:{}]", task.scope.join(",")));
    }
    if !task.after.is_empty() {
        let after: Vec<&str> = task.after.iter().map(TaskId::as_str).collect();
        line.push_str(&format!(" [after:{}]", after.join(",")));
    }
    if task.critical {
        line.push_str(" [critical]");
    }
    line.push('\n');
    for a in &task.acceptance {
        line.push_str(&format!("  - {a}\n"));
    }
    line
}
