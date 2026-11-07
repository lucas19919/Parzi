mod common;

use std::collections::HashMap;
use std::sync::Arc;

use common::*;
use parzi_core::config::ParziConfig;
use parzi_core::store::SessionStore;
use parzi_providers::{PermissionDecision, PermissionGate, PermissionRequest};
use parzi_runtime::handler::RunSink;
use parzi_runtime::mcp::McpManager;
use parzi_runtime::toolhost::{ToolHost, ToolHostParts};
use parzi_runtime::tools::{Approval, ApprovalMode, Approver, ToolCallInfo, ToolExecutor};
use serde_json::json;
use tokio_util::sync::CancellationToken;

struct Allow;
#[async_trait::async_trait]
impl Approver for Allow {
    async fn approve(&self, _call: &ToolCallInfo) -> Approval {
        Approval::Allow
    }
}

fn host(cwd: &str) -> ToolHost {
    host_with(cwd, false)
}

fn host_with(cwd: &str, full: bool) -> ToolHost {
    home("shell-tools");
    let store = SessionStore::open().unwrap();
    let sid = store.create("shell", "", "claude/m").unwrap().id;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    ToolHost::new(ToolHostParts {
        session_id: sid.clone(),
        mode: ApprovalMode::Auto,
        edits_auto: false,
        full,
        cfg: ParziConfig::default(),
        store,
        tools: Arc::new(ToolExecutor {
            cwd: cwd.to_string(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["*".into()],
        }),
        approver: Arc::new(Allow),
        asker: None,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test-shell")),
        harness: None,
        sink: RunSink::new(&sid, tx, None),
        cancel: CancellationToken::new(),
    })
}

fn workdir() -> String {
    let dir = std::env::temp_dir().join(format!("parzi-shell-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.display().to_string()
}

#[tokio::test]
async fn foreground_echo_comes_back() {
    let h = host(&workdir());
    let (ok, out) = h
        .call("shell.exec", &json!({"cmd": "echo shell-owned"}))
        .await;
    assert!(ok, "{out}");
    assert!(out.contains("shell-owned"), "{out}");
}

#[tokio::test]
async fn foreground_rapid_exits_keep_every_byte() {
    let h = host(&workdir());
    for i in 0..50u32 {
        let marker = format!("race-marker-{i}");
        let (ok, out) = h
            .call("shell.exec", &json!({"cmd": format!("echo {marker}")}))
            .await;
        assert!(ok, "{out}");
        assert!(
            out.contains(&marker),
            "lost output on iteration {i}: {out:?}"
        );
    }
}

#[tokio::test]
async fn foreground_failure_reports_its_exit() {
    let h = host(&workdir());
    let (ok, out) = h
        .call(
            "shell.exec",
            &json!({"cmd": "exit 3", "timeout_ms": 10_000}),
        )
        .await;
    assert!(ok, "the tool ran; the command failed: {out}");
    assert!(out.contains("exit 3"), "{out}");
}

#[tokio::test]
async fn background_start_logs_kill() {
    let h = host(&workdir());
    let sleep = if cfg!(windows) {
        "Start-Sleep -Seconds 30"
    } else {
        "sleep 30"
    };
    let (ok, out) = h
        .call("shell.start", &json!({"cmd": sleep, "title": "sleeper"}))
        .await;
    assert!(ok, "{out}");
    let id = out
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    assert!(id.starts_with("sh-"), "{out}");
    let (ok, out) = h.call("shell.logs", &json!({"id": id})).await;
    assert!(ok, "{out}");
    assert!(out.contains("running"), "{out}");
    let (ok, out) = h.call("shell.kill", &json!({"id": id})).await;
    assert!(ok, "{out}");
    assert!(out.contains("killed"), "{out}");
}

#[tokio::test]
async fn workdir_outside_the_folder_is_refused() {
    let h = host(&workdir());
    let away = if cfg!(windows) {
        "C:\\Windows\\Temp"
    } else {
        "/tmp"
    };
    let (ok, out) = h
        .call("shell.exec", &json!({"cmd": "echo hi", "workdir": away}))
        .await;
    assert!(!ok, "outside the fence must not run: {out}");
    assert!(out.contains("outside"), "{out}");
    let here = workdir();
    let (ok, out) = h
        .call("shell.exec", &json!({"cmd": "echo hi", "workdir": here}))
        .await;
    assert!(ok, "the session's own folder runs: {out}");
}

#[tokio::test]
async fn the_shell_follows_a_project_switch_in_the_same_turn() {
    let h = host_with(&workdir(), true);
    let there = std::env::temp_dir().join(format!("parzi-switch-{}", std::process::id()));
    std::fs::create_dir_all(&there).unwrap();
    let there = there.display().to_string();
    let project = parzi_core::brain::project_upsert(None, "Switch target", &there).unwrap();
    let (ok, out) = h
        .call("project.switch", &json!({"slug": project.slug}))
        .await;
    assert!(ok, "{out}");
    let (ok, out) = h.call("shell.exec", &json!({"cmd": "pwd"})).await;
    assert!(ok, "{out}");
    let leaf = std::path::Path::new(&there)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(out.contains(&leaf), "runs in the new folder: {out}");
    let (ok, out) = h
        .call("shell.exec", &json!({"cmd": "echo hi", "workdir": there}))
        .await;
    assert!(ok, "the new folder is inside the fence: {out}");
}

fn vendor(tool: &str, input: serde_json::Value) -> PermissionRequest {
    PermissionRequest {
        name: String::new(),
        id: "req-1".into(),
        tool: tool.into(),
        title: tool.into(),
        input,
        paths: vec![],
    }
}

fn refusal(d: PermissionDecision) -> String {
    match d {
        PermissionDecision::Deny(why) => why,
        PermissionDecision::Allow => panic!("must be refused"),
    }
}

#[tokio::test]
async fn vendor_shell_is_rerouted_not_carded() {
    let h = host(&workdir());
    for tool in ["Bash", "bash", "PowerShell"] {
        let why = refusal(h.decide(vendor(tool, json!({"command": "ls -la"}))).await);
        assert_eq!(
            why,
            format!(
                r#"{tool} is off in Parzi. Call `shell_exec` on the `parzi` server with {{"cmd":"ls -la"}}."#
            ),
            "an agent whose prefix Parzi cannot know gets the MCP tool and its server"
        );
    }
    let claude = host(&workdir()).calling_as("claude");
    let why = refusal(
        claude
            .decide(vendor(
                "Bash",
                json!({"command": "cargo test", "timeout": 300000}),
            ))
            .await,
    );
    assert_eq!(
        why,
        r#"Bash is off in Parzi. Call `mcp__parzi__shell_exec` with {"cmd":"cargo test","timeout_ms":300000}."#,
        "Claude gets the name it can call and the same command"
    );
    let why = refusal(
        claude
            .decide(vendor(
                "Bash",
                json!({"command": "npm run dev", "run_in_background": true}),
            ))
            .await,
    );
    assert!(
        why.contains(r#"`mcp__parzi__shell_start` with {"cmd":"npm run dev"}"#)
            && why.contains("`mcp__parzi__shell_logs`"),
        "a background command starts in the background: {why}"
    );
    let why = refusal(
        claude
            .decide(vendor("BashOutput", json!({"bash_id": "sh-7"})))
            .await,
    );
    assert!(
        why.contains(r#"`mcp__parzi__shell_logs` with {"id":"sh-7"}"#),
        "{why}"
    );
    let why = refusal(
        claude
            .decide(vendor("KillShell", json!({"shell_id": "b1"})))
            .await,
    );
    assert!(why.contains("`mcp__parzi__shell_kill` with"), "{why}");
    let why = refusal(
        claude
            .decide(vendor("Monitor", json!({"command": "tail -f log"})))
            .await,
    );
    assert!(
        why.contains(r#"`mcp__parzi__shell_start` with {"cmd":"tail -f log"}"#),
        "{why}"
    );
    let why = refusal(claude.decide(vendor("Bash", json!({}))).await);
    assert!(why.contains("<the same command>"), "{why}");
    for tool in ["shell", "execute", "run_command"] {
        let d = h
            .decide(PermissionRequest {
                name: String::new(),
                id: "req-2".into(),
                tool: tool.into(),
                title: "run_terminal_command".into(),
                input: json!({"command": "echo hi"}),
                paths: vec![],
            })
            .await;
        assert!(
            !matches!(&d, PermissionDecision::Deny(why) if why.contains("is off in Parzi")),
            "{tool}: an agent whose only shell is its own keeps it: {d:?}"
        );
    }
}

#[tokio::test]
async fn the_agent_hears_whether_parzi_serves_its_shell() {
    home("shell-tools");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            a.say("hi");
            Ok(parzi_providers::TurnEnd::Completed)
        }),
    );
    for (no_shell, serves) in [(false, true), (true, false)] {
        let mut cfg = ParziConfig::default();
        cfg.lanes.default_mode = "auto".into();
        cfg.lanes.no_shell = no_shell;
        let (orch, store) = orch_with(cfg, std::slice::from_ref(&fake));
        let (meta, _rx) = orch
            .spawn("t", "claude", "hi", None, &workdir(), "low", vec![], None)
            .await
            .unwrap();
        settle(&store, &meta.id).await;
        let spec = fake.seen().pop().unwrap();
        assert_eq!(
            spec.tools.map(|t| t.shell),
            Some(serves),
            "shell off in Settings: {no_shell}"
        );
    }
}

#[tokio::test]
async fn the_shell_switch_turns_off_parzis_shell_and_the_agents_own() {
    home("shell-off");
    let store = SessionStore::open().unwrap();
    let sid = store.create("shell", "", "claude/m").unwrap().id;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut cfg = ParziConfig::default();
    cfg.lanes.no_shell = true;
    let host = ToolHost::new(ToolHostParts {
        session_id: sid.clone(),
        mode: ApprovalMode::Auto,
        edits_auto: false,
        full: false,
        cfg,
        store,
        tools: Arc::new(ToolExecutor {
            cwd: workdir(),
            mcp: Arc::new(McpManager::new(HashMap::new(), 60)),
            allowed: vec!["fs.*".into(), "plan.read".into()],
        }),
        approver: Arc::new(Allow),
        asker: None,
        shell: Arc::new(parzi_runtime::shell::ShellRegistry::new("test-shell")),
        harness: None,
        sink: RunSink::new(&sid, tx, None),
        cancel: CancellationToken::new(),
    });
    let (ok, _) = host.call("shell.exec", &json!({"cmd": "echo hi"})).await;
    assert!(!ok, "no commands with the shell off");
    let d = host
        .decide(PermissionRequest {
            name: String::new(),
            id: "req-1".into(),
            tool: "Bash".into(),
            title: "Bash".into(),
            input: json!({}),
            paths: vec![],
        })
        .await;
    assert!(
        matches!(d, PermissionDecision::Deny(_)),
        "the agent's own shell is off too"
    );
}
