mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use common::*;
use parzi_core::config::ParziConfig;
use parzi_core::store::{SessionStore, StepState};
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

fn host(project: &str, asker: Option<Arc<dyn Asker>>) -> (ToolHost, SessionStore, String) {
    home("plan-review");
    let store = SessionStore::open().unwrap();
    let sid = store.create("planning", project, "claude/m").unwrap().id;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let host = ToolHost::new(ToolHostParts {
        session_id: sid.clone(),
        mode: ApprovalMode::Auto,
        edits_auto: false,
        full: true,
        cfg: ParziConfig::default(),
        store: store.clone(),
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
    });
    (host, store, sid)
}

fn steps() -> serde_json::Value {
    json!({
        "propose": true,
        "items": [
            {"title": "Find the stall", "state": "pending"},
            {"title": "Cache the head", "state": "pending"},
        ],
    })
}

/// A proposed plan waits on the session for the person's answer, and the
/// agent's turn ends: nothing is asked inside the tool call, and nothing is
/// saved as the plan until the person says yes.
#[tokio::test]
async fn a_proposal_waits_on_the_session_and_the_turn_ends() {
    let person = Person::says(&[]);
    let (h, store, sid) = host("review-yes", Some(person.clone()));

    let (ok, out) = h.call("plan.write", &steps()).await;
    assert!(ok, "{out}");
    assert!(out.contains("End your turn"), "{out}");
    assert!(
        person.asked().is_empty(),
        "the call does not wait on a question"
    );
    let meta = store.get(&sid).unwrap();
    assert!(
        meta.plan.is_none(),
        "not the plan until the person says yes"
    );
    let proposal = meta.proposal.expect("kept on the session");
    assert_eq!(proposal.items[1].title, "Cache the head");
}

/// Progress updates are not proposals: they save at once, without asking.
#[tokio::test]
async fn a_plain_write_saves_without_asking_and_reads_back() {
    let person = Person::says(&[]);
    let (h, store, sid) = host("review-plain", Some(person.clone()));
    let (ok, out) = h
        .call(
            "plan.write",
            &json!({"items": [{"title": "Find the stall", "state": "done"}, {"title": "Cache the head", "state": "active"}]}),
        )
        .await;
    assert!(ok, "{out}");
    assert!(person.asked().is_empty(), "no question for a plain write");
    let plan = store.get(&sid).unwrap().plan.unwrap();
    assert_eq!(plan.items[1].state, StepState::Active);
    let (ok, read) = h.call("plan.read", &json!({})).await;
    assert!(ok, "{read}");
    assert!(
        read.contains("1 of 2 done") && read.contains("[>] Cache the head"),
        "{read}"
    );
}

/// With nobody to ask (a background helper), a proposal saves as written.
#[tokio::test]
async fn with_nobody_to_ask_a_proposal_saves() {
    let (h, store, sid) = host("review-none", None);
    let (ok, out) = h.call("plan.write", &steps()).await;
    assert!(ok, "{out}");
    assert!(store.get(&sid).unwrap().plan.is_some());
}
