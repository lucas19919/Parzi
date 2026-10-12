//! One writer's log folded into state. Pure: the same entries always give
//! the same state, so a snapshot plus the tail equals a full replay.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::graph;
use super::model::{
    AskHow, AskKind, Check, Entry, FileTouch, GoalMark, GoalStatus, Later, PlanStep, RunCause,
    RunEndKind, RunOutcome, StepStatus, WorkEvent,
};

const HOUR: u64 = 60 * 60 * 1000;
const DAY: u64 = 24 * HOUR;
/// A root session's goal takes its next run while this fresh (or running).
pub const CONTINUE_WITHIN: u64 = 6 * HOUR;
/// Done goals leave the view (archived) after this long.
pub const DONE_FOR: u64 = 7 * DAY;
const KEEP_ARCHIVED: u64 = 90 * DAY;
const KEEP_RESOLVED_ASKS: u64 = 7 * DAY;
const KEEP_ORPHAN_RUNS: u64 = 30 * DAY;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub device: String,
    pub name: String,
    pub engine: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub lane: String,
    #[serde(default)]
    pub status: StepStatus,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub sessions: Vec<String>,
    #[serde(default)]
    pub auto: bool,
    /// When the agent last marked it: a run that started before keeps its
    /// hands off the status.
    #[serde(default)]
    pub marked_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub at: u64,
    pub by: String,
    pub text: String,
    #[serde(default)]
    pub step: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub prompt: String,
    pub origin: RunCause,
    pub created: u64,
    pub updated: u64,
    #[serde(default)]
    pub later: Option<Later>,
    #[serde(default)]
    pub continues: Option<String>,
    #[serde(default)]
    pub accepted: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub cancelled: bool,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub notes: Vec<Note>,
    /// Its runs, in the order they started.
    #[serde(default)]
    pub runs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub session: String,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub step: Option<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub lane: String,
    #[serde(default)]
    pub cwd: String,
    pub cause: RunCause,
    pub started: u64,
    #[serde(default)]
    pub ended: Option<u64>,
    #[serde(default)]
    pub outcome: Option<RunOutcome>,
}

impl Run {
    #[must_use]
    pub fn open(&self) -> bool {
        self.ended.is_none()
    }

    #[must_use]
    pub fn end(&self) -> Option<RunEndKind> {
        self.outcome.as_ref().map(|o| o.end)
    }

    #[must_use]
    pub fn files(&self) -> &[FileTouch] {
        self.outcome.as_ref().map_or(&[], |o| o.files.as_slice())
    }

    #[must_use]
    pub fn checks(&self) -> &[Check] {
        self.outcome.as_ref().map_or(&[], |o| o.checks.as_slice())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resolved {
    pub how: AskHow,
    pub by: String,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ask {
    pub id: String,
    pub session: String,
    #[serde(default)]
    pub goal: Option<String>,
    pub kind: AskKind,
    pub label: String,
    #[serde(default)]
    pub detail: Value,
    pub opened: u64,
    #[serde(default)]
    pub expires: Option<u64>,
    #[serde(default)]
    pub resolved: Option<Resolved>,
}

impl Ask {
    #[must_use]
    pub fn open(&self) -> bool {
        self.resolved.is_none()
    }
}

/// Which goal (and step) a session works for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub goal: String,
    #[serde(default)]
    pub step: Option<String>,
}

/// One writer's state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Writer {
    pub w: String,
    pub seq: u64,
    #[serde(default)]
    pub hello: Option<Hello>,
    #[serde(default)]
    pub goals: BTreeMap<String, Goal>,
    #[serde(default)]
    pub runs: BTreeMap<String, Run>,
    #[serde(default)]
    pub asks: BTreeMap<String, Ask>,
    #[serde(default)]
    pub sessions: BTreeMap<String, Binding>,
    #[serde(default)]
    pub open_runs: BTreeSet<String>,
}

/// What one entry changed, for the watchers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Touched {
    pub goals: Vec<String>,
    /// A goal whose root session's run just ended.
    pub settled: Option<String>,
}

impl Writer {
    #[must_use]
    pub fn new(w: &str) -> Self {
        Self {
            w: w.to_string(),
            ..Self::default()
        }
    }

    /// The open run of a session, if one is running.
    #[must_use]
    pub fn open_run(&self, session: &str) -> Option<&Run> {
        self.open_runs
            .iter()
            .filter_map(|id| self.runs.get(id))
            .find(|r| r.session == session)
    }

    /// Whether a new run of a root session joins this goal: not accepted,
    /// cancelled or archived, and running or touched in the last six hours.
    #[must_use]
    pub fn continuable(&self, goal: &Goal, at: u64) -> bool {
        if goal.accepted || goal.cancelled || goal.archived || goal.later.is_some() {
            return false;
        }
        let running = goal
            .runs
            .iter()
            .any(|r| self.open_runs.contains(r.as_str()));
        running || at.saturating_sub(goal.updated) < CONTINUE_WITHIN
    }

    /// The goal a new run of `session` would join at `at`.
    #[must_use]
    pub fn joins(&self, session: &str, at: u64) -> Option<&Binding> {
        let b = self.sessions.get(session)?;
        let goal = self.goals.get(&b.goal)?;
        if goal.archived {
            return None;
        }
        // A step's session always works for its goal; a root only while fresh.
        (b.step.is_some() || self.continuable(goal, at)).then_some(b)
    }

    pub fn apply(&mut self, e: &Entry) -> Touched {
        self.seq = self.seq.max(e.seq);
        let at = e.at;
        let mut t = Touched::default();
        match &e.ev {
            WorkEvent::Unknown => {}
            WorkEvent::Hello {
                device,
                name,
                engine,
            } => {
                self.hello = Some(Hello {
                    device: device.clone(),
                    name: name.clone(),
                    engine: engine.clone(),
                });
            }
            WorkEvent::GoalCreated {
                goal,
                title,
                project,
                root,
                prompt,
                origin,
                later,
                continues,
            } => {
                if self.goals.contains_key(goal) {
                    return t;
                }
                self.goals.insert(
                    goal.clone(),
                    Goal {
                        id: goal.clone(),
                        title: title.clone(),
                        project: project.clone(),
                        root: root.clone(),
                        prompt: prompt.clone(),
                        origin: *origin,
                        created: at,
                        updated: at,
                        later: *later,
                        continues: continues.clone(),
                        accepted: false,
                        archived: false,
                        cancelled: false,
                        steps: vec![],
                        notes: vec![],
                        runs: vec![],
                    },
                );
                if let Some(root) = root {
                    self.bind_root(goal, root);
                }
                t.goals.push(goal.clone());
            }
            WorkEvent::GoalStarted { goal, root } => {
                if let Some(g) = self.goals.get_mut(goal) {
                    g.later = None;
                    g.root = Some(root.clone());
                    g.updated = at;
                    self.bind_root(goal, root);
                    t.goals.push(goal.clone());
                }
            }
            WorkEvent::GoalRetitled { goal, title } => {
                if let Some(g) = self.goals.get_mut(goal) {
                    g.title.clone_from(title);
                    g.updated = at;
                    t.goals.push(goal.clone());
                }
            }
            WorkEvent::GoalMarked { goal, mark } => {
                if let Some(g) = self.goals.get_mut(goal) {
                    match mark {
                        GoalMark::Accepted => g.accepted = true,
                        GoalMark::Reopened => {
                            g.accepted = false;
                            g.cancelled = false;
                            g.archived = false;
                        }
                        GoalMark::Archived => g.archived = true,
                        GoalMark::Unarchived => g.archived = false,
                        GoalMark::Cancelled => g.cancelled = true,
                    }
                    g.updated = at;
                    t.goals.push(goal.clone());
                }
            }
            WorkEvent::LaterChanged { goal, later } => {
                if let Some(g) = self.goals.get_mut(goal) {
                    g.later = *later;
                    g.updated = at;
                    t.goals.push(goal.clone());
                }
            }
            WorkEvent::NoteAdded {
                goal,
                by,
                text,
                step,
            } => {
                if let Some(g) = self.goals.get_mut(goal) {
                    g.notes.push(Note {
                        at,
                        by: by.clone(),
                        text: text.clone(),
                        step: step.clone(),
                    });
                    g.updated = at;
                    t.goals.push(goal.clone());
                }
            }
            WorkEvent::PlanSet { goal, steps } => {
                if let Some(g) = self.goals.get_mut(goal) {
                    g.steps = merge_plan(std::mem::take(&mut g.steps), steps);
                    g.updated = at;
                    t.goals.push(goal.clone());
                }
            }
            WorkEvent::StepAdded { goal, step, auto } => {
                if let Some(g) = self.goals.get_mut(goal) {
                    if !g.steps.iter().any(|s| s.key == step.key) {
                        g.steps.push(Step {
                            key: step.key.clone(),
                            title: step.title.clone(),
                            needs: step.needs.clone(),
                            lane: step.lane.clone(),
                            auto: *auto,
                            ..Step::default()
                        });
                        g.steps = graph::sanitize(std::mem::take(&mut g.steps));
                    }
                    g.updated = at;
                    t.goals.push(goal.clone());
                }
            }
            WorkEvent::StepMarked {
                goal,
                step,
                status,
                note,
                outcome,
            } => {
                if let Some(g) = self.goals.get_mut(goal) {
                    if let Some(s) = g.steps.iter_mut().find(|s| &s.key == step) {
                        s.status = *status;
                        if note.is_some() {
                            s.note.clone_from(note);
                        }
                        if outcome.is_some() {
                            s.outcome.clone_from(outcome);
                        }
                        s.marked_at = Some(at);
                    }
                    g.updated = at;
                    t.goals.push(goal.clone());
                }
            }
            WorkEvent::StepLinked {
                goal,
                step,
                session,
            } => {
                if let Some(g) = self.goals.get_mut(goal) {
                    if let Some(s) = g.steps.iter_mut().find(|s| &s.key == step) {
                        if !s.sessions.contains(session) {
                            s.sessions.push(session.clone());
                        }
                    }
                    g.updated = at;
                    self.sessions.insert(
                        session.clone(),
                        Binding {
                            goal: goal.clone(),
                            step: Some(step.clone()),
                        },
                    );
                    t.goals.push(goal.clone());
                }
            }
            WorkEvent::RunStarted {
                run,
                session,
                model,
                lane,
                cwd,
                cause,
            } => {
                if self.runs.contains_key(run) {
                    return t;
                }
                let joined = self.joins(session, at).cloned();
                self.runs.insert(
                    run.clone(),
                    Run {
                        id: run.clone(),
                        session: session.clone(),
                        goal: None,
                        step: None,
                        model: model.clone(),
                        lane: lane.clone(),
                        cwd: cwd.clone(),
                        cause: *cause,
                        started: at,
                        ended: None,
                        outcome: None,
                    },
                );
                self.open_runs.insert(run.clone());
                if let Some(b) = joined {
                    self.attach(run, &b, at);
                    t.goals.push(b.goal);
                }
            }
            WorkEvent::RunEnded { run, outcome } => {
                let Some(r) = self.runs.get_mut(run) else {
                    return t;
                };
                if r.ended.is_some() {
                    return t;
                }
                r.ended = Some(at);
                r.outcome = Some(outcome.clone());
                let (goal, step, session, started) =
                    (r.goal.clone(), r.step.clone(), r.session.clone(), r.started);
                self.open_runs.remove(run);
                if let Some(gid) = goal {
                    if let Some(g) = self.goals.get_mut(&gid) {
                        g.updated = at;
                        if let Some(s) = step
                            .as_ref()
                            .and_then(|k| g.steps.iter_mut().find(|s| &s.key == k))
                        {
                            // The agent's own mark during this run stands.
                            if s.marked_at.is_none_or(|m| m < started) {
                                s.status = step_after(outcome.end);
                            }
                        }
                        if g.root.as_deref() == Some(session.as_str()) {
                            t.settled = Some(gid.clone());
                        }
                        t.goals.push(gid);
                    }
                }
            }
            WorkEvent::AskOpened {
                ask,
                session,
                kind,
                label,
                detail,
                expires,
            } => {
                if self.asks.contains_key(ask) {
                    return t;
                }
                let goal = self.sessions.get(session).map(|b| b.goal.clone());
                if let Some(g) = goal.as_ref().and_then(|g| self.goals.get_mut(g)) {
                    g.updated = at;
                    t.goals.push(g.id.clone());
                }
                self.asks.insert(
                    ask.clone(),
                    Ask {
                        id: ask.clone(),
                        session: session.clone(),
                        goal,
                        kind: *kind,
                        label: label.clone(),
                        detail: detail.clone(),
                        opened: at,
                        expires: *expires,
                        resolved: None,
                    },
                );
            }
            WorkEvent::AskResolved { ask, how, by } => {
                if let Some(a) = self.asks.get_mut(ask) {
                    if a.resolved.is_none() {
                        a.resolved = Some(Resolved {
                            how: *how,
                            by: by.clone(),
                            at,
                        });
                        if let Some(g) = a.goal.as_ref().and_then(|g| self.goals.get_mut(g)) {
                            g.updated = at;
                            t.goals.push(g.id.clone());
                        }
                    }
                }
            }
        }
        t
    }

    /// `session` leads `goal`; its open runs that belong to no goal yet
    /// were the start of it.
    fn bind_root(&mut self, goal: &str, session: &str) {
        self.sessions.insert(
            session.to_string(),
            Binding {
                goal: goal.to_string(),
                step: None,
            },
        );
        let orphans: Vec<String> = self
            .open_runs
            .iter()
            .filter(|id| {
                self.runs
                    .get(id.as_str())
                    .is_some_and(|r| r.session == session && r.goal.is_none())
            })
            .cloned()
            .collect();
        let b = Binding {
            goal: goal.to_string(),
            step: None,
        };
        for run in orphans {
            let at = self.runs.get(&run).map_or(0, |r| r.started);
            self.attach(&run, &b, at);
        }
    }

    fn attach(&mut self, run: &str, b: &Binding, at: u64) {
        let Some(r) = self.runs.get_mut(run) else {
            return;
        };
        r.goal = Some(b.goal.clone());
        r.step.clone_from(&b.step);
        let Some(g) = self.goals.get_mut(&b.goal) else {
            return;
        };
        if !g.runs.iter().any(|x| x == run) {
            g.runs.push(run.to_string());
        }
        g.updated = g.updated.max(at);
        if let Some(s) = b
            .step
            .as_ref()
            .and_then(|k| g.steps.iter_mut().find(|s| &s.key == k))
        {
            s.status = StepStatus::Running;
            if let Some(sid) = Some(r.session.clone()).filter(|x| !s.sessions.contains(x)) {
                s.sessions.push(sid);
            }
        }
    }

    /// The runs of a goal, oldest first.
    #[must_use]
    pub fn goal_runs(&self, goal: &Goal) -> Vec<&Run> {
        goal.runs.iter().filter_map(|r| self.runs.get(r)).collect()
    }

    /// Open asks on a goal's sessions.
    pub fn goal_asks<'a>(&'a self, goal: &'a str) -> impl Iterator<Item = &'a Ask> + 'a {
        self.asks
            .values()
            .filter(move |a| a.open() && a.goal.as_deref() == Some(goal))
    }

    /// Where a goal stands, in the order a person cares: archived, not
    /// started, waiting on them, working, paused, then how its lead's last
    /// run ended. A done goal archives itself after a week.
    #[must_use]
    pub fn status(&self, goal: &Goal, now: u64) -> GoalStatus {
        if goal.archived {
            return GoalStatus::Archived;
        }
        if goal.later.is_some() {
            return GoalStatus::Later;
        }
        let asks: Vec<&Ask> = self.goal_asks(&goal.id).collect();
        if asks.iter().any(|a| a.kind != AskKind::Budget) {
            return GoalStatus::NeedsYou;
        }
        let runs = self.goal_runs(goal);
        if runs.iter().any(|r| r.open()) {
            return GoalStatus::Working;
        }
        let settled = if goal.accepted {
            GoalStatus::Done
        } else if goal.cancelled {
            GoalStatus::Stopped
        } else if !asks.is_empty() {
            GoalStatus::Paused
        } else if goal.steps.iter().any(|s| s.status == StepStatus::Check) {
            GoalStatus::Check
        } else {
            let lead = runs
                .iter()
                .rev()
                .find(|r| Some(r.session.as_str()) == goal.root.as_deref())
                .or_else(|| runs.last());
            match lead {
                None => GoalStatus::Stopped,
                Some(r) => match r.end() {
                    Some(RunEndKind::Completed) if failed_checks(r) => GoalStatus::Check,
                    Some(RunEndKind::Completed) => GoalStatus::Done,
                    Some(RunEndKind::Paused) => GoalStatus::Paused,
                    Some(RunEndKind::Stopped) => GoalStatus::Stopped,
                    Some(RunEndKind::Interrupted) => GoalStatus::CutOff,
                    Some(RunEndKind::Failed | RunEndKind::Crashed | RunEndKind::NotStarted)
                    | None => GoalStatus::Failed,
                },
            }
        };
        let quiet = matches!(settled, GoalStatus::Done | GoalStatus::Stopped);
        if quiet && now.saturating_sub(goal.updated) >= DONE_FOR {
            return GoalStatus::Archived;
        }
        settled
    }

    /// Drops what nobody will look at again: archived goals after 90 days,
    /// answered asks after 7, runs that never joined a goal after 30.
    pub fn retain(&mut self, now: u64) {
        let gone: Vec<String> = self
            .goals
            .values()
            .filter(|g| {
                self.status(g, now) == GoalStatus::Archived
                    && now.saturating_sub(g.updated) >= KEEP_ARCHIVED
            })
            .map(|g| g.id.clone())
            .collect();
        for id in &gone {
            if let Some(g) = self.goals.remove(id) {
                for r in &g.runs {
                    self.runs.remove(r);
                    self.open_runs.remove(r);
                }
            }
        }
        self.sessions.retain(|_, b| !gone.contains(&b.goal));
        self.asks.retain(|_, a| {
            let lost = a.goal.as_ref().is_some_and(|g| gone.contains(g));
            let old = a
                .resolved
                .as_ref()
                .is_some_and(|r| now.saturating_sub(r.at) >= KEEP_RESOLVED_ASKS);
            !lost && !old
        });
        self.runs.retain(|_, r| {
            r.goal.is_some()
                || r.ended
                    .is_none_or(|e| now.saturating_sub(e) < KEEP_ORPHAN_RUNS)
        });
    }
}

/// A step follows its child's run end.
fn step_after(end: RunEndKind) -> StepStatus {
    match end {
        RunEndKind::Completed => StepStatus::Done,
        RunEndKind::Failed | RunEndKind::Crashed => StepStatus::Failed,
        RunEndKind::Paused
        | RunEndKind::Stopped
        | RunEndKind::Interrupted
        | RunEndKind::NotStarted => StepStatus::Todo,
    }
}

/// The latest result of each check command, any of them failed.
fn failed_checks(r: &Run) -> bool {
    let mut last: BTreeMap<&str, bool> = BTreeMap::new();
    for c in r.checks() {
        last.insert(c.cmd.as_str(), c.ok);
    }
    last.values().any(|ok| !ok)
}

/// A new plan over the old steps: kept keys keep their status and sessions,
/// todo steps the plan leaves out drop, started or finished ones stay.
fn merge_plan(old: Vec<Step>, plan: &[PlanStep]) -> Vec<Step> {
    let mut out: Vec<Step> = plan
        .iter()
        .map(|p| match old.iter().find(|s| s.key == p.key) {
            Some(s) => Step {
                title: p.title.clone(),
                needs: p.needs.clone(),
                lane: p.lane.clone(),
                ..s.clone()
            },
            None => Step {
                key: p.key.clone(),
                title: p.title.clone(),
                needs: p.needs.clone(),
                lane: p.lane.clone(),
                ..Step::default()
            },
        })
        .collect();
    for s in old {
        if s.status != StepStatus::Todo && !out.iter().any(|o| o.key == s.key) {
            out.push(s);
        }
    }
    graph::sanitize(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::model::{FileTouch, Via};

    struct Log {
        w: Writer,
        seq: u64,
        at: u64,
    }

    impl Log {
        fn new() -> Self {
            Self {
                w: Writer::new("pc-ab12"),
                seq: 0,
                at: 1_000_000,
            }
        }

        fn at(&mut self, ms: u64) -> &mut Self {
            self.at = ms;
            self
        }

        fn add(&mut self, ev: WorkEvent) -> Touched {
            self.seq += 1;
            let e = Entry {
                w: self.w.w.clone(),
                seq: self.seq,
                at: self.at,
                ev,
            };
            self.w.apply(&e)
        }
    }

    fn started(run: &str, session: &str) -> WorkEvent {
        WorkEvent::RunStarted {
            run: run.into(),
            session: session.into(),
            model: "claude/m".into(),
            lane: "build".into(),
            cwd: String::new(),
            cause: RunCause::Send,
        }
    }

    fn ended(run: &str, end: RunEndKind) -> WorkEvent {
        WorkEvent::RunEnded {
            run: run.into(),
            outcome: RunOutcome::bare(end),
        }
    }

    fn created(goal: &str, root: &str) -> WorkEvent {
        WorkEvent::GoalCreated {
            goal: goal.into(),
            title: "Fix the omnibar".into(),
            project: Some("parzi".into()),
            root: Some(root.into()),
            prompt: "fix the omnibar".into(),
            origin: RunCause::Send,
            later: None,
            continues: None,
        }
    }

    #[test]
    fn a_run_is_no_goal_until_it_does_real_work() {
        let mut l = Log::new();
        l.add(started("r1", "s1"));
        assert!(l.w.goals.is_empty(), "a run alone makes no goal");
        assert_eq!(l.w.runs["r1"].goal, None);
        let t = l.add(created("g1", "s1"));
        assert_eq!(t.goals, vec!["g1".to_string()]);
        assert_eq!(
            l.w.runs["r1"].goal.as_deref(),
            Some("g1"),
            "the run that qualified joins"
        );
        assert_eq!(l.w.goals["g1"].runs, vec!["r1".to_string()]);
        assert_eq!(l.w.status(&l.w.goals["g1"], l.at), GoalStatus::Working);
        let t = l.add(ended("r1", RunEndKind::Completed));
        assert_eq!(t.settled.as_deref(), Some("g1"), "its lead ended");
        assert_eq!(l.w.status(&l.w.goals["g1"], l.at), GoalStatus::Done);
    }

    #[test]
    fn the_next_run_joins_while_the_goal_is_fresh() {
        let mut l = Log::new();
        l.add(started("r1", "s1"));
        l.add(created("g1", "s1"));
        l.add(ended("r1", RunEndKind::Completed));
        let t0 = l.at;
        l.at(t0 + CONTINUE_WITHIN - 1).add(started("r2", "s1"));
        assert_eq!(l.w.runs["r2"].goal.as_deref(), Some("g1"), "within 6 h");
        l.add(ended("r2", RunEndKind::Completed));
        let t1 = l.at;
        l.at(t1 + CONTINUE_WITHIN).add(started("r3", "s1"));
        assert_eq!(l.w.runs["r3"].goal, None, "a stale goal is not continued");
        l.add(ended("r3", RunEndKind::Completed));
        l.add(WorkEvent::GoalMarked {
            goal: "g1".into(),
            mark: GoalMark::Accepted,
        });
        l.add(started("r4", "s1"));
        assert_eq!(
            l.w.runs["r4"].goal, None,
            "an accepted goal is not continued"
        );
    }

    #[test]
    fn a_long_running_goal_is_continued_however_old() {
        let mut l = Log::new();
        l.add(started("r1", "s1"));
        l.add(created("g1", "s1"));
        l.add(WorkEvent::StepAdded {
            goal: "g1".into(),
            step: PlanStep {
                key: "a".into(),
                title: "child".into(),
                needs: vec![],
                lane: String::new(),
            },
            auto: true,
        });
        l.add(WorkEvent::StepLinked {
            goal: "g1".into(),
            step: "a".into(),
            session: "c1".into(),
        });
        l.add(started("rc", "c1"));
        assert_eq!(l.w.goals["g1"].steps[0].status, StepStatus::Running);
        l.add(ended("r1", RunEndKind::Completed));
        let t0 = l.at;
        l.at(t0 + 3 * CONTINUE_WITHIN).add(started("r2", "s1"));
        assert_eq!(
            l.w.runs["r2"].goal.as_deref(),
            Some("g1"),
            "a step still runs"
        );
        l.add(ended("rc", RunEndKind::Failed));
        assert_eq!(l.w.goals["g1"].steps[0].status, StepStatus::Failed);
    }

    #[test]
    fn a_restart_cuts_open_runs_off() {
        let mut l = Log::new();
        l.add(started("r1", "s1"));
        l.add(created("g1", "s1"));
        l.add(WorkEvent::AskOpened {
            ask: "k1".into(),
            session: "s1".into(),
            kind: AskKind::Approval,
            label: "Run cargo test".into(),
            detail: Value::Null,
            expires: None,
        });
        assert_eq!(l.w.status(&l.w.goals["g1"], l.at), GoalStatus::NeedsYou);
        l.add(ended("r1", RunEndKind::Interrupted));
        l.add(WorkEvent::AskResolved {
            ask: "k1".into(),
            how: AskHow::Expired,
            by: "parzi".into(),
        });
        assert!(l.w.open_runs.is_empty());
        assert_eq!(l.w.status(&l.w.goals["g1"], l.at), GoalStatus::CutOff);
    }

    #[test]
    fn statuses_follow_pauses_and_checks() {
        let mut l = Log::new();
        l.add(started("r1", "s1"));
        l.add(created("g1", "s1"));
        l.add(WorkEvent::AskOpened {
            ask: "b1".into(),
            session: "s1".into(),
            kind: AskKind::Budget,
            label: "Paused: token budget".into(),
            detail: Value::Null,
            expires: None,
        });
        l.add(ended("r1", RunEndKind::Paused));
        assert_eq!(l.w.status(&l.w.goals["g1"], l.at), GoalStatus::Paused);
        l.add(WorkEvent::AskResolved {
            ask: "b1".into(),
            how: AskHow::Continued,
            by: "you".into(),
        });
        l.add(started("r2", "s1"));
        let mut outcome = RunOutcome::bare(RunEndKind::Completed);
        outcome.checks = vec![
            Check {
                cmd: "cargo test".into(),
                ok: false,
            },
            Check {
                cmd: "cargo test".into(),
                ok: true,
            },
            Check {
                cmd: "npm test".into(),
                ok: false,
            },
        ];
        outcome.files = vec![FileTouch {
            path: "a.rs".into(),
            via: Via::Tool,
        }];
        l.add(WorkEvent::RunEnded {
            run: "r2".into(),
            outcome,
        });
        assert_eq!(
            l.w.status(&l.w.goals["g1"], l.at),
            GoalStatus::Check,
            "npm test failed last"
        );
        l.add(WorkEvent::GoalMarked {
            goal: "g1".into(),
            mark: GoalMark::Accepted,
        });
        assert_eq!(l.w.status(&l.w.goals["g1"], l.at), GoalStatus::Done);
        assert_eq!(
            l.w.status(&l.w.goals["g1"], l.at + DONE_FOR),
            GoalStatus::Archived,
            "done goals archive after a week"
        );
    }

    #[test]
    fn retention_drops_only_what_is_old() {
        let mut l = Log::new();
        l.add(started("r1", "s1"));
        l.add(created("g1", "s1"));
        l.add(ended("r1", RunEndKind::Completed));
        l.add(WorkEvent::GoalMarked {
            goal: "g1".into(),
            mark: GoalMark::Archived,
        });
        l.add(started("orphan", "s9"));
        l.add(ended("orphan", RunEndKind::Completed));
        l.add(started("live", "s8"));
        l.add(WorkEvent::AskOpened {
            ask: "k".into(),
            session: "s8".into(),
            kind: AskKind::Question,
            label: "which one?".into(),
            detail: Value::Null,
            expires: None,
        });
        l.add(WorkEvent::AskResolved {
            ask: "k".into(),
            how: AskHow::Answered,
            by: "you".into(),
        });
        let t0 = l.at;
        let mut w = l.w.clone();
        w.retain(t0 + DAY);
        assert_eq!(w, l.w, "nothing is old yet");
        w.retain(t0 + 8 * DAY);
        assert!(!w.asks.contains_key("k"), "answered asks go after a week");
        assert!(w.runs.contains_key("orphan"));
        w.retain(t0 + 31 * DAY);
        assert!(
            !w.runs.contains_key("orphan"),
            "orphan runs go after 30 days"
        );
        assert!(w.runs.contains_key("live"), "an open run stays");
        assert!(w.goals.contains_key("g1"));
        w.retain(t0 + 91 * DAY);
        assert!(
            !w.goals.contains_key("g1"),
            "archived goals go after 90 days"
        );
        assert!(!w.runs.contains_key("r1"), "with their runs");
        assert!(!w.sessions.contains_key("s1"));
    }

    #[test]
    fn a_new_plan_keeps_started_steps() {
        let mut l = Log::new();
        l.add(created("g1", "s1"));
        let step = |k: &str, needs: &[&str]| PlanStep {
            key: k.into(),
            title: k.to_uppercase(),
            needs: needs.iter().map(|s| (*s).to_string()).collect(),
            lane: String::new(),
        };
        l.add(WorkEvent::PlanSet {
            goal: "g1".into(),
            steps: vec![step("a", &[]), step("b", &["a"]), step("c", &["b"])],
        });
        l.add(WorkEvent::StepMarked {
            goal: "g1".into(),
            step: "a".into(),
            status: StepStatus::Done,
            note: None,
            outcome: Some("built".into()),
        });
        l.add(WorkEvent::PlanSet {
            goal: "g1".into(),
            steps: vec![step("b", &["a", "z"]), step("d", &["b"])],
        });
        let keys: Vec<&str> = l.w.goals["g1"]
            .steps
            .iter()
            .map(|s| s.key.as_str())
            .collect();
        assert_eq!(keys, ["b", "d", "a"], "todo c drops, done a stays");
        let a = &l.w.goals["g1"].steps[2];
        assert_eq!(a.outcome.as_deref(), Some("built"));
        assert_eq!(
            l.w.goals["g1"].steps[0].needs,
            vec!["a".to_string()],
            "a need on a missing step is cut"
        );
    }
}
