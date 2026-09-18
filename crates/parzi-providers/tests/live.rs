//! Live checks against the vendor programs installed on this machine.
//! Ignored by default: they need the programs, and the turn checks spend a
//! few tokens of the signed-in plan. Run with
//! `cargo test -p parzi-providers --test live -- --ignored --nocapture`.

use std::sync::Arc;

use parzi_core::config::ParziConfig;
use parzi_providers::{
    Access, PermissionDecision, PermissionGate, PermissionRequest, ProviderEvent, TurnEnd,
    TurnSpec,
};
use tokio_util::sync::CancellationToken;

struct Refuse;

#[async_trait::async_trait]
impl PermissionGate for Refuse {
    async fn decide(&self, r: PermissionRequest) -> PermissionDecision {
        println!("  gate asked: {} ({})", r.title, r.tool);
        PermissionDecision::Deny("read-only check".into())
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
            "{id:12} {:?} v={:?} account={:?} models={} usage={:?} hint={:?}",
            s.state,
            s.version,
            s.account,
            s.models.len(),
            s.usage,
            s.hint
        );
    }
}

async fn one_turn(id: &str, model: Option<&str>) -> (Result<TurnEnd, parzi_providers::ProviderError>, Vec<ProviderEvent>) {
    let cfg = ParziConfig::default();
    let p = parzi_providers::provider(id, &cfg).unwrap();
    let cwd = std::env::temp_dir().join("parzi-live-check");
    std::fs::create_dir_all(&cwd).unwrap();
    let spec = TurnSpec {
        session_id: "live".into(),
        cwd,
        model: model.map(str::to_string),
        effort: None,
        access: Access::ReadOnly,
        instructions: Some("This is an automated connectivity check. Answer in one word.".into()),
        resume: None,
        prompt: "Reply with exactly the word pong.".into(),
        images: vec![],
        tools: None,
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let end = p
        .run_turn(spec, Arc::new(Refuse), tx, CancellationToken::new())
        .await;
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

#[tokio::test]
#[ignore = "spends a few tokens of the Claude plan"]
async fn claude_answers_a_turn() {
    let (end, events) = one_turn("claude", Some("haiku")).await;
    show("claude", &end, &events);
    assert_eq!(end.unwrap(), TurnEnd::Completed);
    assert!(events.iter().any(|e| matches!(e, ProviderEvent::Session { .. })));
    assert!(events.iter().any(|e| matches!(e, ProviderEvent::Message(m) if m.to_lowercase().contains("pong"))));
}

#[tokio::test]
#[ignore = "spends a free OpenCode model's quota"]
async fn opencode_answers_a_turn() {
    let (end, events) = one_turn("opencode", Some("opencode/big-pickle")).await;
    show("opencode", &end, &events);
    assert_eq!(end.unwrap(), TurnEnd::Completed);
    assert!(events.iter().any(|e| matches!(e, ProviderEvent::Message(m) if m.to_lowercase().contains("pong"))));
}
