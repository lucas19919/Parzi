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

    // Back to Claude: it resumes its own session and hears only what happened
    // while it was away, not the whole thread again.
    orch.send_to(
        &meta.id,
        "check the tests",
        None,
        "",
        "low",
        vec![],
        Some("claude".into()),
        None,
    )
    .await
    .expect("switching back is not refused");
    settle(&store, &meta.id).await;
    let back = claude.seen().pop().expect("claude ran again");
    assert_eq!(
        back.resume
            .as_ref()
            .and_then(|r| r.get("session_id"))
            .and_then(|v| v.as_str()),
        Some("claude-1"),
        "its own session resumes: {:?}",
        back.resume
    );
    assert!(
        back.prompt.contains("<while-you-were-away>"),
        "{}",
        back.prompt
    );
    assert!(back.prompt.contains("Carrying on with the parser."));
    assert!(
        !back.prompt.contains("The parser is half done"),
        "what it said itself is not repeated"
    );
    assert!(!back.prompt.contains("<earlier-conversation>"));
    assert!(back.prompt.ends_with("check the tests"));
}
