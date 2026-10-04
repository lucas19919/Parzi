mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use parzi_core::brain;
use parzi_core::store::SessionStore;
use parzi_runtime::handler::{system_parts, RunSink};
use parzi_runtime::mcp::McpManager;
use parzi_runtime::toolhost::{ToolHost, ToolHostParts};
use parzi_runtime::tools::{
    brain_defs, display_name, is_brain_tool, to_mcp, Approval, ApprovalMode, Approver,
    ToolCallInfo, ToolExecutor,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

struct Person {
    answer: Approval,
    seen: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl Approver for Person {
    async fn approve(&self, call: &ToolCallInfo) -> Approval {
        self.seen.lock().unwrap().push(call.name.clone());
        self.answer
    }
}

fn person(answer: Approval) -> Arc<Person> {
    Arc::new(Person {
        answer,
        seen: Mutex::new(vec![]),
    })
}

fn host(approver: Arc<Person>) -> ToolHost {
    let store = SessionStore::open().unwrap();
    let sid = store.create("brain", "", "", "claude/m").unwrap().id;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: sid.clone(),
        lane: String::new(),
        mode: ApprovalMode::Ask,
        edits_auto: false,
        store,
        tools: Arc::new(ToolExecutor {
            cwd: String::new(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["brain.*".into()],
        }),
        approver,
        harness: None,
        sink: RunSink::new(&sid, tx, None),
        cancel: CancellationToken::new(),
    })
}

#[test]
fn brain_tools_round_trip_their_mcp_names() {
    for d in brain_defs() {
        assert!(is_brain_tool(&d.name));
        assert_eq!(
            display_name(&format!("mcp__parzi__{}", to_mcp(&d.name))),
            d.name
        );
    }
}

#[tokio::test]
async fn brain_reads_freely_writes_with_approval_and_attaches_project_notes() {
    let home = common::home("brain");
    let code = home.join("code");
    std::fs::create_dir_all(code.join("src")).unwrap();
    brain::project_upsert(Some("demo"), "Demo", code.to_str().unwrap()).unwrap();
    brain::write(
        "notes/setup.md",
        "---\nprojects: [demo]\n---\nRun cargo xtask.",
    )
    .unwrap();

    let nobody = person(Approval::Deny);
    let h = host(nobody.clone());
    let (ok, out) = h.call("brain.search", &json!({"query": "XTASK"})).await;
    assert!(ok && out.contains("notes/setup.md"), "{out}");
    let (ok, out) = h.call("brain.read", &json!({"path": "notes/setup"})).await;
    assert!(ok && out.ends_with("Run cargo xtask."), "{out}");
    let (ok, out) = h.call("brain.list", &json!({"project": "demo"})).await;
    assert!(
        ok && out.contains("\n- notes/setup.md — setup: Run cargo xtask."),
        "{out}"
    );
    let (ok, out) = h.call("brain.list", &json!({})).await;
    assert!(ok && out.contains("- demo (Demo)"), "{out}");
    assert!(
        out.contains("\n- notes/setup.md — setup [demo]: Run cargo xtask."),
        "{out}"
    );
    let (ok, _) = h
        .call(
            "brain.write",
            &json!({"path": "notes/new.md", "content": "x"}),
        )
        .await;
    assert!(!ok);
    assert_eq!(*nobody.seen.lock().unwrap(), ["brain.write"]);
    assert!(brain::read("notes/new.md").is_err());

    let (ok, out) = host(person(Approval::Allow))
        .call(
            "brain.write",
            &json!({"path": "notes/new", "content": "learned"}),
        )
        .await;
    assert!(ok, "{out}");
    assert_eq!(brain::read("notes/new.md").unwrap(), "learned");

    let src = code.join("src");
    let src = src.to_str().unwrap();
    let parts = system_parts("", src);
    assert!(parts[0].contains("on-demand notes are listed with a one-line summary"));
    let last = parts.last().unwrap();
    assert!(
        last.starts_with("# The user's notes (Parzi brain)\n"),
        "{last}"
    );
    assert!(last.contains("\n\n## Project: Demo\nFolder: "), "{last}");
    assert!(
        last.ends_with("\n\n## On demand\n- notes/setup.md — setup: Run cargo xtask."),
        "{last}"
    );
    assert!(brain::pin("notes/setup.md", true).unwrap().pinned);
    let last = system_parts("", src).pop().unwrap();
    assert!(
        last.ends_with("\n\n## setup (notes/setup.md)\n\nRun cargo xtask."),
        "{last}"
    );
    assert!(!last.contains("## On demand"), "{last}");

    let bare = system_parts("w", home.to_str().unwrap());
    assert!(!bare.iter().any(|p| p.contains("Parzi brain")));
    brain::write(
        "notes/me.md",
        "---\nprojects: [all]\npinned: true\n---\nKeep answers short.",
    )
    .unwrap();
    let (ok, out) = h.call("brain.list", &json!({"project": "ALL"})).await;
    assert!(
        ok && out == "Notes for every session:\n- notes/me.md — me (pinned): Keep answers short.",
        "{out}"
    );
    for cwd in ["", "  ", home.to_str().unwrap()] {
        let last = system_parts("w", cwd).pop().unwrap();
        assert!(
            last.ends_with("\n\n## me (notes/me.md)\n\nKeep answers short."),
            "{last}"
        );
        assert!(!last.contains("## Project:"), "{last}");
    }
    let last = system_parts("", src).pop().unwrap();
    let (me, setup) = (
        last.find("## me (notes/me.md)").unwrap(),
        last.find("## setup (notes/setup.md)").unwrap(),
    );
    assert!(me < setup, "{last}");
}
