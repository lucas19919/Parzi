mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use common::*;
use parzi_core::config::ParziConfig;
use parzi_core::store::{SessionStatus, SessionStore};
use parzi_core::tasks::{NewTask, Task, TaskStatus, TaskStore};
use parzi_providers::{ProviderError, TurnEnd};
use parzi_runtime::handler::{RunSink, SpawnSpec};
use parzi_runtime::mcp::McpManager;
use parzi_runtime::toolhost::{ToolHost, ToolHostParts};
use parzi_runtime::tools::{Approval, ApprovalMode, Approver, ToolCallInfo, ToolExecutor};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

struct Allow;
#[async_trait::async_trait]
impl Approver for Allow {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

fn host_for(store: &SessionStore, sid: &str) -> ToolHost {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: sid.to_string(),
        lane: "build".into(),
        mode: ApprovalMode::Auto,
        edits_auto: false,
        full: false,
        cfg: ParziConfig::default(),
        store: store.clone(),
        tools: Arc::new(ToolExecutor {
            cwd: std::env::temp_dir().display().to_string(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["*".into()],
        }),
        approver: Arc::new(Allow),
        asker: None,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test")),
        harness: None,
        sink: RunSink::new(sid, tx, None),
        cancel: CancellationToken::new(),
    })
}

/// A session in `project` and a tool host for it.
fn session_in(project: &str) -> (ToolHost, SessionStore, String) {
    home("tasks");
    let store = SessionStore::open().unwrap();
    let sid = store
        .create("planner", project, "build", "claude/m")
        .unwrap()
        .id;
    (host_for(&store, &sid), store, sid)
}

fn ids_in(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| l.strip_prefix("- "))
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn agents_plan_with_the_shared_list() {
    let (host, _store, sid) = session_in("tasks-plan");
    let (ok, out) = host
        .call(
            "task.add",
            &json!({"tasks": [
                {"title": "Write the parser", "key": "parser", "lane": "build"},
                {"title": "Wire the UI", "needs": ["parser"], "notes": "Use the new view."},
            ]}),
        )
        .await;
    assert!(ok, "{out}");
    assert!(out.contains("added 2 tasks to project tasks-plan"), "{out}");
    let ids = ids_in(&out);
    let (parser, ui) = (ids[0].clone(), ids[1].clone());

    let (ok, list) = host.call("task.list", &json!({})).await;
    assert!(ok, "{list}");
    assert!(list.contains("1 ready, 1 blocked"), "{list}");
    assert!(
        list.contains(&format!("- {parser} Write the parser (build)")),
        "{list}"
    );
    assert!(list.contains(&format!("waits on {parser}")), "{list}");
    assert!(list.contains("Use the new view."), "notes show: {list}");

    let (ok, out) = host
        .call("task.update", &json!({"id": ui, "status": "done"}))
        .await;
    assert!(
        !ok && out.contains("waits on"),
        "done waits for needs: {out}"
    );
    let (ok, out) = host
        .call(
            "task.update",
            &json!({"id": parser, "status": "done", "note": "Parser merged."}),
        )
        .await;
    assert!(ok && out.contains("is done now"), "{out}");
    assert!(
        out.contains(&format!("ready: {ui}")),
        "the UI task is ready: {out}"
    );
    let (ok, out) = host
        .call("task.update", &json!({"id": ui, "status": "someday"}))
        .await;
    assert!(!ok && out.contains("not a status"), "{out}");

    let task = TaskStore::open().unwrap().get(&parser).unwrap();
    assert_eq!(task.notes, "Parser merged.");
    assert_eq!(task.created_by, sid);
}

#[tokio::test]
async fn agents_change_their_own_project_and_their_crews_tasks() {
    let (host, store, sid) = session_in("tasks-alpha");
    let ts = TaskStore::open().unwrap();
    let draft = |t: &str| NewTask {
        title: t.into(),
        ..NewTask::default()
    };
    let theirs = ts
        .add(Some("tasks-beta"), &[draft("not mine")], "you")
        .unwrap()
        .remove(0);
    let (ok, out) = host
        .call("task.update", &json!({"id": theirs.id, "status": "done"}))
        .await;
    assert!(
        !ok && out.contains("belongs to project tasks-beta"),
        "{out}"
    );

    // A child of this session works it: now it may.
    let child = store
        .create_with_parent("crew", "tasks-beta", "build", "claude/m", Some(&sid))
        .unwrap();
    let linked = parzi_runtime::task_tools::link(&theirs.id, &child.id).unwrap();
    assert_eq!(linked.status, TaskStatus::Doing);
    let (ok, out) = host
        .call("task.update", &json!({"id": theirs.id, "status": "review"}))
        .await;
    assert!(ok, "{out}");

    // Reading another project is fine.
    let (ok, out) = host
        .call("task.list", &json!({"project": "tasks-beta"}))
        .await;
    assert!(ok && out.contains("not mine"), "{out}");
}

#[test]
fn task_tools_replace_the_plan_tools() {
    let tools = ToolExecutor {
        cwd: String::new(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: vec!["*".into()],
    };
    let names: Vec<String> = tools.defs().into_iter().map(|d| d.name).collect();
    for t in ["task.list", "task.add", "task.update"] {
        assert!(names.iter().any(|n| n == t), "{t} is offered");
    }
    assert!(names.iter().all(|n| !n.starts_with("plan.")));
    let spawn = tools
        .defs()
        .into_iter()
        .find(|d| d.name == "session.spawn")
        .unwrap();
    assert!(spawn.schema["properties"]["task"].is_object());
}

fn staffed() -> Arc<Fake> {
    Fake::new(
        "claude",
        script(|a: Agent| async move {
            if a.spec.prompt.contains("You are working task") {
                a.say("built it");
                return Ok(TurnEnd::Completed);
            }
            let id = a
                .spec
                .prompt
                .split_whitespace()
                .last()
                .unwrap_or("")
                .to_string();
            let (ok, out) = a
                .parzi(
                    "session.spawn",
                    json!({"prompt": "build the thing", "task": id, "wait": true}),
                )
                .await;
            a.say(&format!("parent heard: ok={ok} {out}"));
            Ok(TurnEnd::Completed)
        }),
    )
}

fn crew(fake: Arc<Fake>) -> (Arc<parzi_runtime::Orchestrator>, SessionStore) {
    home("tasks");
    let mut cfg = ParziConfig::default();
    cfg.orchestrator.max_concurrent = 4;
    cfg.lanes.default_mode = "auto".into();
    orch_with(cfg, &[fake])
}

fn finished_log() -> Arc<Mutex<Vec<Task>>> {
    let log = Arc::new(Mutex::new(vec![]));
    let sink = log.clone();
    parzi_core::tasks::watch(move |c| sink.lock().unwrap().extend(c.finished.iter().cloned()));
    log
}

#[tokio::test]
async fn a_staffed_child_takes_its_task_to_review() {
    let fake = staffed();
    let (orch, store) = crew(fake.clone());
    let log = finished_log();
    let ts = TaskStore::open().unwrap();
    let task = ts
        .add(
            Some("tasks-gamma"),
            &[NewTask {
                title: "Build the thing".into(),
                notes: "Keep it small.".into(),
                ..NewTask::default()
            }],
            "you",
        )
        .unwrap()
        .remove(0);
    let (meta, _rx) = orch
        .spawn(
            "tasks-gamma",
            "build",
            "claude",
            &format!("staff task {}", task.id),
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let reply = last_reply(&store, &meta.id);
    assert!(
        reply.contains("ok=true") && reply.contains("built it"),
        "{reply}"
    );

    let child = store.list_children(&meta.id).unwrap().remove(0);
    let after = ts.get(&task.id).unwrap();
    assert_eq!(after.session.as_deref(), Some(child.id.as_str()));
    assert_eq!(
        after.status,
        TaskStatus::Review,
        "a finished run hands it back"
    );
    let brief = fake
        .seen()
        .into_iter()
        .map(|s| s.prompt)
        .find(|p| p.contains("You are working task"))
        .unwrap();
    assert!(
        brief.contains(&task.id) && brief.contains("Keep it small."),
        "{brief}"
    );
    for _ in 0..40 {
        if log.lock().unwrap().iter().any(|t| t.id == task.id) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|t| t.id == task.id && t.status == TaskStatus::Review),
        "the user hears it finished"
    );
}

#[tokio::test]
async fn a_child_that_marks_its_task_done_keeps_it_done() {
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            let id = a
                .spec
                .prompt
                .split("You are working task ")
                .nth(1)
                .and_then(|r| r.split(':').next())
                .unwrap_or("")
                .to_string();
            let (ok, out) = a
                .parzi("task.update", json!({"id": id, "status": "done"}))
                .await;
            a.say(&format!("marked: ok={ok} {out}"));
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = crew(fake);
    let log = finished_log();
    let ts = TaskStore::open().unwrap();
    let task = ts
        .add(
            Some("t"),
            &[NewTask {
                title: "Small fix".into(),
                ..NewTask::default()
            }],
            "you",
        )
        .unwrap()
        .remove(0);
    let parent = store.create("boss", "t", "build", "claude/model").unwrap();
    let out = orch
        .harness()
        .spawn(
            &parent.id,
            SpawnSpec {
                prompt: "fix it".into(),
                is_subsession: true,
                wait: true,
                mode_override: Some("full".into()),
                task: Some(task.id.clone()),
                ..SpawnSpec::default()
            },
        )
        .await
        .unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert!(v["response"].as_str().unwrap().contains("ok=true"), "{out}");
    assert_eq!(
        v["title"], "Small fix",
        "an untitled spawn takes the task's title"
    );
    let mut status = TaskStatus::Doing;
    for _ in 0..40 {
        status = ts.get(&task.id).unwrap().status;
        if log.lock().unwrap().iter().any(|t| t.id == task.id) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        status,
        TaskStatus::Done,
        "the run end leaves a done task done"
    );
    assert!(
        log.lock()
            .unwrap()
            .iter()
            .any(|t| t.id == task.id && t.status == TaskStatus::Done),
        "and reports it"
    );
}

#[tokio::test]
async fn failed_and_stopped_runs_hand_the_task_back() {
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            if a.spec.prompt.contains("break") {
                return Err(ProviderError::process("the agent fell over"));
            }
            a.cancel.cancelled().await;
            Ok(TurnEnd::Interrupted)
        }),
    );
    let (orch, store) = crew(fake);
    let ts = TaskStore::open().unwrap();
    let draft = |t: &str| NewTask {
        title: t.into(),
        ..NewTask::default()
    };
    let added = ts
        .add(
            Some("tasks-delta"),
            &[draft("will fail"), draft("will stop")],
            "you",
        )
        .unwrap();
    let parent = store
        .create("boss", "tasks-delta", "build", "claude/model")
        .unwrap();
    let h = orch.harness();
    let spec = |prompt: &str, task: &Task| SpawnSpec {
        prompt: prompt.into(),
        is_subsession: true,
        wait: false,
        mode_override: Some("full".into()),
        task: Some(task.id.clone()),
        ..SpawnSpec::default()
    };
    let failing = serde_json::from_str::<Value>(
        &h.spawn(&parent.id, spec("break", &added[0])).await.unwrap(),
    )
    .unwrap()["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let stopping =
        serde_json::from_str::<Value>(&h.spawn(&parent.id, spec("hang", &added[1])).await.unwrap())
            .unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
    assert!(wait_status(&store, &stopping, SessionStatus::Active).await);
    assert_eq!(ts.get(&added[1].id).unwrap().status, TaskStatus::Doing);
    orch.kill(&stopping).await.unwrap();
    settle(&store, &failing).await;
    let mut got = (TaskStatus::Doing, TaskStatus::Doing);
    for _ in 0..60 {
        got = (
            ts.get(&added[0].id).unwrap().status,
            ts.get(&added[1].id).unwrap().status,
        );
        if got == (TaskStatus::Failed, TaskStatus::Todo) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(got, (TaskStatus::Failed, TaskStatus::Todo));
}

#[tokio::test]
async fn blocked_and_foreign_tasks_are_not_staffed() {
    let fake = staffed();
    let (orch, store) = crew(fake);
    let ts = TaskStore::open().unwrap();
    let first = ts
        .add(
            Some("tasks-eps"),
            &[NewTask {
                title: "first".into(),
                key: Some("a".into()),
                ..NewTask::default()
            }],
            "you",
        )
        .unwrap()
        .remove(0);
    let second = ts
        .add(
            Some("tasks-eps"),
            &[NewTask {
                title: "second".into(),
                needs: vec![first.id.clone()],
                ..NewTask::default()
            }],
            "you",
        )
        .unwrap()
        .remove(0);
    let (meta, _rx) = orch
        .spawn(
            "tasks-eps",
            "build",
            "claude",
            &format!("staff task {}", second.id),
            Some(Arc::new(Allow)),
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let reply = last_reply(&store, &meta.id);
    assert!(
        reply.contains("ok=false") && reply.contains("waits on"),
        "{reply}"
    );
    assert!(
        store.list_children(&meta.id).unwrap().is_empty(),
        "no child started"
    );
    assert_eq!(ts.get(&second.id).unwrap().status, TaskStatus::Todo);
}
