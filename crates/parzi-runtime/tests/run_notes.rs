//! What a run owes the person when the agent cannot keep a promise: a
//! read-only lane refuses an agent that acts unasked, a thread says once
//! that its agent is not fully gated, a lost vendor conversation goes on
//! with the history, and a dollar cap that cannot bite is said to be so.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use parzi_core::config::{Budget, ParziConfig};
use parzi_core::store::{Event, SessionStatus};
use parzi_providers::{ErrorClass, ProviderError, TurnEnd};

fn notes(store: &parzi_core::store::SessionStore, id: &str, needle: &str) -> usize {
    events(store, id)
        .iter()
        .filter(|e| matches!(e, Event::System { text } if text.contains(needle)))
        .count()
}

#[tokio::test]
async fn a_read_only_lane_refuses_an_agent_that_acts_unasked() {
    home("notes");
    let agent = Fake::ungated(
        "claude",
        script(|a: Agent| async move {
            a.say("should never run");
            Ok(TurnEnd::Completed)
        }),
    );
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "deny".into();
    let (orch, store) = orch_with(cfg, std::slice::from_ref(&agent));
    let err = orch
        .spawn(
            "t",
            "",
            "claude",
            "look around",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("read-only"), "{err}");
    assert!(agent.seen().is_empty(), "no turn was started");
    let sid = store.list().unwrap()[0].id.clone();
    assert_eq!(store.get(&sid).unwrap().status, SessionStatus::Idle);
    assert_eq!(notes(&store, &sid, "run failed to start"), 1);
}

#[tokio::test]
async fn a_thread_says_once_that_its_agent_is_not_fully_gated() {
    home("notes");
    let agent = Fake::ungated(
        "claude",
        script(|a: Agent| async move {
            a.say("done");
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&agent));
    let (meta, _rx) = orch
        .spawn("t", "", "claude", "first", None, "", "low", vec![], None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    drop(
        orch.send_to(&meta.id, "second", None, "", "low", vec![], None, None)
            .await
            .unwrap(),
    );
    settle(&store, &meta.id).await;
    assert_eq!(agent.seen().len(), 2);
    assert_eq!(notes(&store, &meta.id, "without asking Parzi first"), 1);
}

/// The vendor lost the conversation it was asked to resume: once, the run
/// starts a new one and hands it the thread so far.
#[tokio::test]
async fn a_lost_conversation_goes_on_with_the_history() {
    home("notes");
    let calls = Arc::new(AtomicUsize::new(0));
    let n = calls.clone();
    let agent = Fake::new(
        "claude",
        script(move |a: Agent| {
            let n = n.clone();
            async move {
                match n.fetch_add(1, Ordering::SeqCst) {
                    0 => {
                        a.session("vendor-1");
                        a.say("the answer is 42");
                        Ok(TurnEnd::Completed)
                    }
                    1 => Err(ProviderError::new(
                        ErrorClass::SessionLost,
                        "No conversation found with session ID: vendor-1",
                    )),
                    _ => {
                        a.session("vendor-2");
                        a.say("continued");
                        Ok(TurnEnd::Completed)
                    }
                }
            }
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&agent));
    let (meta, _rx) = orch
        .spawn(
            "t",
            "",
            "claude",
            "what is it?",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    drop(
        orch.send_to(&meta.id, "and then?", None, "", "low", vec![], None, None)
            .await
            .unwrap(),
    );
    assert_eq!(settle(&store, &meta.id).await.status, SessionStatus::Done);
    let turns = agent.seen();
    assert_eq!(turns.len(), 3, "first turn, the lost resume, the new one");
    assert!(turns[1].resume.is_some(), "the second turn tried to resume");
    assert!(
        turns[2].resume.is_none(),
        "the retry starts a new conversation"
    );
    assert!(
        turns[2].prompt.contains("the answer is 42") && turns[2].prompt.contains("and then?"),
        "the new conversation gets the thread so far: {}",
        turns[2].prompt
    );
    assert_eq!(
        notes(&store, &meta.id, "no longer had this conversation"),
        1
    );
    assert!(!events(&store, &meta.id)
        .iter()
        .any(|e| matches!(e, Event::Error { .. })));
    assert_eq!(last_reply(&store, &meta.id), "continued");
}

#[tokio::test]
async fn a_dollar_cap_the_agent_cannot_price_is_said_to_be_so() {
    home("notes");
    let agent = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.usage(100, 20, None);
            a.say("done on a plan");
            Ok(TurnEnd::Completed)
        }),
    );
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "auto".into();
    cfg.budget = Budget {
        max_cost_usd: Some(5.0),
        max_tokens: None,
    };
    let (orch, store) = orch_with(cfg, std::slice::from_ref(&agent));
    let (meta, _rx) = orch
        .spawn("t", "", "claude", "work", None, "", "low", vec![], None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    assert_eq!(notes(&store, &meta.id, "dollar cap cannot stop it"), 1);
}
