mod common;

use common::*;
use parzi_core::store::Event;
use parzi_providers::{ProviderEvent, TurnEnd};
use serde_json::json;

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
}
