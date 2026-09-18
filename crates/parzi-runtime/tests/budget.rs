//! R-4: a run stops at its spend cap, says why, and stays available (Idle)
//! instead of being killed — §15.6 "lanes pause, the header explains".

mod common;

use std::sync::Arc;

use common::*;
use parzi_core::config::{Budget, ParziConfig};
use parzi_core::project::{Project, Roster, Status};
use parzi_core::store::{Event, SessionStatus};
use parzi_providers::TurnEnd;
use parzi_runtime::handler::RunEvent;
use parzi_runtime::orchestrator::{run_note, BUDGET_NOTE};
use parzi_runtime::tools::{Approval, Approver, ToolCallInfo};

struct Allow;
#[async_trait::async_trait]
impl Approver for Allow {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

/// Reports 1 000 tokens (and a cent) per step and works until stopped.
fn spender() -> Arc<Fake> {
    Fake::new(
        "claude",
        script(|a: Agent| async move {
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
    )
}

#[test]
fn tightest_budget_wins_and_reports_the_reason() {
    let config = Budget {
        max_cost_usd: Some(10.0),
        max_tokens: None,
    };
    let project = Budget {
        max_cost_usd: Some(2.0),
        max_tokens: Some(5_000),
    };
    let both = config.tightest(project);
    assert_eq!(both.max_cost_usd, Some(2.0));
    assert_eq!(both.max_tokens, Some(5_000));
    assert!(Budget::default().is_unlimited());
    assert!(Budget::default().exceeded(1_000_000, 500.0).is_none());
    assert!(both.exceeded(5_000, 0.0).unwrap().contains("token budget"));
    assert!(both.exceeded(0, 2.5).unwrap().contains("cost budget"));
}

#[tokio::test]
async fn token_budget_pauses_the_run_and_marks_it() {
    home("budget");
    let mut cfg = ParziConfig::default();
    cfg.budget = Budget {
        max_cost_usd: None,
        max_tokens: Some(2_500),
    };
    let (orch, store) = orch_with(cfg, &[spender()]);
    let (meta, mut rx) = orch
        .spawn(
            "t",
            "",
            "claude",
            "spend it",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    let mut notices = vec![];
    while let Some(ev) = rx.recv().await {
        if let RunEvent::Notice { text } = ev {
            notices.push(text);
        }
    }
    assert_eq!(settle(&store, &meta.id).await.status, SessionStatus::Idle);
    assert_eq!(run_note(&meta.id).as_deref(), Some(BUDGET_NOTE));
    assert!(
        notices.iter().any(|n| n.contains("token budget")),
        "the pause is announced: {notices:?}"
    );
    let said = events(&store, &meta.id)
        .iter()
        .any(|e| matches!(e, Event::System { text } if text.contains("Paused")));
    assert!(said, "the thread says why it stopped");
    let spent = store.get(&meta.id).unwrap();
    assert!(
        spent.tokens_in + spent.tokens_out < 5_000,
        "the run stopped near the cap, not after it: {}",
        spent.tokens_in + spent.tokens_out
    );
}

/// A run dispatched for a project honours that project's `budget:` line —
/// with nothing left, the first turn is never bought.
#[tokio::test]
async fn project_budget_is_read_from_project_md() {
    home("budget");
    let project = Project {
        slug: "spent".into(),
        title: "Spent".into(),
        workspace: "budgets".into(),
        repos: vec![],
        roster: Roster {
            header: "claude".into(),
            orchestrator: "claude".into(),
            coder: "claude".into(),
        },
        budget_usd: Some(0.0),
        status: Status::Running,
        critical: vec![],
        why: "prove the cap".into(),
        what: vec![],
        constraints: vec![],
    };
    std::fs::create_dir_all(parzi_core::project::dir("budgets", "spent")).unwrap();
    parzi_core::project::save(&project).unwrap();
    let fake = spender();
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, mut rx) = orch
        .spawn_in_project(
            Some(("budgets".into(), "spent".into())),
            "t",
            "",
            "claude",
            "do the work",
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    let mut paused = false;
    while let Some(ev) = rx.recv().await {
        if let RunEvent::Notice { text } = ev {
            paused |= text.contains("cost budget");
        }
    }
    assert!(paused, "a project with no budget left pauses its lane");
    assert_eq!(settle(&store, &meta.id).await.status, SessionStatus::Idle);
    assert_eq!(run_note(&meta.id).as_deref(), Some(BUDGET_NOTE));
    assert!(fake.seen().is_empty(), "no turn was bought");
}
