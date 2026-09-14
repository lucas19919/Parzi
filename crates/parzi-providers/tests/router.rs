//! Smart Auto router gates: ordering, effort picks, retriable classification.
//!
//! Each test file is its own process, so env credentials set here cannot
//! leak sideways. Files on disk (`~/.claude`, `~/.codex`) may still sign a
//! provider in on the developer machine; assertions are written so that a
//! signed-in subscription never makes them fail.

use parzi_core::config::ParziConfig;
use parzi_providers::router::{
    auto_chain, auto_order, is_retriable, parse_cooldown_secs, pick, tier_fallback_chain,
};
use parzi_providers::{Billing, PROVIDERS};

#[test]
fn auto_order_normalises_aliases_and_fills_roster() {
    let mut cfg = ParziConfig::default();
    cfg.routing.auto_order = vec![
        "t3".into(),
        "claude-code".into(),
        "antigravity".into(),
        "claude".into(),
    ];
    let order = auto_order(&cfg);
    assert_eq!(
        order,
        vec!["claude", "antigravity", "codex", "opencode", "xai"]
    );
}

#[test]
fn auto_chain_never_spends_keys_by_default() {
    std::env::set_var("XAI_API_KEY", "test-key");
    std::env::set_var("ANTHROPIC_API_KEY", "test-key");
    let cfg = ParziConfig::default();
    for r in auto_chain("med", &cfg) {
        assert_eq!(
            r.reason, "subscription",
            "{} entered auto on a key",
            r.provider
        );
        assert_ne!(r.provider, "xai");
    }
}

#[test]
fn auto_chain_takes_keys_when_opted_in() {
    std::env::set_var("XAI_API_KEY", "test-key");
    let mut cfg = ParziConfig::default();
    cfg.routing.keys_in_auto = true;
    let chain = auto_chain("med", &cfg);
    let xai = chain
        .iter()
        .find(|r| r.provider == "xai")
        .expect("xai joins with keys_in_auto");
    assert_eq!(xai.reason, "api key (opt-in)");
    assert_eq!(xai.model, "grok-4.3");
    // Keys sit behind every subscription.
    let first_key = chain
        .iter()
        .position(|r| r.reason != "subscription")
        .unwrap();
    assert!(chain[..first_key]
        .iter()
        .all(|r| r.reason == "subscription"));
}

#[test]
fn auto_chain_picks_effort_models() {
    std::env::set_var("ANTIGRAVITY_ACCESS_TOKEN", "test-token");
    let cfg = ParziConfig::default();
    let low = auto_chain("low", &cfg);
    let high = auto_chain("high", &cfg);
    let ag_low = low.iter().find(|r| r.provider == "antigravity").unwrap();
    let ag_high = high.iter().find(|r| r.provider == "antigravity").unwrap();
    assert_eq!(ag_low.model, "gemini-3.8-flash-low");
    assert_eq!(ag_high.model, "gemini-3.8-flash-high");
    assert_eq!(pick("claude", "low"), "claude-sonnet-5");
    assert_eq!(pick("claude", "high"), "claude-opus-5");
}

#[test]
fn tier_fallback_chain_preserves_explicit_pick_first() {
    std::env::set_var("XAI_API_KEY", "test-key");
    let cfg = ParziConfig::default();
    let chain = tier_fallback_chain("xai", "grok-4", "med", &cfg);
    assert_eq!(chain[0].provider, "xai");
    assert_eq!(chain[0].model, "grok-4");
    assert_eq!(chain[0].reason, "explicit pick");
    // Primary never repeats; nothing after slot 0 is a key by default.
    assert!(chain[1..].iter().all(|r| r.provider != "xai"));
    assert!(chain[1..]
        .iter()
        .all(|r| r.reason == "subscription fallback"));
}

#[test]
fn tier_fallback_chain_maps_legacy_ids() {
    let cfg = ParziConfig::default();
    let chain = tier_fallback_chain("claude-code", "claude-opus-5", "med", &cfg);
    assert_eq!(chain[0].provider, "claude");
    assert!(chain[1..].iter().all(|r| r.provider != "claude"));
}

#[test]
fn roster_and_billing_classes() {
    assert_eq!(
        PROVIDERS,
        &["claude", "codex", "antigravity", "opencode", "xai"]
    );
    assert_eq!(Billing::Subscription.as_str(), "subscription");
    assert_eq!(Billing::ApiKey.as_str(), "api_key");
    assert_eq!(parzi_providers::canonical_id("anthropic"), Some("claude"));
    assert_eq!(parzi_providers::canonical_id("grok-cli"), Some("xai"));
    assert_eq!(parzi_providers::canonical_id("ollama"), None);
    assert_eq!(parzi_providers::key_entry("claude"), Some("anthropic"));
    assert_eq!(parzi_providers::key_entry("antigravity"), None);
}

#[test]
fn retriable_classification() {
    assert!(is_retriable("http 429: too many requests"));
    assert!(is_retriable("Rate limit reached, try again"));
    assert!(is_retriable("backend overloaded (503)"));
    assert!(is_retriable("overloaded_error"));
    assert!(!is_retriable("http 401: bad key"));
    assert!(!is_retriable("context length exceeded"));
    assert!(!is_retriable("provider not authenticated"));
}

#[test]
fn effort_options_are_provider_aware() {
    use parzi_providers::router::{effort_options, thinking_budget};
    let ag = effort_options("antigravity");
    assert_eq!(ag.len(), 5);
    assert_eq!(
        ag.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["low", "medium", "high", "extra", "ultra"]
    );
    assert!(ag.iter().any(|o| o.hint.contains("flash-medium")));
    let cl = effort_options("claude");
    assert!(cl.iter().any(|o| o.hint.contains("effort medium")));
    // Legacy id resolves to the same scheme.
    assert_eq!(effort_options("claude-code")[1].hint, cl[1].hint);
    let cx = effort_options("codex");
    assert!(cx.iter().any(|o| o.hint.contains("medium reasoning")));
    assert!(cx.iter().any(|o| o.hint.contains("xhigh")));
    let key = effort_options("xai");
    assert!(key.iter().any(|o| o.hint.contains("output")));
    assert_eq!(thinking_budget("low"), 4_096);
    assert_eq!(thinking_budget("high"), 32_768);
    assert_eq!(thinking_budget("medium"), 16_384);
    // Legacy id still resolves.
    assert_eq!(thinking_budget("med"), 16_384);
    assert_eq!(thinking_budget("extra"), 65_536);
    assert_eq!(thinking_budget("ultra"), 131_072);
}

#[test]
fn cooldown_parser_extracts_retry_windows() {
    assert_eq!(parse_cooldown_secs("retry after 120"), Some(120));
    assert_eq!(parse_cooldown_secs("http 429, retry-after: 30"), Some(30));
    assert_eq!(
        parse_cooldown_secs("rate limited, try again in 45s"),
        Some(45)
    );
    assert_eq!(
        parse_cooldown_secs("rate limited, try again in 2m"),
        Some(120)
    );
    assert_eq!(parse_cooldown_secs("http 429: too many requests"), Some(60));
    assert_eq!(parse_cooldown_secs("backend overloaded (503)"), Some(30));
    assert_eq!(parse_cooldown_secs("http 401: bad key"), None);
    assert_eq!(parse_cooldown_secs("context length exceeded"), None);
}

#[test]
fn families_collapse_variants() {
    let all = parzi_providers::catalog::antigravity();
    let fams: std::collections::HashSet<String> = all.iter().map(|m| m.family.clone()).collect();
    assert!(fams.contains("gemini-3.8-flash"));
    assert!(fams.contains("gemini-3.1-pro"));
    let flash: Vec<_> = all
        .iter()
        .filter(|m| m.family == "gemini-3.8-flash")
        .collect();
    assert_eq!(flash.len(), 3);
    assert!(flash.iter().all(|m| m.family_name == "Gemini 3.8 Flash"));
    assert!(flash
        .iter()
        .any(|m| m.variant.as_deref() == Some("medium") && m.is_default));
}

#[test]
fn every_pick_exists_in_its_catalog() {
    for p in PROVIDERS {
        for effort in ["low", "medium", "high", "extra", "ultra"] {
            let id = pick(p, effort);
            assert!(
                parzi_providers::catalog::for_provider(p)
                    .iter()
                    .any(|m| m.id == id),
                "{p}/{effort} picks {id} which is not in the catalog"
            );
        }
    }
}
