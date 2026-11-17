mod common;

use std::time::Duration;

use common::*;
use parzi_core::store::Event;
use parzi_providers::TurnEnd;

/// A small model names a new session and sets a mathy message for display,
/// in tool-less turns beside the real one; the agent still gets what was typed.
#[tokio::test]
async fn a_small_model_names_the_session_and_tidies_math() {
    home("quick");
    let fake = Fake::with_small(
        "claude",
        script(|a: Agent| async move {
            let small = a.spec.model.as_deref() == Some("claude-mini");
            let asks = a.spec.instructions.clone().unwrap_or_default();
            if small && asks.starts_with("You name") {
                a.say("Energy equation derivation");
            } else if small && asks.starts_with("You set") {
                a.say("Derive $\\rho \\frac{De}{Dt}$ with $e = u + \\tfrac12 v^2$");
            } else {
                a.session("main-1");
                a.say("Start from the momentum equation.");
            }
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    orch.refresh_providers(&["claude".to_string()]).await;
    let typed = "Derive pDe/Dt with e = u + 1/2v^2";
    let (meta, _rx) = orch
        .spawn(
            "quick",
            "claude/claude-big",
            typed,
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    orch.name_soon(&meta.id, typed);
    orch.tidy_soon(&meta.id, typed);
    settle(&store, &meta.id).await;

    let mut tidy = None;
    for _ in 0..50 {
        let events = store.events(&meta.id).unwrap();
        tidy = events.iter().find_map(|e| match e {
            Event::Widget { fence, payload } if fence == "parzi-tidy" => Some(payload.clone()),
            _ => None,
        });
        if tidy.is_some() && store.get(&meta.id).unwrap().title == "Energy equation derivation" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    assert_eq!(
        store.get(&meta.id).unwrap().title,
        "Energy equation derivation"
    );
    let tidy = tidy.expect("a tidy version of the message");
    assert_eq!(tidy["n"], 0, "the first message");
    assert!(tidy["text"].as_str().unwrap().contains("\\frac{De}{Dt}"));

    let turns = fake.seen();
    let main = turns
        .iter()
        .find(|t| t.model.as_deref() == Some("claude-big"))
        .unwrap();
    assert!(main.prompt.contains(typed), "the agent got what was typed");
    assert!(
        turns
            .iter()
            .filter(|t| t.model.as_deref() == Some("claude-mini"))
            .all(|t| t.tools.is_none()),
        "the quick turns have no tools"
    );
}
