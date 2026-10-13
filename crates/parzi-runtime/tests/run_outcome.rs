//! A run's outcome lives on its session: how it ended, its last reply, the
//! files it changed (named by its tools, or seen by git), the checks it
//! ran, the branch it ended on, and the effort it used. Sessions get a
//! readable first name.

mod common;

use std::path::Path;
use std::sync::Arc;

use common::*;
use parzi_core::store::{Check, FileTouch, RunEndKind, SessionMeta, SessionStore, Via};
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

/// A repo on branch `plan/names` with one commit.
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

/// The session once its run's outcome is kept.
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
            // A file the agent's own tools never named: git sees it. Git
            // looks at the repo in the background as the run starts; real
            // edits come later than that, and so does this one.
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
            "build",
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
            "build",
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
            a.say(if a.spec.prompt.starts_with("/compact") {
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
            "build",
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
