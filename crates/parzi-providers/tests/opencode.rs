//! OpenCode Zen/Go routing gates: real catalog, wire kinds, legacy compat.
//!
//! Each test file is its own process, so env credentials set here cannot
//! leak sideways.

use parzi_providers::catalog;
use parzi_providers::opencode::{
    resolve_model_id, route_for, ZenKind, DEFAULT_MODEL, LEGACY_PLACEHOLDER,
};
use parzi_providers::router::{is_retriable, parse_cooldown_secs, pick};

#[test]
fn catalog_is_the_real_zen_go_roster() {
    let all = catalog::opencode();
    // 28 Go + 7 Zen-free as verified 2026-09-14; live merge only adds.
    assert!(all.len() >= 35, "expected full roster, got {}", all.len());
    assert!(
        !all.iter().any(|m| m.id == LEGACY_PLACEHOLDER),
        "placeholder must be gone"
    );
    let def = all.iter().find(|m| m.is_default).expect("a default model");
    assert_eq!(def.id, DEFAULT_MODEL);
    // Capability overlay is real data, not generic defaults.
    let k3 = all.iter().find(|m| m.id == "kimi-k3").unwrap();
    assert_eq!(k3.context_limit, 1_048_576);
    assert_eq!(k3.output_limit, 131_072);
    assert!(k3.tools && k3.vision);
    let flash = all.iter().find(|m| m.id == "glm-5.3-flash").unwrap();
    assert_eq!((flash.price_in, flash.price_out), (0.15, 0.5));
    // Free tier bills zero.
    for id in ["big-pickle", "muse-spark-1.2-contributor-free"] {
        let m = all
            .iter()
            .find(|m| m.id == id)
            .unwrap_or_else(|| panic!("{id}"));
        assert_eq!((m.price_in, m.price_out), (0.0, 0.0), "{id}");
        assert!(m.tools, "{id} keeps tools");
    }
    // Image-capable models are flagged for the picker.
    assert!(all.iter().find(|m| m.id == "qwen3.8-flash").unwrap().vision);
    assert!(!all.iter().find(|m| m.id == "longcat-2.0").unwrap().vision);
}

#[test]
fn every_wire_kind_is_covered() {
    assert_eq!(
        route_for("glm-5.3-flash"),
        (parzi_providers::opencode::GO_BASE, ZenKind::Chat)
    );
    assert_eq!(
        route_for("kimi-k3"),
        (parzi_providers::opencode::GO_BASE, ZenKind::Chat)
    );
    assert_eq!(
        route_for("minimax-m3"),
        (parzi_providers::opencode::GO_BASE, ZenKind::Messages)
    );
    assert_eq!(
        route_for("qwen3.8-flash"),
        (parzi_providers::opencode::GO_BASE, ZenKind::Messages)
    );
    assert_eq!(
        route_for("muse-spark-1.3-contributor"),
        (parzi_providers::opencode::GO_BASE, ZenKind::Responses)
    );
    assert_eq!(
        route_for("gpt-5.6-luna"),
        (parzi_providers::opencode::GO_BASE, ZenKind::Responses)
    );
    // Free tier rides the Zen base.
    assert_eq!(
        route_for("big-pickle"),
        (parzi_providers::opencode::ZEN_BASE, ZenKind::Chat)
    );
    assert_eq!(
        route_for("muse-spark-1.2-contributor-free"),
        (parzi_providers::opencode::ZEN_BASE, ZenKind::Responses)
    );
    // Unknown future ids default to Go+chat (live merge usually knows them).
    assert_eq!(
        route_for("something-new-9"),
        (parzi_providers::opencode::GO_BASE, ZenKind::Chat)
    );
}

#[test]
fn legacy_placeholder_resolves_to_flagship() {
    assert_eq!(resolve_model_id("opencode-default"), "kimi-k3");
    assert_eq!(resolve_model_id("kimi-k3"), "kimi-k3");
}

#[test]
fn effort_picks_are_real_catalog_ids() {
    assert_eq!(pick("opencode", "low"), "glm-5.3-flash");
    assert_eq!(pick("opencode", "medium"), "kimi-k2.7-code");
    assert_eq!(pick("opencode", "high"), "kimi-k3");
    assert_eq!(pick("opencode", "ultra"), "kimi-k3");
    for effort in ["low", "medium", "high", "extra", "ultra"] {
        let id = pick("opencode", effort);
        assert!(
            catalog::for_provider("opencode").iter().any(|m| m.id == id),
            "pick {id} not in catalog"
        );
    }
    let opts = parzi_providers::router::effort_options("opencode");
    assert!(opts.iter().any(|o| o.hint.contains("kimi-k3")));
}

#[test]
fn quota_errors_fail_over_but_auth_does_not() {
    for e in [
        "monthly limit reached, try again next cycle",
        "insufficient_quota: no balance left",
        "usage limit hit on GLM-5.3-Flash",
    ] {
        assert!(is_retriable(e), "{e}");
        assert_eq!(parse_cooldown_secs(e), Some(30), "{e}");
    }
    // A 429 keeps its explicit window even when quota-worded.
    assert_eq!(
        parse_cooldown_secs("http 429: quota exceeded for opencode-go/kimi-k3"),
        Some(60)
    );
    assert!(!is_retriable("http 401: bad key"));
    assert!(!is_retriable("http 400: MissingSessionID"));
    assert!(!is_retriable("context length exceeded"));
    // Edge send-phase shed hops instead of killing the turn.
    assert!(is_retriable(
        "request: error sending request for url (https://opencode.ai/zen/v1/chat/completions)"
    ));
}

#[test]
fn tool_names_sanitize_round_trip() {
    use parzi_providers::ToolDef;
    use parzi_providers::{desanitize_tool, sanitize_tool};
    assert_eq!(sanitize_tool("fs.read"), "fs_read");
    assert_eq!(
        sanitize_tool("session.send_message"),
        "session_send_message"
    );
    let defs = vec![
        ToolDef {
            name: "fs.read".into(),
            description: String::new(),
            schema: serde_json::Value::Null,
        },
        ToolDef {
            name: "plain".into(),
            description: String::new(),
            schema: serde_json::Value::Null,
        },
    ];
    assert_eq!(desanitize_tool(&defs, "fs_read"), "fs.read");
    assert_eq!(desanitize_tool(&defs, "plain"), "plain");
    assert_eq!(desanitize_tool(&defs, "unknown_tool"), "unknown_tool");
}

#[test]
fn exact_key_resolution_prefers_env_without_logging() {
    std::env::set_var("OPENCODE_API_KEY", "test-zen-key");
    let key = parzi_providers::opencode::resolve_key();
    std::env::remove_var("OPENCODE_API_KEY");
    assert_eq!(key.as_deref(), Some("test-zen-key"));
}
