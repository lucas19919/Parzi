//! Picker ordering gates: default first, A–Z, legacy last; catalog lookup.

use parzi_providers::catalog::{for_provider, sort_models};
use parzi_providers::Model;

fn m(id: &str, name: &str) -> Model {
    Model {
        id: id.into(),
        name: name.into(),
        context_limit: 0,
        output_limit: 0,
        price_in: 0.0,
        price_out: 0.0,
        tools: true,
        vision: false,
        legacy: false,
        is_default: false,
        family: id.into(),
        family_name: name.into(),
        variant: None,
    }
}

#[test]
fn sort_models_default_first_alpha_legacy_last() {
    let mut v = vec![
        {
            let mut x = m("z", "Zulu");
            x.legacy = true;
            x
        },
        m("b", "Bravo"),
        {
            let mut x = m("a", "Alpha");
            x.is_default = true;
            x
        },
        m("c", "Charlie"),
    ];
    sort_models(&mut v);
    let ids: Vec<&str> = v.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b", "c", "z"]);
}

#[test]
fn catalog_lookup_accepts_aliases_and_rejects_retired() {
    assert!(!for_provider("claude").is_empty());
    assert_eq!(for_provider("claude-code").len(), for_provider("claude").len());
    assert_eq!(for_provider("anthropic").len(), for_provider("claude").len());
    assert!(!for_provider("grok").is_empty());
    assert!(for_provider("ollama").is_empty());
    assert!(for_provider("t3").is_empty());
    assert!(for_provider("openrouter").is_empty());
}

#[test]
fn every_catalog_has_exactly_one_default() {
    for p in parzi_providers::PROVIDERS {
        let n = for_provider(p).iter().filter(|m| m.is_default).count();
        assert_eq!(n, 1, "{p} has {n} defaults");
    }
}

#[test]
fn claude_ids_are_bare_aliases() {
    for m in for_provider("claude") {
        assert!(m.id.starts_with("claude-"), "{}", m.id);
        assert!(!m.id.chars().rev().take(8).all(|c| c.is_ascii_digit()), "date-suffixed id {}", m.id);
    }
}
