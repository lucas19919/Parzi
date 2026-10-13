//! Agents write the plan, read it, link their session to an item and name
//! their session; a run opens with the plan only when it changed since the
//! conversation last saw it, and never in the instructions.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use parzi_core::brain::Vault;
use parzi_core::plan::{self, ItemWrite, PlanState, PlanWrite};
use parzi_providers::TurnEnd;
use serde_json::json;

fn vault() -> Vault {
    Vault::open().unwrap()
}

#[tokio::test]
async fn an_agent_plans_marks_an_item_active_and_names_its_session() {
    home("plans");
    let seen: Arc<Mutex<Vec<(bool, String)>>> = Arc::default();
    let log = seen.clone();
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let log = log.clone();
            async move {
                let mut out = vec![];
                out.push(
                    a.parzi(
                        "plan.write",
                        json!({
                            "title": "Next release",
                            "goal": "A calmer sidebar",
                            "phases": ["Now", "Next"],
                            "items": [
                                {"id": "names", "title": "Session names", "phase": "Now", "state": "active", "branch": "plan/names"},
                                {"id": "panel", "title": "Plan panel", "phase": "Next", "needs": ["names"]},
                            ],
                        }),
                    )
                    .await,
                );
                out.push(
                    a.parzi(
                        "plan.write",
                        json!({"items": [{"id": "x", "needs": ["ghost"], "title": "X"}]}),
                    )
                    .await,
                );
                out.push(a.parzi("plan.read", json!({})).await);
                out.push(
                    a.parzi("session.rename", json!({"title": "Release planning"}))
                        .await,
                );
                out.push(
                    a.parzi("session.rename", json!({"title": "x".repeat(61)}))
                        .await,
                );
                out.push(
                    a.parzi(
                        "session.rename",
                        json!({"title": "One two three four five six"}),
                    )
                    .await,
                );
                *log.lock().unwrap() = out;
                a.say("planned");
                Ok(TurnEnd::Completed)
            }
        }),
    );
    // Ask mode with nobody to ask: a card would deny, so none is shown.
    let mut cfg = parzi_core::config::ParziConfig::default();
    cfg.lanes.default_mode = "ask".into();
    let (orch, store) = orch_with(cfg, &[fake]);
    let (meta, _rx) = orch
        .spawn(
            "plans-demo",
            "build",
            "claude",
            "lets plan the next release",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    assert_eq!(meta.title, "Plan the next release");
    settle(&store, &meta.id).await;
    let out = seen.lock().unwrap().clone();
    assert!(out[0].0, "{}", out[0].1);
    assert!(
        out[0]
            .1
            .contains("Saved projects/plans-demo/plan.md (2 items, 0 of 2 done)."),
        "{}",
        out[0].1
    );
    assert!(
        out[0].1.contains("This session now works on `names`."),
        "{}",
        out[0].1
    );
    assert!(
        !out[1].0 && out[1].1.contains("not an item of this plan"),
        "{:?}",
        out[1]
    );
    assert!(out[2].0, "{}", out[2].1);
    assert!(
        out[2]
            .1
            .contains("- names: Session names [active], branch plan/names"),
        "{}",
        out[2].1
    );
    assert!(
        out[2].1.contains(&format!("Sessions: {} ", &meta.id[..8])),
        "the session is linked: {}",
        out[2].1
    );
    assert!(out[3].0, "{}", out[3].1);
    assert!(
        !out[4].0 && out[4].1.contains("keep it to 60"),
        "{:?}",
        out[4]
    );
    assert!(
        !out[5].0 && out[5].1.contains("2 to 5 words"),
        "the automatic names' rule: {:?}",
        out[5]
    );

    let m = store.get(&meta.id).unwrap();
    assert_eq!(m.plan_item.as_deref(), Some("names"));
    assert_eq!(m.title, "Release planning");
    let p = plan::load(&vault(), "plans-demo").plan.unwrap();
    assert_eq!(p.title, "Next release");
    assert_eq!(p.items.len(), 2, "the refused write changed nothing");
    assert_eq!(p.item("names").unwrap().state, PlanState::Active);
    assert_eq!(p.by, meta.id);
    let events = store.events(&meta.id).unwrap();
    let calls: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            parzi_core::store::Event::ToolCall { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(calls.contains(&"plan.write".to_string()), "{calls:?}");
}

#[tokio::test]
async fn a_session_without_a_project_has_no_plan_to_write() {
    home("plans");
    let seen: Arc<Mutex<Option<(bool, String)>>> = Arc::default();
    let log = seen.clone();
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let log = log.clone();
            async move {
                let r = a
                    .parzi(
                        "plan.write",
                        json!({"title": "T", "items": [{"id": "a", "title": "A"}]}),
                    )
                    .await;
                *log.lock().unwrap() = Some(r);
                a.say("ok");
                Ok(TurnEnd::Completed)
            }
        }),
    );
    let (orch, store) = orch(&[fake]);
    let (meta, _rx) = orch
        .spawn(
            "default",
            "build",
            "claude",
            "plan it",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let (ok, text) = seen.lock().unwrap().clone().unwrap();
    assert!(!ok);
    assert!(text.contains("no project to plan in"), "{text}");
}

#[tokio::test]
async fn the_plan_opens_a_turn_only_when_it_changed() {
    home("plans");
    let slug = "brief-demo";
    let write = |title: &str| PlanWrite {
        title: Some(title.into()),
        items: Some(vec![ItemWrite {
            id: "names".into(),
            title: Some("Session names".into()),
            ..ItemWrite::default()
        }]),
        ..PlanWrite::default()
    };
    plan::write(&vault(), slug, write("First"), "you").unwrap();
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.session("vendor-brief");
            a.say("ok");
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, _rx) = orch
        .spawn(
            slug,
            "build",
            "claude",
            "work on it",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    store.set_plan_item(&meta.id, Some("names")).unwrap();
    for prompt in ["again", "and again"] {
        orch.send_to(&meta.id, prompt, None, "", "low", vec![], None, None)
            .await
            .unwrap();
        settle(&store, &meta.id).await;
    }
    plan::write(
        &vault(),
        slug,
        PlanWrite {
            items: Some(vec![ItemWrite {
                id: "names".into(),
                state: Some("done".into()),
                ..ItemWrite::default()
            }]),
            ..PlanWrite::default()
        },
        "you",
    )
    .unwrap();
    orch.send_to(&meta.id, "once more", None, "", "low", vec![], None, None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;

    let turns = fake.seen();
    assert_eq!(turns.len(), 4);
    let block = format!("<parzi:plan project=\"{slug}\">");
    assert!(turns[0].prompt.starts_with(&block), "{}", turns[0].prompt);
    assert!(
        turns[0].prompt.contains("Plan \"First\". 0 of 1 done."),
        "{}",
        turns[0].prompt
    );
    assert!(
        turns[0].prompt.ends_with("work on it"),
        "the request comes after"
    );
    assert!(
        turns[1]
            .prompt
            .contains("This session works on item names."),
        "linked to an item since: told again, with the item: {}",
        turns[1].prompt
    );
    assert!(
        !turns[2].prompt.contains(&block),
        "a resumed conversation that saw it is not told again: {}",
        turns[2].prompt
    );
    assert!(
        turns[3].prompt.contains(&block),
        "the plan changed: {}",
        turns[3].prompt
    );
    assert!(
        turns[3].prompt.contains("1 of 1 done."),
        "{}",
        turns[3].prompt
    );
    for t in &turns {
        let instructions = t.instructions.clone().unwrap_or_default();
        assert!(
            !instructions.contains("<parzi:plan project="),
            "never in the instructions"
        );
        assert!(
            instructions.contains("## plans"),
            "both lanes get the Plans section"
        );
    }
    let shown: Vec<String> = store
        .events(&meta.id)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e {
            parzi_core::store::Event::User { text } => Some(text),
            _ => None,
        })
        .collect();
    assert!(
        shown.iter().all(|t| !t.contains("<parzi:plan project=")),
        "never in the transcript"
    );
}

#[tokio::test]
async fn a_plan_a_turn_never_showed_is_sent_again() {
    home("plans");
    let slug = "brief-retry";
    plan::write(
        &vault(),
        slug,
        PlanWrite {
            title: Some("Retry".into()),
            items: Some(vec![ItemWrite {
                id: "a".into(),
                title: Some("A".into()),
                ..ItemWrite::default()
            }]),
            ..PlanWrite::default()
        },
        "you",
    )
    .unwrap();
    let calls = Arc::new(Mutex::new(0));
    let n = calls.clone();
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let n = n.clone();
            async move {
                a.session("vendor-retry");
                let first = {
                    let mut n = n.lock().unwrap();
                    *n += 1;
                    *n == 1
                };
                if first {
                    // The conversation exists, but the turn never finished.
                    return Err(parzi_providers::ProviderError::process("dropped"));
                }
                a.say("ok");
                Ok(TurnEnd::Completed)
            }
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, _rx) = orch
        .spawn(
            slug,
            "build",
            "claude",
            "start",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    orch.send_to(&meta.id, "go on", None, "", "low", vec![], None, None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    // A /compact drops what the conversation saw: the plan goes again.
    orch.compact(&meta.id, "").await.unwrap();
    settle(&store, &meta.id).await;
    orch.send_to(
        &meta.id,
        "after compact",
        None,
        "",
        "low",
        vec![],
        None,
        None,
    )
    .await
    .unwrap();
    settle(&store, &meta.id).await;
    let turns = fake.seen();
    let block = format!("<parzi:plan project=\"{slug}\">");
    assert_eq!(turns.len(), 4);
    assert!(turns[0].prompt.contains(&block));
    assert!(
        turns[1].prompt.contains(&block),
        "the failed turn never showed it: {}",
        turns[1].prompt
    );
    assert!(
        turns[2].prompt.starts_with("/compact"),
        "{}",
        turns[2].prompt
    );
    assert!(
        turns[3].prompt.contains(&block),
        "compacted away: {}",
        turns[3].prompt
    );
}

#[tokio::test]
async fn a_read_only_lane_reads_the_plan_and_changes_nothing() {
    home("plans");
    let slug = "read-only-plan";
    plan::write(
        &vault(),
        slug,
        PlanWrite {
            title: Some("Locked".into()),
            items: Some(vec![ItemWrite {
                id: "a".into(),
                title: Some("A".into()),
                ..ItemWrite::default()
            }]),
            ..PlanWrite::default()
        },
        "you",
    )
    .unwrap();
    let seen: Arc<Mutex<Vec<(bool, String)>>> = Arc::default();
    let log = seen.clone();
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let log = log.clone();
            async move {
                let read = a.parzi("plan.read", json!({})).await;
                let write = a
                    .parzi(
                        "plan.write",
                        json!({"items": [{"id": "a", "state": "done"}]}),
                    )
                    .await;
                let rename = a
                    .parzi("session.rename", json!({"title": "Renamed here"}))
                    .await;
                *log.lock().unwrap() = vec![read, write, rename];
                a.say("ok");
                Ok(TurnEnd::Completed)
            }
        }),
    );
    let mut cfg = parzi_core::config::ParziConfig::default();
    cfg.lanes.default_mode = "deny".into();
    let (orch, store) = orch_with(cfg, &[fake]);
    let (meta, _rx) = orch
        .spawn(
            slug,
            "build",
            "claude",
            "look at the plan",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let out = seen.lock().unwrap().clone();
    assert!(out[0].0, "plan.read reads: {}", out[0].1);
    assert!(!out[1].0 && out[1].1.contains("denied"), "{:?}", out[1]);
    assert!(!out[2].0 && out[2].1.contains("denied"), "{:?}", out[2]);
    let p = plan::load(&vault(), slug).plan.unwrap();
    assert_eq!(p.item("a").unwrap().state, PlanState::Planned);
    assert_eq!(store.get(&meta.id).unwrap().title, "Look at the plan");
}
