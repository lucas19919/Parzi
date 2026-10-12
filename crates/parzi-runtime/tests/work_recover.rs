//! An engine start closes what the last one left open. Its own binary:
//! `recover` would cut off the live runs of tests running beside it.

mod common;

use common::*;
use parzi_core::work::{AskHow, AskKind, RunCause, RunEndKind, WorkEvent, WorkLog};
use serde_json::json;

#[tokio::test]
async fn an_engine_start_closes_what_the_last_one_left_open() {
    home("work-recover");
    let log = WorkLog::shared().unwrap();
    log.append(vec![
        WorkEvent::RunStarted {
            run: "left-open".into(),
            session: "gone-session".into(),
            model: "claude/m".into(),
            lane: "build".into(),
            cwd: String::new(),
            cause: RunCause::Send,
        },
        WorkEvent::AskOpened {
            ask: "old-approval".into(),
            session: "gone-session".into(),
            kind: AskKind::Approval,
            label: "Run cargo test".into(),
            detail: json!({}),
            expires: None,
        },
        WorkEvent::AskOpened {
            ask: "old-budget".into(),
            session: "gone-session".into(),
            kind: AskKind::Budget,
            label: "Paused: cost budget".into(),
            detail: json!({}),
            expires: None,
        },
    ])
    .unwrap();
    let (orch, _store) = orch(&[]);
    orch.recover().unwrap();
    let w = log.state();
    assert_eq!(
        w.runs["left-open"].end(),
        Some(RunEndKind::Interrupted),
        "cut off"
    );
    assert_eq!(
        w.asks["old-approval"].resolved.as_ref().map(|r| r.how),
        Some(AskHow::Expired),
        "nobody waits on it any more"
    );
    assert!(w.asks["old-budget"].open(), "a pause outlives a restart");
    assert!(w.hello.is_some(), "the writer says who it is");
}
