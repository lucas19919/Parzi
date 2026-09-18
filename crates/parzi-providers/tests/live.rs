//! Live checks against the vendor programs installed on this machine.
//! Ignored by default: they need the programs, and the turn checks spend a
//! few tokens of the signed-in plan. Run with
//! `cargo test -p parzi-providers --test live -- --ignored --nocapture`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use parzi_core::config::ParziConfig;
use parzi_providers::{
    PermissionDecision, PermissionGate, PermissionRequest, ProviderEvent, TurnEnd, TurnSpec,
};
use tokio_util::sync::CancellationToken;

/// Refuses everything and remembers what it was asked.
#[derive(Default)]
struct Refuse {
    asked: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl PermissionGate for Refuse {
    async fn decide(&self, r: PermissionRequest) -> PermissionDecision {
        println!("  gate asked: {} ({})", r.title, r.tool);
        self.asked.lock().unwrap().push(r.tool);
        PermissionDecision::Deny("refused by the live check".into())
    }
}

#[tokio::test]
#[ignore = "needs the vendor programs"]
async fn every_provider_reports_where_it_stands() {
    let cfg = ParziConfig::default();
    for id in parzi_providers::PROVIDERS {
        let p = parzi_providers::provider(id, &cfg).unwrap();
        let s = p.status().await;
        println!(
            "{id:12} {:?} gated={} v={:?} account={:?} models={} usage={:?} hint={:?}",
            s.state,
            p.gated(),
            s.version,
            s.account,
            s.models.len(),
            s.usage,
            s.hint
        );
    }
}

fn folder(tag: &str) -> PathBuf {
    let cwd = std::env::temp_dir().join(format!("parzi-live-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).unwrap();
    cwd
}

async fn one_turn(
    id: &str,
    model: Option<&str>,
    cwd: PathBuf,
    prompt: &str,
    gate: Arc<Refuse>,
) -> (
    Result<TurnEnd, parzi_providers::ProviderError>,
    Vec<ProviderEvent>,
) {
    let cfg = ParziConfig::default();
    let p = parzi_providers::provider(id, &cfg).unwrap();
    let spec = TurnSpec {
        session_id: "live".into(),
        cwd,
        model: model.map(str::to_string),
        effort: None,
        instructions: Some("This is an automated check of a harness. Be brief.".into()),
        resume: None,
        prompt: prompt.into(),
        images: vec![],
        tools: None,
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let end = p.run_turn(spec, gate, tx, CancellationToken::new()).await;
    let mut events = vec![];
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    (end, events)
}

fn show(id: &str, end: &Result<TurnEnd, parzi_providers::ProviderError>, events: &[ProviderEvent]) {
    println!("{id}: {end:?}");
    for e in events {
        match e {
            ProviderEvent::TextDelta(_) | ProviderEvent::ReasoningDelta(_) => {}
            other => println!("  {other:?}"),
        }
    }
}

const PONG: &str = "Reply with exactly the word pong.";

#[tokio::test]
#[ignore = "spends a few tokens of the Claude plan"]
async fn claude_answers_a_turn() {
    let gate = Arc::new(Refuse::default());
    let (end, events) = one_turn("claude", Some("haiku"), folder("claude-pong"), PONG, gate).await;
    show("claude", &end, &events);
    assert_eq!(end.unwrap(), TurnEnd::Completed);
    assert!(events
        .iter()
        .any(|e| matches!(e, ProviderEvent::Session { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, ProviderEvent::Message(m) if m.to_lowercase().contains("pong"))));
}

#[tokio::test]
#[ignore = "spends a free OpenCode model's quota"]
async fn opencode_answers_a_turn() {
    let gate = Arc::new(Refuse::default());
    let (end, events) = one_turn(
        "opencode",
        Some("opencode/big-pickle"),
        folder("opencode-pong"),
        PONG,
        gate,
    )
    .await;
    show("opencode", &end, &events);
    assert_eq!(end.unwrap(), TurnEnd::Completed);
    assert!(events
        .iter()
        .any(|e| matches!(e, ProviderEvent::Message(m) if m.to_lowercase().contains("pong"))));
}

/// Parzi runs Claude Code with no settings, which also drops the repo's
/// CLAUDE.md; the driver hands that text over itself.
#[tokio::test]
#[ignore = "spends a few tokens of the Claude plan"]
async fn claude_reads_the_repos_claude_md() {
    let cwd = folder("claude-md");
    std::fs::write(
        cwd.join("CLAUDE.md"),
        "The project code word is PELICAN-42.\n",
    )
    .unwrap();
    let gate = Arc::new(Refuse::default());
    let ask = "What is the project code word from the repository's instructions? \
               Reply with the code word only, or NONE.";
    let (end, events) = one_turn("claude", Some("haiku"), cwd, ask, gate).await;
    show("claude", &end, &events);
    assert_eq!(end.unwrap(), TurnEnd::Completed);
    assert!(events
        .iter()
        .any(|e| matches!(e, ProviderEvent::Message(m) if m.contains("PELICAN-42"))));
}

const WRITE: &str = "Create a file named gate-check.txt in the current directory containing \
                     the word hello. If you are not allowed, reply with the word refused.";

/// The claim every gated agent makes: it asks before it writes, and a
/// refusal means the file is not there.
async fn a_write_waits_for_the_gate(id: &str, model: Option<&str>) {
    let cwd = folder(&format!("{id}-gate"));
    let gate = Arc::new(Refuse::default());
    let (end, events) = one_turn(id, model, cwd.clone(), WRITE, gate.clone()).await;
    show(id, &end, &events);
    let asked = gate.asked.lock().unwrap().clone();
    assert!(!asked.is_empty(), "{id} wrote without asking Parzi's gate");
    assert!(
        !cwd.join("gate-check.txt").exists(),
        "{id} wrote the file although the gate refused ({asked:?})"
    );
}

#[tokio::test]
#[ignore = "spends a few tokens of the Claude plan"]
async fn claude_asks_before_it_writes() {
    a_write_waits_for_the_gate("claude", Some("haiku")).await;
}

#[tokio::test]
#[ignore = "spends a free OpenCode model's quota"]
async fn opencode_asks_before_it_writes() {
    a_write_waits_for_the_gate("opencode", Some("opencode/big-pickle")).await;
}

#[tokio::test]
#[ignore = "spends a few tokens of the Grok plan"]
async fn grok_asks_before_it_writes_once_verified() {
    a_write_waits_for_the_gate("grok", None).await;
}
