mod common;

use common::*;
use parzi_providers::TurnEnd;

/// Switching agents mid-thread (say, at a usage limit) keeps the thread: the
/// same session goes on, the new agent starts without the old agent's resume
/// and is handed the conversation so far.
#[tokio::test]
async fn another_agent_takes_over_the_same_thread_with_its_history() {
    home("agent-switch");
    let claude = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.session("claude-1");
            a.say("The parser is half done; the lexer works.");
            Ok(TurnEnd::Completed)
        }),
    );
    let opencode = Fake::new(
        "opencode",
        script(|a: Agent| async move {
            a.session("opencode-1");
            a.say("Carrying on with the parser.");
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(&[claude.clone(), opencode.clone()]);
    let (meta, _rx) = orch
        .spawn(
            "switch",
            "claude",
            "build the parser",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;

    orch.send_to(
        &meta.id,
        "go on",
        None,
        "",
        "low",
        vec![],
        Some("opencode".into()),
        None,
    )
    .await
    .expect("a switch is not refused");
    settle(&store, &meta.id).await;

    let turn = opencode.seen().pop().expect("the new agent ran");
    assert!(
        turn.resume.is_none(),
        "the old agent's session is not resumed by another agent"
    );
    assert!(
        turn.prompt.contains("<earlier-conversation>"),
        "history handed over"
    );
    assert!(
        turn.prompt.contains("The parser is half done"),
        "with what was said"
    );
    assert!(turn.prompt.ends_with("go on"), "then the new message");
    assert!(
        store.get(&meta.id).unwrap().model.starts_with("opencode"),
        "the thread shows its new agent"
    );
    let threads = store.list().unwrap();
    assert_eq!(threads.len(), 1, "no second session");
}
