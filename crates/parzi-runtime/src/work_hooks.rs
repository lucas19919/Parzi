//! Where runs meet the work ledger. A run is recorded when it starts and
//! ends; the first real tool call of a run the person asked for makes (or
//! continues) its goal; a spawn adds a step; a budget pause asks. Every
//! hook logs and moves on when the ledger cannot be written: work tracking
//! never stops a run.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use parzi_core::store::{Event, SessionStore};
use parzi_core::work::{
    self, AskHow, AskKind, FileTouch, PlanStep, RunEndKind, RunOutcome, StepStatus, Via, WorkEvent,
    WorkLog,
};
use serde_json::{json, Value};

use crate::run::RunTally;

pub(crate) type Tally = Arc<Mutex<RunTally>>;

/// Live runs of this process by session: what `ensure_goal` and a spawn
/// look at to know a run's cause and prompt.
static LIVE: Mutex<Option<HashMap<String, Tally>>> = Mutex::new(None);
/// Making a goal is look-then-append: one at a time, or a tool call and a
/// spawn racing in the same run could make two.
static ENSURE: Mutex<()> = Mutex::new(());

fn live<R>(f: impl FnOnce(&mut HashMap<String, Tally>) -> R) -> R {
    let mut g = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
    f(g.get_or_insert_with(HashMap::new))
}

fn log() -> Option<Arc<WorkLog>> {
    match WorkLog::shared() {
        Ok(l) => Some(l),
        Err(e) => {
            tracing::warn!("work ledger unavailable: {e}");
            None
        }
    }
}

fn append(log: &WorkLog, events: Vec<WorkEvent>) {
    if let Err(e) = log.append(events) {
        tracing::warn!("work ledger not written: {e}");
    }
}

fn lock(t: &Tally) -> std::sync::MutexGuard<'_, RunTally> {
    t.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Engine start: runs the last process left open were cut off, asks it
/// was waiting on can no longer be answered, and an old task list comes
/// in once as Later goals.
pub(crate) fn recover() {
    let Some(log) = log() else {
        return;
    };
    let mine: Vec<String> = live(|m| m.values().map(|t| lock(t).run.clone()).collect());
    let (open, waiting, hello) = log.read(|w| {
        (
            w.open_runs
                .iter()
                .filter(|r| !mine.contains(r))
                .cloned()
                .collect::<Vec<_>>(),
            w.asks
                .values()
                .filter(|a| a.open() && a.kind.waits())
                .map(|a| a.id.clone())
                .collect::<Vec<_>>(),
            w.hello.is_none(),
        )
    });
    let mut events = vec![];
    if hello {
        let me = parzi_core::device::this();
        events.push(WorkEvent::Hello {
            device: me.id,
            name: me.name,
            engine: if crate::orchestrator::headless() {
                "server".into()
            } else {
                "desk".into()
            },
        });
    }
    for run in open {
        events.push(WorkEvent::RunEnded {
            run,
            outcome: RunOutcome::bare(RunEndKind::Interrupted),
        });
    }
    for ask in waiting {
        events.push(WorkEvent::AskResolved {
            ask,
            how: AskHow::Expired,
            by: "parzi".into(),
        });
    }
    append(&log, events);
    if let Ok(home) = parzi_core::paths::parzi_dir() {
        if let Err(e) = work::import_legacy_tasks(&home, &log) {
            tracing::warn!("old tasks not imported: {e}");
        }
    }
}

/// A run began: it is recorded (joining its session's goal when that goal
/// takes it), a budget pause on the session is over, and git is looked at
/// in the background.
pub(crate) fn run_started(t: &Tally, model: &str, lane: &str) {
    let (run, session, cause, cwd) = {
        let t = lock(t);
        (t.run.clone(), t.session.clone(), t.cause, t.cwd.clone())
    };
    if !cause.ledgered() {
        return;
    }
    live(|m| m.insert(session.clone(), t.clone()));
    lock(t).git = Some(tokio::spawn(async move {
        crate::gitmark::GitMark::take(std::path::Path::new(&cwd)).await
    }));
    let Some(log) = log() else {
        return;
    };
    let cwd = lock(t).cwd.clone();
    let paused: Vec<String> = log.read(|w| {
        w.asks
            .values()
            .filter(|a| {
                a.open()
                    && a.session == session
                    && matches!(a.kind, AskKind::Budget | AskKind::Continue)
            })
            .map(|a| a.id.clone())
            .collect()
    });
    let mut events: Vec<WorkEvent> = paused
        .into_iter()
        .map(|ask| WorkEvent::AskResolved {
            ask,
            how: AskHow::Continued,
            by: "you".into(),
        })
        .collect();
    events.push(WorkEvent::RunStarted {
        run: run.clone(),
        session,
        model: model.to_string(),
        lane: lane.to_string(),
        cwd,
        cause,
    });
    append(&log, events);
    let goal = log.read(|w| w.runs.get(&run).and_then(|r| r.goal.clone()));
    lock(t).goal = goal;
}

/// A tool call began. The first real one makes the run's goal.
pub(crate) fn tool_started(t: &Tally, id: &str, name: &str, input: &Value) {
    let (newly, session) = {
        let mut t = lock(t);
        (t.tool_started(id, name, input), t.session.clone())
    };
    if newly {
        ensure_goal(&session);
    }
}

/// The goal the session's live run works for, made now when the run is
/// real work the person asked for and its session leads no goal that
/// takes it (see `Writer::joins`).
pub(crate) fn ensure_goal(session: &str) -> Option<String> {
    let _one = ENSURE.lock().unwrap_or_else(PoisonError::into_inner);
    let t = live(|m| m.get(session).cloned())?;
    let (run, cause, prompt, hint, project, have) = {
        let t = lock(&t);
        (
            t.run.clone(),
            t.cause,
            t.prompt.clone(),
            t.title_hint.clone(),
            t.project.clone(),
            t.goal.clone(),
        )
    };
    if have.is_some() {
        return have;
    }
    let log = log()?;
    let (joined, before, goal) = log.read(|w| {
        (
            w.runs.get(&run).and_then(|r| r.goal.clone()),
            w.sessions.get(session).map(|b| b.goal.clone()),
            work::graph::fresh_id(5, &|id| w.goals.contains_key(id)),
        )
    });
    if let Some(g) = joined {
        lock(&t).goal = Some(g.clone());
        return Some(g);
    }
    if !cause.makes_goals() {
        return None;
    }
    let mut title = work::title_from_prompt(&prompt);
    if title.is_empty() {
        title = work::title_from_prompt(&hint);
    }
    if title.is_empty() {
        title = "Untitled goal".into();
    }
    log.append(vec![WorkEvent::GoalCreated {
        goal: goal.clone(),
        title,
        project,
        root: Some(session.to_string()),
        prompt,
        origin: cause,
        later: None,
        continues: before,
    }])
    .map_err(|e| tracing::warn!("goal not recorded: {e}"))
    .ok()?;
    lock(&t).goal = Some(goal.clone());
    Some(goal)
}

/// A session spawned a child: the child works a step of the caller's goal
/// (made now if the caller's run had none yet), added for it.
pub(crate) fn spawned(caller: &str, child: &str, title: &str, lane: &str) {
    let goal = ensure_goal(caller);
    let _one = ENSURE.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(log) = log() else {
        return;
    };
    let found = log.read(|w| {
        // A step's own children work for the same goal.
        let gid = goal
            .clone()
            .or_else(|| w.sessions.get(caller).map(|b| b.goal.clone()))?;
        let g = w.goals.get(&gid).filter(|g| !g.archived)?;
        let taken: Vec<String> = g.steps.iter().map(|s| s.key.clone()).collect();
        Some((gid, work::auto_step_key(&taken)))
    });
    let Some((goal, key)) = found else {
        return;
    };
    append(
        &log,
        vec![
            WorkEvent::StepAdded {
                goal: goal.clone(),
                step: PlanStep {
                    key: key.clone(),
                    title: title.to_string(),
                    needs: vec![],
                    lane: lane.to_string(),
                },
                auto: true,
            },
            WorkEvent::StepLinked {
                goal,
                step: key,
                session: child.to_string(),
            },
        ],
    );
}

/// A run stopped at the budget: the person decides whether it goes on.
pub(crate) fn budget_paused(session: &str, reason: &str) {
    if live(|m| !m.contains_key(session)) {
        return;
    }
    let Some(log) = log() else {
        return;
    };
    let ask = log.read(|w| work::graph::fresh_id(8, &|id| w.asks.contains_key(id)));
    append(
        &log,
        vec![WorkEvent::AskOpened {
            ask,
            session: session.to_string(),
            kind: AskKind::Budget,
            label: format!("Paused: {reason}"),
            detail: json!({ "reason": reason }),
            expires: None,
        }],
    );
}

/// A run could not start: its goal hears why and its step is open again.
pub(crate) fn launch_failed(session: &str, error: &str) {
    let Some(log) = log() else {
        return;
    };
    let bound = log.read(|w| {
        let b = w.sessions.get(session)?;
        let g = w.goals.get(&b.goal)?;
        let step = b
            .step
            .as_ref()
            .and_then(|k| g.steps.iter().find(|s| &s.key == k))
            .map(|s| (s.key.clone(), s.status));
        Some((b.goal.clone(), step))
    });
    let Some((goal, step)) = bound else {
        return;
    };
    let mut events = vec![WorkEvent::NoteAdded {
        goal: goal.clone(),
        by: "parzi".into(),
        text: format!("Didn't start: {error}"),
        step: step.as_ref().map(|(k, _)| k.clone()),
    }];
    if let Some((key, status)) = step {
        if !matches!(status, StepStatus::Todo | StepStatus::Done) {
            events.push(WorkEvent::StepMarked {
                goal,
                step: key,
                status: StepStatus::Todo,
                note: None,
                outcome: None,
            });
        }
    }
    append(&log, events);
}

/// The last finished reply since the run's first event.
fn summary_of(events: &[Event]) -> String {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Assistant { text, done: true } if !text.trim().is_empty() => {
                Some(work::summarize(text))
            }
            _ => None,
        })
        .unwrap_or_default()
}

/// Tool-named files first, then what only git saw.
fn merge_files(edits: &[String], git: Vec<String>) -> Vec<FileTouch> {
    let mut out: Vec<FileTouch> = edits
        .iter()
        .map(|p| FileTouch {
            path: p.clone(),
            via: Via::Tool,
        })
        .collect();
    for p in git {
        if !out.iter().any(|f| crate::gitmark::same_path(&f.path, &p)) {
            out.push(FileTouch {
                path: p,
                via: Via::Git,
            });
        }
    }
    out
}

/// A run's end, recorded from its tally. `stopped`: the person stopped it;
/// `settled`: the run returned (a panic or an abort did not).
pub(crate) async fn run_ended(store: &SessionStore, t: &Tally, stopped: bool, settled: bool) {
    let (session, cause) = {
        let t = lock(t);
        (t.session.clone(), t.cause)
    };
    if !cause.ledgered() {
        return;
    }
    live(|m| {
        if m.get(&session).is_some_and(|x| Arc::ptr_eq(x, t)) {
            m.remove(&session);
        }
    });
    let (run, first, end, error, git, edits, mut outcome) = {
        let mut t = lock(t);
        let (end, error) = match t.end.take() {
            Some(e) => e,
            None if stopped => (RunEndKind::Stopped, None),
            None if settled => (RunEndKind::Failed, None),
            None => (
                RunEndKind::Crashed,
                Some("the run stopped without settling".into()),
            ),
        };
        let outcome = RunOutcome {
            ms: u64::try_from(t.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            turns: t.turns,
            tokens_in: t.tokens_in,
            tokens_out: t.tokens_out,
            cost_usd: t.cost,
            checks: t.checks.clone(),
            ..RunOutcome::bare(end)
        };
        (
            t.run.clone(),
            t.first_event,
            end,
            error,
            t.git.take(),
            t.edits().to_vec(),
            outcome,
        )
    };
    let git_files = match git {
        Some(h) => match tokio::time::timeout(Duration::from_secs(10), h).await {
            Ok(Ok(Some(mark))) => mark.finish(&edits).await,
            _ => vec![],
        },
        None => vec![],
    };
    let events = store.events_from(&session, first).unwrap_or_default();
    outcome.end = end;
    outcome.error = error;
    outcome.summary = summary_of(&events);
    outcome.artifacts = u32::try_from(
        events
            .iter()
            .filter(|e| matches!(e, Event::Artifact { .. }))
            .count(),
    )
    .unwrap_or(u32::MAX);
    outcome.files = merge_files(&edits, git_files);
    let Some(log) = log() else {
        return;
    };
    append(&log, vec![WorkEvent::RunEnded { run, outcome }]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_is_the_last_finished_reply() {
        let events = vec![
            Event::Assistant {
                text: "**Done**: fixed it.".into(),
                done: true,
            },
            Event::Assistant {
                text: "half a thought".into(),
                done: false,
            },
            Event::ToolCall {
                id: "1".into(),
                name: "Edit".into(),
                args: Value::Null,
            },
        ];
        assert_eq!(summary_of(&events), "Done: fixed it.");
        assert_eq!(summary_of(&events[1..]), "");
    }

    #[test]
    fn git_only_adds_what_the_tools_did_not_name() {
        let files = merge_files(
            &["src/a.rs".into()],
            vec!["src\\a.rs".into(), "Cargo.lock".into()],
        );
        assert_eq!(
            files,
            [
                FileTouch {
                    path: "src/a.rs".into(),
                    via: Via::Tool
                },
                FileTouch {
                    path: "Cargo.lock".into(),
                    via: Via::Git
                },
            ]
        );
    }
}
