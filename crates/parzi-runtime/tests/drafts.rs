mod common;

use common::*;
use parzi_core::store::SessionStatus;
use parzi_providers::TurnEnd;
use serde_json::json;

/// An agent writes work down for later: a draft in its project, with a brief
/// and a plan, that never runs until the person starts it.
#[tokio::test]
async fn a_drafted_session_waits_and_starts_on_the_first_send() {
    home("drafts");
    let said: std::sync::Arc<std::sync::Mutex<Vec<(bool, String)>>> = Default::default();
    let log = said.clone();
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let log = log.clone();
            async move {
                a.session("c-1");
                if a.spec.prompt.contains("fix the login") {
                    let draft = a
                        .parzi(
                            "session.draft",
                            json!({
                                "title": "Retry flaky sync",
                                "brief": "Make the sync retry with backoff, after the login fix lands.",
                                "plan": [{"title": "Find the sync call"}, {"title": "Add backoff"}],
                            }),
                        )
                        .await;
                    let list = a.parzi("session.list", json!({})).await;
                    *log.lock().unwrap() = vec![draft, list];
                }
                a.say("ok");
                Ok(TurnEnd::Completed)
            }
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, _rx) = orch
        .spawn(
            "drafts",
            "claude",
            "fix the login",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;

    let out = said.lock().unwrap().clone();
    assert!(out[0].0, "{:?}", out[0]);
    assert!(
        out[1].1.contains("\"status\":\"draft\""),
        "the list shows drafts: {}",
        out[1].1
    );
    assert!(
        out[1].1.contains("0 of 2 done"),
        "with their plan: {}",
        out[1].1
    );

    let draft = store
        .list()
        .unwrap()
        .into_iter()
        .find(|m| m.status == SessionStatus::Draft)
        .expect("a draft in the store");
    assert_eq!(draft.title, "Retry flaky sync");
    assert_eq!(draft.project, "drafts", "in the caller's project");
    assert!(draft.parent_id.is_none(), "its own session, not a helper");
    assert_eq!(draft.plan.as_ref().map(|p| p.items.len()), Some(2));
    assert!(store.events(&draft.id).unwrap().is_empty(), "nothing ran");
    let turns_before = fake.seen().len();

    let brief = draft.brief.clone().unwrap();
    orch.send_to(&draft.id, &brief, None, "", "low", vec![], None, None)
        .await
        .unwrap();
    let started = settle(&store, &draft.id).await;
    assert_eq!(fake.seen().len(), turns_before + 1, "the draft ran once");
    assert!(fake
        .seen()
        .last()
        .unwrap()
        .prompt
        .contains("after the login fix lands"));
    assert_ne!(started.status, SessionStatus::Draft);
    assert!(started.brief.is_none(), "the brief was delivered");
    assert_eq!(
        started.plan.map(|p| p.items.len()),
        Some(2),
        "the plan carries into the run"
    );
}
