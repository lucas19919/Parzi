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
    let sid = store.create("brain", "", "claude/m").unwrap().id;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: sid.clone(),
        mode: ApprovalMode::Ask,
        edits_auto: false,
        full: false,
        cfg: parzi_core::config::ParziConfig::default(),
        store,
        tools: Arc::new(ToolExecutor {
            cwd: String::new(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["brain.*".into()],
        }),
        approver,
        asker: None,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test")),
        harness: None,
        sink: RunSink::new(&sid, tx, None),
        cancel: CancellationToken::new(),
    })
}

#[test]
fn brain_tools_round_trip_their_mcp_names() {
    let names: Vec<String> = brain_defs().into_iter().map(|d| d.name).collect();
    // Every brain tool the host runs is offered to agents (tools/list).
    assert_eq!(
        names,
        ["brain.search", "brain.read", "brain.write", "brain.delete"]
    );
    for name in &names {
        assert!(is_brain_tool(name));
        assert_eq!(
            &display_name(&format!("mcp__parzi__{}", to_mcp(name))),
            name
        );
    }
    let offered = ToolExecutor {
        cwd: String::new(),
        mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
        allowed: vec!["brain.*".into()],
    }
    .defs();
    assert!(offered.iter().any(|d| d.name == "brain.delete"));
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
    let parts = system_parts(src);
    assert!(parts[1].contains("one line summaries"));
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
    let last = system_parts(src).pop().unwrap();
    assert!(
        last.ends_with("\n\n## setup (notes/setup.md)\n\nRun cargo xtask."),
        "{last}"
    );
    assert!(!last.contains("## On demand"), "{last}");

    let bare = system_parts(home.to_str().unwrap());
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
        let last = system_parts(cwd).pop().unwrap();
        assert!(
            last.ends_with("\n\n## me (notes/me.md)\n\nKeep answers short."),
            "{last}"
        );
        assert!(!last.contains("## Project:"), "{last}");
    }
    let last = system_parts(src).pop().unwrap();
    let (me, setup) = (
        last.find("## me (notes/me.md)").unwrap(),
        last.find("## setup (notes/setup.md)").unwrap(),
    );
    assert!(setup < me, "the project's pinned notes go first: {last}");

    // Search wants every word, in any order.
    let (ok, out) = h.call("brain.search", &json!({"query": "xtask run"})).await;
    assert!(ok && out.starts_with("- notes/setup.md — setup\n"), "{out}");

    // A sync conflict copy is listed as one, but never searched or attached.
    brain::write(
        "notes/setup.conflict-laptop-macos.md",
        "---\nprojects: [demo]\npinned: true\n---\nRun make.",
    )
    .unwrap();
    let (ok, out) = h.call("brain.list", &json!({"project": "demo"})).await;
    assert!(
        ok && out.contains("\n- notes/setup.conflict-laptop-macos.md — setup.conflict-laptop-macos (sync conflict copy of notes/setup.md; the user settles it in the Brain view)"),
        "{out}"
    );
    let (ok, out) = h.call("brain.search", &json!({"query": "run"})).await;
    assert!(ok && !out.contains("conflict"), "{out}");
    let last = system_parts(src).pop().unwrap();
    assert!(
        !last.contains("conflict") && !last.contains("make"),
        "{last}"
    );
    assert_eq!(
        brain::resolve_conflict("notes/setup.conflict-laptop-macos.md", false).unwrap(),
        "notes/setup.md"
    );

    // brain.delete is gated like brain.write.
    let (ok, _) = h.call("brain.delete", &json!({"path": "notes/new"})).await;
    assert!(!ok);
    assert_eq!(
        *nobody.seen.lock().unwrap(),
        ["brain.write", "brain.delete"]
    );
    assert!(brain::read("notes/new.md").is_ok());
    let (ok, out) = host(person(Approval::Allow))
        .call("brain.delete", &json!({"path": "notes/new"}))
        .await;
    assert!(ok && out == "deleted notes/new.md", "{out}");
    assert!(brain::read("notes/new.md").is_err());

    // Every listing stops at 50 notes.
    for i in 0..60 {
        brain::write(
            &format!("bulk/n{i:02}.md"),
            "---\nprojects: [demo, all]\n---\nbulk",
        )
        .unwrap();
    }
    for args in [
        json!({"project": "demo"}),
        json!({"project": "all"}),
        json!({}),
    ] {
        let (ok, out) = h.call("brain.list", &args).await;
        assert!(ok, "{out}");
        assert_eq!(out.matches("\n- bulk/").count(), 50, "{args}: {out}");
        assert!(out.ends_with(" more; use brain.search"), "{args}: {out}");
    }
    let (_, out) = h.call("brain.list", &json!({"project": "demo"})).await;
    assert!(out.ends_with("\n- …and 11 more; use brain.search"), "{out}");
}
