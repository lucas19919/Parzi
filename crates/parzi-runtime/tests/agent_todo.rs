mod common;

use common::*;
use parzi_core::store::{Event, StepState};
use parzi_providers::{ProviderEvent, TurnEnd};
use serde_json::json;

#[tokio::test]
async fn near_misses_are_fitted_and_said_instead_of_failing() {
    home("agent-todo");
    let seen: std::sync::Arc<std::sync::Mutex<Vec<(bool, String)>>> = Default::default();
    let log = seen.clone();
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let log = log.clone();
            async move {
                let mut out = vec![];
                out.push(
                    a.parzi(
                        "todo.write",
                        json!({"items": [
                            {"title": "Read", "state": "completed"},
                            {"title": "Fix", "state": "in_progress"},
                        ]}),
                    )
                    .await,
                );
                out.push(a.parzi("todo.write", json!({"items": []})).await);
                out.push(
                    a.parzi(
                        "ui.offer_replies",
                        json!({"replies": [{"label": "Ship the release candidate to everyone now"}]}),
                    )
                    .await,
                );
                out.push(
                    a.parzi(
                        "session.rename",
                        json!({"title": "Fix the flaky login test on the nightly build"}),
                    )
                    .await,
                );
                *log.lock().unwrap() = out;
                a.say("done");
                Ok(TurnEnd::Completed)
            }
        }),
    );
    let (orch, store) = orch(&[fake]);
    let (meta, _rx) = orch
        .spawn("todo", "claude", "tidy up", None, "", "low", vec![], None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let out = seen.lock().unwrap().clone();
    assert!(out.iter().all(|(ok, _)| *ok), "{out:?}");
    assert!(out[1].1.contains("cleared"), "{}", out[1].1);
    assert!(
        out[2]
            .1
            .contains("so its button reads \"Ship the release candidate\""),
        "{}",
        out[2].1
    );
    assert!(out[3].1.contains("was cut to"), "{}", out[3].1);
    let todos: Vec<serde_json::Value> = store
        .events(&meta.id)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e {
            Event::Widget { fence, payload } if fence == "parzi-todo" => Some(payload),
            _ => None,
        })
        .collect();
    assert_eq!(
        todos,
        [
            json!({"items": [{"title": "Read", "state": "done"}, {"title": "Fix", "state": "active"}]}),
            json!({"items": []}),
        ]
    );
    let title = store.get(&meta.id).unwrap().title;
    assert_eq!(title, "Fix the flaky login test");
    assert_eq!(
        store.get(&meta.id).unwrap().plan,
        None,
        "an empty list clears the plan"
    );
}

#[tokio::test]
async fn the_agents_own_todo_list_is_the_sessions_checklist() {
    home("agent-todo");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.session("todo-1");
            let _ = a.events.send(ProviderEvent::ToolStarted {
                id: "t1".into(),
                name: "TodoWrite".into(),
                input: json!({"todos": [
                    {"content": "Clone the repos", "status": "in_progress", "activeForm": "Cloning"},
                    {"content": "Audit them", "status": "pending", "activeForm": "Auditing"},
                ]}),
            });
            a.say("on it");
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(&[fake]);
    let (meta, _rx) = orch
        .spawn(
            "todo",
            "claude",
            "pull and audit",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let events = store.events(&meta.id).unwrap();
    let todo = events.iter().find_map(|e| match e {
        Event::Widget { fence, payload } if fence == "parzi-todo" => Some(payload.clone()),
        _ => None,
    });
    assert_eq!(
        todo,
        Some(json!({"items": [
            {"title": "Clone the repos", "state": "active"},
            {"title": "Audit them", "state": "pending"},
        ]}))
    );
    let plan = store
        .get(&meta.id)
        .unwrap()
        .plan
        .expect("kept on the session");
    assert_eq!(
        plan.items
            .iter()
            .map(|i| (i.title.as_str(), i.state))
            .collect::<Vec<_>>(),
        [
            ("Clone the repos", StepState::Active),
            ("Audit them", StepState::Pending)
        ]
    );
}

/// The plan lives on the session: an agent that starts on the thread fresh
/// (here, after a switch) is handed it.
#[tokio::test]
async fn the_plan_stays_with_the_session_and_reaches_a_new_agent() {
    home("agent-todo");
    let claude = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.session("c-1");
            let _ = a
                .parzi(
                    "todo.write",
                    json!({"items": [
                        {"title": "Write the lexer", "state": "done"},
                        {"title": "Write the parser", "state": "active"},
                    ]}),
                )
                .await;
            a.say("lexer done");
            Ok(TurnEnd::Completed)
        }),
    );
    let grok = Fake::new(
        "grok",
        script(|a: Agent| async move {
            a.session("g-1");
            a.say("on the parser");
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(&[claude, grok.clone()]);
    let (meta, _rx) = orch
        .spawn("todo", "claude", "build it", None, "", "low", vec![], None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    assert_eq!(store.get(&meta.id).unwrap().plan.map(|p| p.done()), Some(1));
    orch.send_to(
        &meta.id,
        "go on",
        None,
        "",
        "low",
        vec![],
        Some("grok".into()),
        None,
    )
    .await
    .unwrap();
    settle(&store, &meta.id).await;
    let prompt = grok.seen().pop().unwrap().prompt;
    assert!(prompt.contains("<session-plan>"), "{prompt}");
    assert!(
        prompt.contains("[x] Write the lexer") && prompt.contains("[>] Write the parser"),
        "{prompt}"
    );
}
