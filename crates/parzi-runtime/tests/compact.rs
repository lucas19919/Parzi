mod common;

use common::*;
use parzi_core::store::Event;
use parzi_providers::{ErrorClass, ProviderError, ProviderEvent, TurnEnd};

fn checkpoints(events: &[Event]) -> Vec<(String, Option<u64>, Option<u64>)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Checkpoint {
                summary,
                before,
                after,
            } => Some((summary.clone(), *before, *after)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn an_agent_that_compacts_is_asked_to_and_says_how_much() {
    home("compact");
    let fake = Fake::new(
        "grok",
        script(|a: Agent| async move {
            a.session("grok-1");
            if a.spec.compact {
                let _ = a.events.send(ProviderEvent::Compacted {
                    before: Some(90_000),
                    after: Some(12_000),
                });
            } else {
                a.say("done");
            }
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, _rx) = orch
        .spawn("compact", "grok", "build it", None, "", "low", vec![], None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    orch.compact(&meta.id, "the API shape").await.unwrap();
    settle(&store, &meta.id).await;
    let turns = fake.seen();
    assert!(turns[1].compact, "the agent's own way");
    assert_eq!(turns[1].prompt, "the API shape", "what to keep");
    let events = store.events(&meta.id).unwrap();
    assert_eq!(
        checkpoints(&events),
        vec![(String::new(), Some(90_000), Some(12_000))]
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::User { text } if text.contains("compact"))),
        "no /compact message in the thread"
    );
    assert_eq!(store.get(&meta.id).unwrap().context_tokens, 12_000);
}

#[tokio::test]
async fn an_agent_that_cannot_compact_summarizes_and_goes_on_from_the_summary() {
    home("compact");
    let fake = Fake::new(
        "cursor",
        script(|a: Agent| async move {
            if a.spec.compact {
                return Err(ProviderError::new(
                    ErrorClass::Unsupported,
                    "Cursor cannot compact its conversation",
                ));
            }
            a.session("cursor-1");
            if a.spec.prompt.contains("Summarize this conversation") {
                a.say("Building the parser; lexer done, tests next.");
            } else {
                a.say("ok");
            }
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, _rx) = orch
        .spawn(
            "compact",
            "cursor",
            "SECRET-EARLY-DETAIL",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    orch.compact(&meta.id, "").await.unwrap();
    settle(&store, &meta.id).await;
    let events = store.events(&meta.id).unwrap();
    let marks = checkpoints(&events);
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].0, "Building the parser; lexer done, tests next.");
    assert!(
        !events.iter().any(
            |e| matches!(e, Event::Assistant { text, .. } if text.starts_with("Building the parser"))
        ),
        "the summary is the checkpoint, not a reply"
    );
    orch.send_to(&meta.id, "go on", None, "", "low", vec![], None, None)
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let next = fake.seen().pop().unwrap();
    assert!(next.resume.is_none(), "a new conversation");
    assert!(
        next.prompt.contains("Building the parser; lexer done"),
        "{}",
        next.prompt
    );
    assert!(
        !next.prompt.contains("SECRET-EARLY-DETAIL"),
        "what the summary stands for stays out: {}",
        next.prompt
    );
}

#[tokio::test]
async fn compacting_on_its_own_mid_turn_leaves_a_mark_where_it_happened() {
    home("compact");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.session("claude-1");
            a.say("first half");
            let _ = a.events.send(ProviderEvent::Compacted {
                before: Some(180_000),
                after: None,
            });
            a.say("second half");
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(&[fake]);
    let (meta, _rx) = orch
        .spawn(
            "compact",
            "claude",
            "long job",
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
    let at = |want: &dyn Fn(&Event) -> bool| events.iter().position(|e| want(e)).unwrap();
    let first = at(&|e| matches!(e, Event::Assistant { text, .. } if text == "first half"));
    let mark = at(&|e| matches!(e, Event::Checkpoint { .. }));
    let second = at(&|e| matches!(e, Event::Assistant { text, .. } if text == "second half"));
    assert!(first < mark && mark < second, "{events:?}");
    assert_eq!(
        checkpoints(&events),
        vec![(String::new(), Some(180_000), None)]
    );
}
