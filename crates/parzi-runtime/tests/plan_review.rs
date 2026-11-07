mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use common::*;
use parzi_core::brain::Vault;
use parzi_core::config::ParziConfig;
use parzi_core::plan::{self, PlanState};
use parzi_core::store::SessionStore;
use parzi_runtime::handler::RunSink;
use parzi_runtime::mcp::McpManager;
use parzi_runtime::toolhost::{ToolHost, ToolHostParts};
use parzi_runtime::tools::{ApprovalMode, AskRequest, Asker, AutoApprover, ToolExecutor};
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// Answers each question with the next scripted answer and keeps what it
/// was asked.
struct Person {
    answers: Mutex<Vec<String>>,
    asked: Mutex<Vec<AskRequest>>,
}

impl Person {
    fn says(answers: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.iter().rev().map(|a| (*a).to_string()).collect()),
            asked: Mutex::default(),
        })
    }

    fn asked(&self) -> Vec<AskRequest> {
        self.asked.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Asker for Person {
    async fn ask(&self, req: &AskRequest) -> String {
        self.asked.lock().unwrap().push(req.clone());
        self.answers.lock().unwrap().pop().unwrap_or_default()
    }
}

fn host(project: &str, asker: Option<Arc<dyn Asker>>) -> ToolHost {
    home("plan-review");
    let store = SessionStore::open().unwrap();
    let sid = store.create("planning", project, "claude/m").unwrap().id;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: sid.clone(),
        mode: ApprovalMode::Auto,
        edits_auto: false,
        full: true,
        cfg: ParziConfig::default(),
        store,
        tools: Arc::new(ToolExecutor {
            cwd: std::env::temp_dir().display().to_string(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["*".into()],
        }),
        approver: Arc::new(AutoApprover),
        asker,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test")),
        harness: None,
        sink: RunSink::new(&sid, tx, None),
        cancel: CancellationToken::new(),
    })
}

fn saved(project: &str) -> Option<plan::Plan> {
    plan::load(&Vault::open().unwrap(), project).plan
}

fn release() -> serde_json::Value {
    json!({
        "title": "Release",
        "phases": ["Now", "Next"],
        "items": [
            {"id": "names", "title": "Session names", "phase": "Now"},
            {"id": "panel", "title": "Plan panel", "phase": "Next"},
        ],
    })
}

#[tokio::test]
async fn a_new_plan_waits_for_the_persons_yes() {
    let person = Person::says(&["Not now", "Ship names alone first", "Use this plan"]);
    let h = host("review-yes", Some(person.clone()));

    let (ok, out) = h.call("plan.write", &release()).await;
    assert!(ok, "a no is an answer, not a failure: {out}");
    assert!(
        out.contains("not now") && out.contains("nothing was saved"),
        "{out}"
    );
    assert!(saved("review-yes").is_none());

    let (ok, out) = h.call("plan.write", &release()).await;
    assert!(ok, "{out}");
    assert!(
        out.contains("\"Ship names alone first\"") && out.contains("call plan.write again"),
        "{out}"
    );
    assert!(saved("review-yes").is_none());

    let (ok, out) = h.call("plan.write", &release()).await;
    assert!(ok, "{out}");
    assert!(
        out.starts_with("The person approved this plan. Saved"),
        "{out}"
    );
    assert_eq!(saved("review-yes").unwrap().items.len(), 2);

    let asked = person.asked();
    assert_eq!(asked.len(), 3);
    assert_eq!(
        asked[0].question,
        "Start the plan \"Release\" for review-yes?\nNow\n+ Session names\nNext\n+ Plan panel"
    );
    assert_eq!(asked[0].options, vec!["Use this plan", "Not now"]);
}

#[tokio::test]
async fn progress_is_saved_without_asking_and_new_work_is_asked_for() {
    let person = Person::says(&["Use this plan", "Use this plan"]);
    let h = host("review-progress", Some(person.clone()));
    h.call("plan.write", &release()).await;
    assert_eq!(person.asked().len(), 1);

    let (ok, out) = h
        .call(
            "plan.write",
            &json!({"items": [{"id": "names", "state": "done", "note": "shipped in 1.3"}]}),
        )
        .await;
    assert!(ok && out.starts_with("Saved"), "{out}");
    assert_eq!(person.asked().len(), 1, "progress needs no yes");
    assert_eq!(
        saved("review-progress")
            .unwrap()
            .item("names")
            .unwrap()
            .state,
        PlanState::Done
    );

    let (ok, out) = h
        .call(
            "plan.write",
            &json!({"project": "review-progress", "items": [{"id": "history", "title": "Plan history", "phase": "Next"}]}),
        )
        .await;
    assert!(ok, "{out}");
    let asked = person.asked();
    assert_eq!(asked.len(), 2);
    assert_eq!(
        asked[1].question,
        "Change the plan \"Release\"?\n+ Plan history (Next)"
    );
}

#[tokio::test]
async fn a_bad_write_fails_before_anyone_is_asked_and_another_project_is_refused() {
    let person = Person::says(&[]);
    let h = host("review-bad", Some(person.clone()));
    let (ok, out) = h
        .call(
            "plan.write",
            &json!({"title": "X", "items": [{"id": "a", "title": "A", "needs": ["ghost"]}]}),
        )
        .await;
    assert!(!ok, "{out}");
    let (ok, out) = h
        .call("plan.write", &json!({"project": "elsewhere", "title": "X"}))
        .await;
    assert!(!ok && out.contains("project.switch"), "{out}");
    assert!(person.asked().is_empty());
}

#[tokio::test]
async fn with_no_one_to_ask_the_plan_is_saved() {
    let h = host("review-alone", None);
    let (ok, out) = h.call("plan.write", &release()).await;
    assert!(ok && out.starts_with("Saved"), "{out}");
    assert!(saved("review-alone").is_some());
}
