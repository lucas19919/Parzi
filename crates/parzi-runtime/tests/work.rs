//! Runs and the work ledger: which runs become goals, what a run's end
//! records, steps from spawns, and what an engine start closes.

mod common;

use std::sync::Arc;

use common::*;
use parzi_core::config::{Budget, ParziConfig};
use parzi_core::work::{
    AskHow, AskKind, GoalStatus, RunCause, RunEndKind, StepStatus, Via, WorkLog, Writer,
};
use parzi_providers::TurnEnd;
use parzi_runtime::handler::RunEvent;
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};
use serde_json::json;

struct Allow;
#[async_trait::async_trait]
impl Approver for Allow {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

fn ledger() -> Writer {
    WorkLog::shared().unwrap().state()
}

/// The ledger once every run of `session` has ended.
async fn ended(session: &str) -> Writer {
    for _ in 0..200 {
        let w = ledger();
        let runs: Vec<_> = w.runs.values().filter(|r| r.session == session).collect();
        if !runs.is_empty() && runs.iter().all(|r| !r.open()) {
            return w;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the runs of {session} were not recorded as ended");
}

fn goal_of(w: &Writer, session: &str) -> Option<String> {
    w.sessions.get(session).map(|b| b.goal.clone())
}

async fn run_once(fake: Arc<Fake>, prompt: &str) -> (Writer, String) {
    home("work");
    let (orch, store) = orch(&[fake]);
    let (meta, _rx) = orch
        .spawn(
            "work-tests",
            "build",
            "claude",
            prompt,
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    (ended(&meta.id).await, meta.id)
}

async fn tool(a: &Agent, id: &str, name: &str, input: serde_json::Value) {
    a.own_tool(id, name, input, || async { (true, "ok".to_string()) })
        .await;
}

#[tokio::test]
async fn a_chat_is_recorded_but_makes_no_goal() {
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            tool(&a, "1", "Read", json!({"file_path": "a.rs"})).await;
            tool(&a, "2", "Grep", json!({"pattern": "x"})).await;
            a.say("It reads fine.");
            Ok(TurnEnd::Completed)
        }),
    );
    let (w, sid) = run_once(fake, "what does a.rs do?").await;
    assert_eq!(goal_of(&w, &sid), None, "two reads are a question");
    let run = w.runs.values().find(|r| r.session == sid).unwrap();
    assert_eq!(run.goal, None);
    assert_eq!(run.cause, RunCause::Send);
    assert_eq!(run.end(), Some(RunEndKind::Completed));
}

#[tokio::test]
async fn an_edit_makes_a_goal_with_the_run_and_what_it_changed() {
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            tool(&a, "1", "Edit", json!({"file_path": "src/omnibar.ts"})).await;
            a.own_tool(
                "2",
                "mcp__parzi__shell_exec",
                json!({"cmd": "npm test"}),
                || async { (false, "1 failed".to_string()) },
            )
            .await;
            a.usage(1_000, 200, Some(0.05));
            a.say("## Fixed\nThe omnibar grows with its text now.");
            Ok(TurnEnd::Completed)
        }),
    );
    let (w, sid) = run_once(fake, "hey, can you fix the omnibar autosize?").await;
    let gid = goal_of(&w, &sid).expect("an edit is real work");
    let g = &w.goals[&gid];
    assert_eq!(g.title, "Fix the omnibar autosize");
    assert_eq!(g.project.as_deref(), Some("work-tests"));
    assert_eq!(g.root.as_deref(), Some(sid.as_str()));
    assert_eq!(g.origin, RunCause::Send);
    assert_eq!(g.runs.len(), 1, "the run that qualified joined");
    let run = &w.runs[&g.runs[0]];
    let out = run.outcome.as_ref().unwrap();
    assert_eq!(out.end, RunEndKind::Completed);
    assert_eq!(out.summary, "Fixed The omnibar grows with its text now.");
    assert_eq!(out.files.len(), 1);
    assert_eq!(out.files[0].path, "src/omnibar.ts");
    assert_eq!(out.files[0].via, Via::Tool);
    assert_eq!(out.checks.len(), 1);
    assert!(!out.checks[0].ok);
    assert_eq!((out.tokens_in, out.tokens_out), (1_000, 200));
    assert_eq!(out.cost_usd, Some(0.05));
    assert_eq!(out.turns, 1);
    assert_eq!(
        w.status(g, parzi_core::work::now_ms()),
        GoalStatus::Check,
        "a failed test needs a look"
    );
}

#[tokio::test]
async fn three_calls_of_any_kind_make_a_goal_and_the_next_send_continues_it() {
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            for (i, name) in ["Read", "Grep", "WebFetch"].iter().enumerate() {
                tool(&a, &i.to_string(), name, json!({})).await;
            }
            a.say("Looked into it.");
            Ok(TurnEnd::Completed)
        }),
    );
    home("work");
    let (orch, store) = orch(&[fake]);
    let (meta, _rx) = orch
        .spawn(
            "work-tests",
            "build",
            "claude",
            "research the flaky test",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let w = ended(&meta.id).await;
    let gid = goal_of(&w, &meta.id).expect("three calls are work");
    orch.send_to(&meta.id, "and fix it", None, "", "low", vec![], None, None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let w = ended(&meta.id).await;
    assert_eq!(
        w.goals[&gid].runs.len(),
        2,
        "a fresh goal takes the next run"
    );
    assert_eq!(
        w.goals
            .values()
            .filter(|g| g.root.as_deref() == Some(meta.id.as_str()))
            .count(),
        1
    );
}

#[tokio::test]
async fn a_spawn_adds_a_step_the_child_works() {
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            if a.spec.prompt.contains("child job") {
                a.say("child done");
                return Ok(TurnEnd::Completed);
            }
            let (ok, out) = a
                .parzi(
                    "session.spawn",
                    json!({"title": "Write the test", "prompt": "child job", "wait": true}),
                )
                .await;
            a.say(&format!("spawned ok={ok} {out}"));
            Ok(TurnEnd::Completed)
        }),
    );
    home("work");
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "auto".into();
    cfg.orchestrator.max_concurrent = 4;
    let (orch, store) = orch_with(cfg, &[fake]);
    let (meta, _rx) = orch
        .spawn(
            "work-tests",
            "build",
            "claude",
            "build the parser with a helper",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            Some("full".into()),
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let child = store.list_children(&meta.id).unwrap().remove(0);
    ended(&child.id).await;
    let w = ended(&meta.id).await;
    let gid = goal_of(&w, &meta.id).expect("a spawn is real work");
    let g = &w.goals[&gid];
    assert_eq!(g.steps.len(), 1);
    let step = &g.steps[0];
    assert_eq!(
        (step.key.as_str(), step.title.as_str()),
        ("a", "Write the test")
    );
    assert!(step.auto);
    assert_eq!(step.sessions, vec![child.id.clone()]);
    assert_eq!(
        step.status,
        StepStatus::Done,
        "the step follows its child's run"
    );
    let child_run = w.runs.values().find(|r| r.session == child.id).unwrap();
    assert_eq!(child_run.cause, RunCause::Message);
    assert_eq!(child_run.goal.as_deref(), Some(gid.as_str()));
    assert_eq!(child_run.step.as_deref(), Some("a"));
    assert_eq!(w.sessions[&child.id].step.as_deref(), Some("a"));
}

#[tokio::test]
async fn a_budget_pause_says_so_and_asks() {
    home("work");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            tool(&a, "1", "Write", json!({"file_path": "big.txt"})).await;
            for _ in 0..50 {
                if a.cancel.is_cancelled() {
                    return Ok(TurnEnd::Interrupted);
                }
                a.usage(800, 200, Some(0.01));
                a.say("working");
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Ok(TurnEnd::Completed)
        }),
    );
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "auto".into();
    cfg.budget = Budget {
        max_cost_usd: None,
        max_tokens: Some(2_500),
    };
    let (orch, store) = orch_with(cfg, &[fake]);
    let (meta, mut rx) = orch
        .spawn(
            "work-tests",
            "build",
            "claude",
            "write the big file",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    let mut paused = None;
    while let Some(ev) = rx.recv().await {
        if let RunEvent::Paused { reason } = ev {
            paused = Some(reason);
        }
    }
    let reason = paused.expect("the UI hears the pause");
    assert!(reason.contains("token budget"), "{reason}");
    let w = ended(&meta.id).await;
    let run = w.runs.values().find(|r| r.session == meta.id).unwrap();
    assert_eq!(run.end(), Some(RunEndKind::Paused));
    let ask = w
        .asks
        .values()
        .find(|a| a.session == meta.id)
        .expect("a budget ask");
    assert_eq!(ask.kind, AskKind::Budget);
    assert!(ask.open());
    let gid = goal_of(&w, &meta.id).unwrap();
    assert_eq!(
        w.status(&w.goals[&gid], parzi_core::work::now_ms()),
        GoalStatus::Paused
    );
    // Sending the thread on is the answer.
    let _ = orch
        .send_to(&meta.id, "go on", None, "", "low", vec![], None, None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let w = ended(&meta.id).await;
    let resolved = w.asks[&ask.id].resolved.as_ref().expect("resolved");
    assert_eq!(resolved.how, AskHow::Continued);
}
