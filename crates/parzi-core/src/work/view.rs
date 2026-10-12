//! The shapes the desk returns to the UI (`work_view`, `work_goal`), built
//! from a writer's state. Field names are the contract the UI codes
//! against: change them only together with it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::fold::{Ask, Goal, Run, Step, Writer};
use super::graph;
use super::model::{AskKind, Check, FileTouch, GoalStatus, RunCause, RunEndKind, StepStatus};

const DAY: u64 = 24 * 60 * 60 * 1000;
/// Archived goals stay in the view this long after their last change.
const ARCHIVED_SHOWN: u64 = 7 * DAY;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    Here,
    Server,
    Device,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Engine {
    pub kind: EngineKind,
    pub name: String,
}

impl Engine {
    #[must_use]
    pub fn here() -> Self {
        Self {
            kind: EngineKind::Here,
            name: "This PC".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepView {
    pub key: String,
    pub title: String,
    pub needs: Vec<String>,
    pub lane: String,
    pub status: StepStatus,
    pub note: Option<String>,
    pub outcome: Option<String>,
    pub sessions: Vec<String>,
    pub auto: bool,
    pub ready: bool,
    pub blocked_by: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunView {
    pub id: String,
    pub session: String,
    pub step: Option<String>,
    pub model: String,
    pub lane: String,
    pub cause: RunCause,
    pub started: u64,
    pub ended: Option<u64>,
    pub end: Option<RunEndKind>,
    pub error: Option<String>,
    pub ms: u64,
    pub turns: u32,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: Option<f64>,
    pub summary: String,
    pub files: Vec<FileTouch>,
    pub files_more: usize,
    pub checks: Vec<Check>,
    pub artifacts: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AskView {
    pub id: String,
    pub session: String,
    pub goal: Option<String>,
    pub kind: AskKind,
    pub label: String,
    pub detail: Value,
    pub opened: u64,
    pub expires: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaterView {
    pub auto_start: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalView {
    pub id: String,
    pub title: String,
    pub status: GoalStatus,
    pub project: Option<String>,
    pub root: Option<String>,
    pub prompt: String,
    pub created: u64,
    pub updated: u64,
    pub engine: Engine,
    pub steps: Vec<StepView>,
    pub running: Vec<String>,
    pub last: Option<RunView>,
    pub cost_usd: Option<f64>,
    pub files: usize,
    pub asks: usize,
    pub later: Option<LaterView>,
    pub owned: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteView {
    pub at: u64,
    pub by: String,
    pub text: String,
    pub step: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalDetail {
    #[serde(flatten)]
    pub goal: GoalView,
    pub runs: Vec<RunView>,
    pub notes: Vec<NoteView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Digest {
    pub since: u64,
    pub done: usize,
    pub steps: usize,
    pub needs_you: usize,
    pub failed: usize,
    pub goals: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerLink {
    pub linked: bool,
    /// False when the server is too old for the work ops.
    pub ops: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkView {
    pub goals: Vec<GoalView>,
    pub asks: Vec<AskView>,
    pub digest: Option<Digest>,
    pub server: ServerLink,
}

/// How session ids are shown: raw for this PC, tagged for others.
pub type Tag<'a> = &'a dyn Fn(&str) -> String;

/// Whose goals these are and how to show their session ids.
pub struct Lens<'a> {
    pub engine: Engine,
    pub tag: Tag<'a>,
    /// This desk may act on them.
    pub owned: bool,
    pub now: u64,
}

impl Lens<'_> {
    fn t(&self, id: &str) -> String {
        (self.tag)(id)
    }
}

fn step_view(s: &Step, all: &[Step], lens: &Lens) -> StepView {
    let blocked_by = if s.status.settled() {
        vec![]
    } else {
        graph::open_needs(s, all)
    };
    StepView {
        key: s.key.clone(),
        title: s.title.clone(),
        needs: s.needs.clone(),
        lane: s.lane.clone(),
        status: s.status,
        note: s.note.clone(),
        outcome: s.outcome.clone(),
        sessions: s.sessions.iter().map(|x| lens.t(x)).collect(),
        auto: s.auto,
        ready: s.status == StepStatus::Todo && blocked_by.is_empty(),
        blocked_by,
    }
}

#[must_use]
pub fn run_view(r: &Run, lens: &Lens) -> RunView {
    let o = r.outcome.as_ref();
    RunView {
        id: r.id.clone(),
        session: lens.t(&r.session),
        step: r.step.clone(),
        model: r.model.clone(),
        lane: r.lane.clone(),
        cause: r.cause,
        started: r.started,
        ended: r.ended,
        end: o.map(|o| o.end),
        error: o.and_then(|o| o.error.clone()),
        ms: o.map_or_else(|| lens.now.saturating_sub(r.started), |o| o.ms),
        turns: o.map_or(0, |o| o.turns),
        tokens_in: o.map_or(0, |o| o.tokens_in),
        tokens_out: o.map_or(0, |o| o.tokens_out),
        cost_usd: o.and_then(|o| o.cost_usd),
        summary: o.map(|o| o.summary.clone()).unwrap_or_default(),
        files: o.map(|o| o.files.clone()).unwrap_or_default(),
        files_more: o.map_or(0, |o| o.files_more),
        checks: o.map(|o| o.checks.clone()).unwrap_or_default(),
        artifacts: o.map_or(0, |o| o.artifacts),
    }
}

#[must_use]
pub fn ask_view(a: &Ask, lens: &Lens) -> AskView {
    AskView {
        id: a.id.clone(),
        session: lens.t(&a.session),
        goal: a.goal.clone(),
        kind: a.kind,
        label: a.label.clone(),
        detail: a.detail.clone(),
        opened: a.opened,
        expires: a.expires,
    }
}

#[must_use]
pub fn goal_view(w: &Writer, g: &Goal, lens: &Lens) -> GoalView {
    let runs = w.goal_runs(g);
    let mut running: Vec<String> = vec![];
    for r in runs.iter().filter(|r| r.open()) {
        let s = lens.t(&r.session);
        if !running.contains(&s) {
            running.push(s);
        }
    }
    let last = runs
        .iter()
        .max_by_key(|r| r.started)
        .map(|r| run_view(r, lens));
    let costs: Vec<f64> = runs
        .iter()
        .filter_map(|r| r.outcome.as_ref().and_then(|o| o.cost_usd))
        .collect();
    let mut files: Vec<String> = runs
        .iter()
        .flat_map(|r| r.files().iter().map(|f| f.path.clone()))
        .collect();
    files.sort_unstable();
    files.dedup();
    GoalView {
        id: g.id.clone(),
        title: g.title.clone(),
        status: w.status(g, lens.now),
        project: g.project.clone(),
        root: g.root.as_deref().map(|r| lens.t(r)),
        prompt: g.prompt.clone(),
        created: g.created,
        updated: g.updated,
        engine: lens.engine.clone(),
        steps: g
            .steps
            .iter()
            .map(|s| step_view(s, &g.steps, lens))
            .collect(),
        running,
        last,
        cost_usd: (!costs.is_empty()).then(|| costs.iter().sum()),
        files: files.len(),
        asks: w.goal_asks(&g.id).count(),
        later: g.later.map(|l| LaterView {
            auto_start: l.auto_start,
        }),
        owned: lens.owned,
    }
}

#[must_use]
pub fn goal_detail(w: &Writer, id: &str, lens: &Lens) -> Option<GoalDetail> {
    let g = w.goals.get(id)?;
    Some(GoalDetail {
        goal: goal_view(w, g, lens),
        runs: w.goal_runs(g).iter().map(|r| run_view(r, lens)).collect(),
        notes: g
            .notes
            .iter()
            .map(|n| NoteView {
                at: n.at,
                by: n.by.clone(),
                text: n.text.clone(),
                step: n.step.clone(),
            })
            .collect(),
    })
}

/// Goals worth showing, newest change first: archived ones only for a
/// week after their last change.
#[must_use]
pub fn goal_views(w: &Writer, lens: &Lens) -> Vec<GoalView> {
    let mut out: Vec<GoalView> = w
        .goals
        .values()
        .map(|g| goal_view(w, g, lens))
        .filter(|g| {
            g.status != GoalStatus::Archived || lens.now.saturating_sub(g.updated) < ARCHIVED_SHOWN
        })
        .collect();
    out.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
    out
}

/// Asks still open, oldest first.
#[must_use]
pub fn open_asks(w: &Writer, lens: &Lens) -> Vec<AskView> {
    let mut out: Vec<AskView> = w
        .asks
        .values()
        .filter(|a| a.open())
        .map(|a| ask_view(a, lens))
        .collect();
    out.sort_by_key(|a| a.opened);
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::work::model::{Entry, PlanStep, RunOutcome, Via, WorkEvent};

    fn keys(v: &Value) -> BTreeSet<String> {
        v.as_object().unwrap().keys().cloned().collect()
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    /// A small ledger: one goal with an auto step, a finished run with a
    /// file and a check, a running child, and a budget pause elsewhere.
    pub(crate) fn sample() -> Writer {
        let mut w = Writer::new("pc-ab12");
        let evs = vec![
            WorkEvent::RunStarted {
                run: "r1".into(),
                session: "s1".into(),
                model: "claude/opus".into(),
                lane: "build".into(),
                cwd: "D:/Repos/app".into(),
                cause: RunCause::Send,
            },
            WorkEvent::GoalCreated {
                goal: "k7m2q".into(),
                title: "Fix the omnibar autosize".into(),
                project: Some("parzi".into()),
                root: Some("s1".into()),
                prompt: "fix the omnibar autosize".into(),
                origin: RunCause::Send,
                later: None,
                continues: None,
            },
            WorkEvent::StepAdded {
                goal: "k7m2q".into(),
                step: PlanStep {
                    key: "a".into(),
                    title: "Write the test".into(),
                    needs: vec![],
                    lane: "build".into(),
                },
                auto: true,
            },
            WorkEvent::StepLinked {
                goal: "k7m2q".into(),
                step: "a".into(),
                session: "c1".into(),
            },
            WorkEvent::RunStarted {
                run: "r2".into(),
                session: "c1".into(),
                model: "claude/sonnet".into(),
                lane: "build".into(),
                cwd: "D:/Repos/app".into(),
                cause: RunCause::Message,
            },
            WorkEvent::RunEnded {
                run: "r1".into(),
                outcome: RunOutcome {
                    summary: "Fixed the autosize and added a test.".into(),
                    files: vec![FileTouch {
                        path: "ui/src/Omnibar.svelte".into(),
                        via: Via::Tool,
                    }],
                    checks: vec![Check {
                        cmd: "npm test".into(),
                        ok: true,
                    }],
                    ms: 81_000,
                    turns: 1,
                    tokens_in: 12_000,
                    tokens_out: 900,
                    cost_usd: Some(0.42),
                    ..RunOutcome::bare(RunEndKind::Completed)
                },
            },
            WorkEvent::AskOpened {
                ask: "b1".into(),
                session: "s9".into(),
                kind: AskKind::Budget,
                label: "Paused: token budget reached".into(),
                detail: serde_json::json!({"reason": "token budget reached"}),
                expires: None,
            },
        ];
        for (i, ev) in evs.into_iter().enumerate() {
            w.apply(&Entry {
                w: "pc-ab12".into(),
                seq: i as u64 + 1,
                at: 1_760_000_000_000 + i as u64 * 1_000,
                ev,
            });
        }
        w
    }

    fn lens() -> Lens<'static> {
        Lens {
            engine: Engine::here(),
            tag: &|s: &str| s.to_string(),
            owned: true,
            now: 1_760_000_010_000,
        }
    }

    #[test]
    fn the_view_has_the_contract_fields() {
        let w = sample();
        let view = WorkView {
            goals: goal_views(&w, &lens()),
            asks: open_asks(&w, &lens()),
            digest: None,
            server: ServerLink::default(),
        };
        let v = serde_json::to_value(&view).unwrap();
        assert_eq!(keys(&v), set(&["goals", "asks", "digest", "server"]));
        assert_eq!(keys(&v["server"]), set(&["linked", "ops"]));
        let g = &v["goals"][0];
        assert_eq!(
            keys(g),
            set(&[
                "id", "title", "status", "project", "root", "prompt", "created", "updated",
                "engine", "steps", "running", "last", "cost_usd", "files", "asks", "later",
                "owned",
            ])
        );
        assert_eq!(
            g["engine"],
            serde_json::json!({"kind": "here", "name": "This PC"})
        );
        assert_eq!(g["status"], "working", "a step still runs");
        assert_eq!(g["running"], serde_json::json!(["c1"]));
        assert_eq!(
            keys(&g["steps"][0]),
            set(&[
                "key",
                "title",
                "needs",
                "lane",
                "status",
                "note",
                "outcome",
                "sessions",
                "auto",
                "ready",
                "blocked_by",
            ])
        );
        assert_eq!(g["steps"][0]["status"], "running");
        let last = &g["last"];
        assert_eq!(
            keys(last),
            set(&[
                "id",
                "session",
                "step",
                "model",
                "lane",
                "cause",
                "started",
                "ended",
                "end",
                "error",
                "ms",
                "turns",
                "tokens_in",
                "tokens_out",
                "cost_usd",
                "summary",
                "files",
                "files_more",
                "checks",
                "artifacts",
            ])
        );
        assert_eq!(last["id"], "r2", "the latest run, root or child");
        assert_eq!(last["cause"], "message");
        assert_eq!(last["end"], Value::Null);
        assert_eq!(g["files"], 1);
        assert_eq!(g["cost_usd"], 0.42);
        assert_eq!(g["later"], Value::Null);
        let ask = &v["asks"][0];
        assert_eq!(
            keys(ask),
            set(&["id", "session", "goal", "kind", "label", "detail", "opened", "expires"])
        );
        assert_eq!(ask["kind"], "budget");
        let back: WorkView = serde_json::from_value(v).unwrap();
        assert_eq!(back, view, "the view reads back as written");
    }

    #[test]
    fn the_detail_adds_runs_and_notes() {
        let w = sample();
        let d = goal_detail(&w, "k7m2q", &lens()).unwrap();
        let v = serde_json::to_value(&d).unwrap();
        assert!(keys(&v).contains("runs") && keys(&v).contains("notes"));
        assert!(keys(&v).contains("steps"), "the goal's own fields are flat");
        let runs: Vec<&str> = v["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        assert_eq!(runs, ["r1", "r2"], "oldest first");
        assert_eq!(v["runs"][0]["files"][0]["via"], "tool");
        assert_eq!(
            v["runs"][0]["checks"][0],
            serde_json::json!({"cmd": "npm test", "ok": true})
        );
        assert!(goal_detail(&w, "nope", &lens()).is_none());
    }
}
