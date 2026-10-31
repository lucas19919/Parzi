mod common;

use std::path::Path;
use std::sync::Arc;

use common::*;
use parzi_core::store::{
    Check, FileTouch, RunEndKind, SessionMeta, SessionStatus, SessionStore, Via,
};
use parzi_providers::{ProviderError, TurnEnd};
use serde_json::json;

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q"]);
    git(d.path(), &["config", "user.email", "t@example.com"]);
    git(d.path(), &["config", "user.name", "t"]);
    git(d.path(), &["config", "commit.gpgsign", "false"]);
    git(d.path(), &["checkout", "-q", "-b", "plan/names"]);
    std::fs::write(d.path().join("kept.txt"), "one").unwrap();
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "start"]);
    d
}

async fn recorded(store: &SessionStore, id: &str) -> SessionMeta {
    for _ in 0..200 {
        let m = store.get(id).unwrap();
        if m.last_run.is_some() {
            return m;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the run of {id} was not recorded");
}

#[tokio::test]
async fn a_run_keeps_its_summary_files_checks_and_branch_on_the_session() {
    home("outcome");
    let dir = repo();
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            let cwd = a.spec.cwd.clone();
            a.own_tool(
                "1",
                "Edit",
                json!({"file_path": "src/omnibar.ts"}),
                || async { (true, "ok".to_string()) },
            )
            .await;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            std::fs::write(cwd.join("generated.txt"), "made").unwrap();
            a.own_tool(
                "2",
                "mcp__parzi__shell_exec",
                json!({"cmd": "npm test"}),
                || async { (false, "1 failed".to_string()) },
            )
            .await;
            a.own_tool(
                "3",
                "mcp__parzi__shell_exec",
                json!({"cmd": "npm test"}),
                || async { (true, "all passed".to_string()) },
            )
            .await;
            a.usage(1_000, 200, Some(0.05));
            a.say("## Fixed\nThe omnibar grows with its text now.");
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(&[fake]);
    let cwd = dir.path().display().to_string();
    let (meta, _rx) = orch
        .spawn(
            "outcome-tests",
            "claude",
            "hey, can you please fix the omnibar autosize?",
            None,
            &cwd,
            "high",
            vec![],
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        meta.title, "Fix the omnibar autosize",
        "a readable first name"
    );
    let m = recorded(&store, &meta.id).await;
    let out = m.last_run.unwrap();
    assert_eq!(out.end, RunEndKind::Completed);
    assert_eq!(out.summary, "Fixed The omnibar grows with its text now.");
    assert_eq!(
        out.files,
        [
            FileTouch {
                path: "src/omnibar.ts".into(),
                via: Via::Tool
            },
            FileTouch {
                path: "generated.txt".into(),
                via: Via::Git
            },
        ]
    );
    assert_eq!(out.files_more, 0);
    assert_eq!(
        out.checks,
        [Check {
            cmd: "npm test".into(),
            ok: true
        }],
        "a rerun counts once, with its latest result"
    );
    assert_eq!(out.cost_usd, Some(0.05));
    assert!(out.ended > 0);
    assert_eq!(m.branch, "plan/names");
    assert_eq!(m.effort, "high");
}

#[tokio::test]
async fn a_failed_run_says_why_and_a_run_that_never_started_says_so() {
    home("outcome");
    let fake = Fake::new(
        "claude",
        script(
            |_a: Agent| async move { Err(ProviderError::process("the agent crashed on start")) },
        ),
    );
    let (orch, store) = orch(&[fake]);
    let (meta, _rx) = orch
        .spawn(
            "outcome-tests",
            "claude",
            "Plan the next release",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    let m = recorded(&store, &meta.id).await;
    let out = m.last_run.unwrap();
    assert_eq!(out.end, RunEndKind::Failed);
    assert!(
        out.summary.contains("the agent crashed on start"),
        "{}",
        out.summary
    );
    assert_eq!(m.branch, "", "no repo, no branch");

    let err = orch
        .send_to(
            &meta.id,
            "again",
            None,
            "",
            "low",
            vec![],
            Some("nobody/x".into()),
            None,
        )
        .await;
    assert!(err.is_err());
    let out = store.get(&meta.id).unwrap().last_run.unwrap();
    assert_eq!(out.end, RunEndKind::NotStarted);
    assert!(out.summary.starts_with("Didn't start: "), "{}", out.summary);
}

#[tokio::test]
async fn a_compact_turn_leaves_the_last_run_alone() {
    home("outcome");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.session("vendor-compact");
            a.say(if a.spec.compact {
                "compacted"
            } else {
                "first answer"
            });
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(&[fake]);
    let (meta, _rx) = orch
        .spawn(
            "outcome-tests",
            "claude",
            "say hi",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    let first = recorded(&store, &meta.id).await.last_run.unwrap();
    assert_eq!(first.summary, "first answer");
    orch.compact(&meta.id, "").await.unwrap();
    settle(&store, &meta.id).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let after = store.get(&meta.id).unwrap().last_run.unwrap();
    assert_eq!(after, first, "housekeeping is not the session's run");
    let _ = Arc::strong_count(&orch);
}

#[tokio::test]
async fn a_failed_compact_launch_leaves_the_last_run_alone() {
    home("outcome");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.session("vendor-compact-fail");
            a.say("first answer");
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(&[fake]);
    let (meta, _rx) = orch
        .spawn(
            "outcome-tests",
            "claude",
            "say hi",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    let first = recorded(&store, &meta.id).await.last_run.unwrap();
    let mut cfg = orch.config();
    cfg.providers.entry("claude".into()).or_default().enabled = false;
    orch.apply_config(cfg).await;
    assert!(orch.compact(&meta.id, "").await.is_err());
    let after = store.get(&meta.id).unwrap().last_run.unwrap();
    assert_eq!(
        after, first,
        "a housekeeping run that failed is not the run"
    );
}

#[tokio::test]
async fn a_run_is_on_record_before_it_reads_as_finished() {
    home("outcome");
    let dir = repo();
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            let at = a.spec.cwd.join("new.txt");
            a.own_tool(
                "1",
                "Write",
                json!({"file_path": "new.txt"}),
                || async move {
                    std::fs::write(at, "made").unwrap();
                    (true, "ok".to_string())
                },
            )
            .await;
            a.say("made a file");
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(&[fake]);
    let (meta, mut rx) = orch
        .spawn(
            "outcome-tests",
            "claude",
            "make a file",
            None,
            &dir.path().display().to_string(),
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    let mut m = store.get(&meta.id).unwrap();
    for _ in 0..400 {
        m = store.get(&meta.id).unwrap();
        if m.status != parzi_core::store::SessionStatus::Active {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(m.branch, "plan/names", "git's look is in");
    let out = m.last_run.expect("on record when it settled");
    assert_eq!(out.summary, "made a file");
    assert!(
        out.files.iter().any(|f| f.path == "new.txt"),
        "{:?}",
        out.files
    );
    while let Some(ev) = rx.recv().await {
        if matches!(ev, parzi_runtime::handler::RunEvent::Done { .. }) {
            let now = store.get(&meta.id).unwrap();
            assert_eq!(now.status, parzi_core::store::SessionStatus::Done);
            assert!(now.last_run.is_some());
            break;
        }
    }
}

#[tokio::test]
async fn a_relative_folder_is_refused_before_the_agent_starts() {
    home("outcome");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.say("ran");
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let err = orch
        .spawn(
            "outcome-tests",
            "claude",
            "make a file",
            None,
            ".",
            "low",
            vec![],
            None,
        )
        .await
        .expect_err("`.` is Parzi's own folder, not the person's")
        .to_string();
    assert!(err.contains("not a full path"), "{err}");
    assert!(fake.seen().is_empty(), "the agent never started");
    let _ = store;
}

#[tokio::test]
async fn a_runaway_tool_loop_is_stopped_instead_of_spinning_forever() {
    home("outcome");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            for i in 0..120u32 {
                if a.cancel.is_cancelled() {
                    return Ok(TurnEnd::Interrupted);
                }
                a.own_tool(
                    &format!("t{i}"),
                    "Edit",
                    serde_json::json!({"file_path": "x.ts"}),
                    || async { (true, "ok".to_string()) },
                )
                .await;
            }
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, _rx) = orch
        .spawn(
            "outcome-tests",
            "claude",
            "keep editing",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    let m = recorded(&store, &meta.id).await;
    let out = m.last_run.unwrap();
    assert_eq!(out.end, RunEndKind::Failed, "the looping turn fails");
    assert!(
        out.summary.contains("kept calling the same tool"),
        "the thread says why it stopped: {out:?}"
    );
    assert_eq!(
        store.get(&meta.id).unwrap().status,
        SessionStatus::Idle,
        "the session is usable again, not stuck active"
    );
}

#[tokio::test]
async fn a_long_turn_of_different_tool_calls_is_left_to_finish() {
    home("outcome");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            for i in 0..150u32 {
                a.own_tool(
                    &format!("t{i}"),
                    "Edit",
                    serde_json::json!({"file_path": format!("f{i}.ts")}),
                    || async { (true, "ok".to_string()) },
                )
                .await;
            }
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, _rx) = orch
        .spawn(
            "outcome-tests",
            "claude",
            "edit them all",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    let m = recorded(&store, &meta.id).await;
    assert_eq!(m.last_run.unwrap().end, RunEndKind::Completed);
}

#[tokio::test]
async fn a_halted_turn_keeps_the_reply_it_had_so_far() {
    home("outcome");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.say("half an answer");
            for i in 0..40u32 {
                a.own_tool(
                    &format!("t{i}"),
                    "Edit",
                    serde_json::json!({"file_path": "x.ts"}),
                    || async { (true, "ok".to_string()) },
                )
                .await;
            }
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, _rx) = orch
        .spawn(
            "outcome-tests",
            "claude",
            "keep editing",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    recorded(&store, &meta.id).await;
    let kept = store.events(&meta.id).unwrap().iter().any(|e| {
        matches!(e, parzi_core::store::Event::Assistant { text, .. } if text.contains("half an answer"))
    });
    assert!(kept, "the text before the halt stays in the thread");
}

#[tokio::test]
async fn a_session_marked_active_with_no_run_takes_the_next_message() {
    home("outcome");
    let fake = Fake::new(
        "claude",
        script(|_a: Agent| async move { Ok(TurnEnd::Completed) }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, _rx) = orch
        .spawn(
            "outcome-tests",
            "claude",
            "first",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    recorded(&store, &meta.id).await;
    store.set_status(&meta.id, SessionStatus::Active).unwrap();
    orch.send_to(&meta.id, "second", None, "", "low", vec![], None, None)
        .await
        .expect("a lost status write does not lock the session");
}
