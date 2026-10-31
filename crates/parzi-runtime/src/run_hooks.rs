use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use parzi_core::now_ms;
use parzi_core::store::{Event, FileTouch, RunCause, RunEndKind, RunOutcome, SessionStore, Via};
use serde_json::Value;

use crate::run::RunTally;

pub(crate) type Tally = Arc<Mutex<RunTally>>;

const GIT_WAIT: Duration = Duration::from_secs(3);

pub(crate) fn lock(t: &Tally) -> std::sync::MutexGuard<'_, RunTally> {
    t.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn run_started(t: &Tally) {
    let (cause, cwd) = {
        let t = lock(t);
        (t.cause, t.cwd.clone())
    };
    if !cause.recorded() {
        return;
    }
    lock(t).git = Some(tokio::spawn(async move {
        crate::gitmark::GitMark::take(std::path::Path::new(&cwd)).await
    }));
}

pub(crate) fn tool_started(t: &Tally, id: &str, name: &str, input: &Value) {
    lock(t).tool_started(id, name, input);
}

pub(crate) fn launch_failed(store: &SessionStore, session: &str, cause: RunCause, error: &str) {
    if !cause.recorded() {
        return;
    }
    let mut outcome = RunOutcome::bare(RunEndKind::NotStarted, now_ms());
    outcome.summary = format!("Didn't start: {error}");
    if let Err(e) = store.set_last_run(session, outcome, None) {
        tracing::warn!("run outcome of {session} not kept: {e}");
    }
}

fn summary_of(events: &[Event]) -> String {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Assistant { text, done: true } if !text.trim().is_empty() => {
                Some(parzi_core::text::summarize(text, RunOutcome::SUMMARY_CAP))
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn merge_files(edits: &[String], git: Vec<String>) -> Vec<FileTouch> {
    let mut out: Vec<FileTouch> = edits
        .iter()
        .map(|p| FileTouch {
            path: p.clone(),
            via: Via::Tool,
        })
        .collect();
    for p in git {
        if !out
            .iter()
            .any(|f| parzi_core::paths::same_path(&f.path, &p))
        {
            out.push(FileTouch {
                path: p,
                via: Via::Git,
            });
        }
    }
    out
}

pub(crate) async fn run_ended(store: &SessionStore, t: &Tally, stopped: bool, settled: bool) {
    let (session, cause, recorded) = {
        let t = lock(t);
        (t.session.clone(), t.cause, t.recorded)
    };
    if !cause.recorded() || recorded {
        return;
    }
    let (first, error, git, edits, mut outcome) = {
        let mut t = lock(t);
        let (end, error) = match t.end.clone() {
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
            cost_usd: t.cost,
            checks: t.checks.clone(),
            ..RunOutcome::bare(end, 0)
        };
        (
            t.first_event,
            error,
            t.git.take(),
            t.edits().to_vec(),
            outcome,
        )
    };
    let looked = async {
        let mark = git?.await.ok()??;
        Some(mark.finish(&edits).await)
    };
    let (git_files, branch) = match tokio::time::timeout(GIT_WAIT, looked).await {
        Ok(Some(done)) => (done.files, Some(done.branch)),
        _ => (vec![], None),
    };
    let events = store.events_from(&session, first).unwrap_or_default();
    outcome.summary = summary_of(&events);
    if outcome.summary.is_empty() || outcome.end != RunEndKind::Completed {
        if let Some(e) = error.filter(|e| !e.trim().is_empty()) {
            outcome.summary = match outcome.end {
                RunEndKind::Paused => format!("Paused: {e}"),
                RunEndKind::Interrupted => e,
                _ => format!("Failed: {e}"),
            };
        }
    }
    outcome.files = merge_files(&edits, git_files);
    outcome.ended = now_ms();
    match store.set_last_run(&session, outcome, branch.as_deref()) {
        Ok(()) => lock(t).recorded = true,
        Err(e) => tracing::warn!("run outcome of {session} not kept: {e}"),
    }
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
