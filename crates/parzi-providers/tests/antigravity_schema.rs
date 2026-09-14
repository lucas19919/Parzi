//! Antigravity port gates: schema allowlist + request wrap shape.

use parzi_providers::antigravity::clean_schema;

#[test]
fn schema_keeps_allowlist_only() {
    let v = serde_json::json!({
        "type": "object",
        "title": "Drop me",
        "const": "x",
        "$ref": "#/nope",
        "properties": {
            "a": {"type": "string", "default": "d", "examples": ["e"]},
        },
        "additionalProperties": false,
    });
    let c = clean_schema(&v);
    assert_eq!(c.get("type").and_then(|t| t.as_str()), Some("object"));
    assert!(c.get("title").is_none());
    assert!(c.get("$ref").is_none());
    assert!(c.get("additionalProperties").is_none());
    // const became single-value enum at top level? top has no const→enum only if const present.
    let props = c.get("properties").unwrap();
    assert!(props.get("a").unwrap().get("default").is_none());
}

#[test]
fn const_becomes_enum() {
    let v = serde_json::json!({"const": "only"});
    let c = clean_schema(&v);
    assert_eq!(c.get("enum"), Some(&serde_json::json!(["only"])));
}

#[test]
fn empty_object_gets_placeholder() {
    let v = serde_json::json!({"type": "object"});
    let c = clean_schema(&v);
    assert!(c.get("properties").is_some());
}

#[test]
fn oauth_headers_carry_version() {
    let h = parzi_providers::antigravity_oauth::headers();
    assert!(h.contains_key("user-agent"));
    assert!(h.contains_key("x-goog-api-client"));
    assert!(h.contains_key("client-metadata"));
    let ua = h["user-agent"].to_str().unwrap();
    assert!(
        ua.contains(parzi_providers::antigravity_oauth::AG_VERSION),
        "{ua}"
    );
}

#[test]
fn auth_url_is_google_oauth() {
    let u = parzi_providers::antigravity_oauth::auth_url();
    assert!(u.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
    assert!(u.contains("access_type=offline"));
    // Every param Google requires must survive as its own query item
    // (regression: the Windows launcher once truncated the URL at the
    // first `&`, yielding Google's "missing response_type" 400).
    for param in [
        "client_id=",
        "redirect_uri=",
        "response_type=code",
        "scope=",
    ] {
        assert!(u.contains(param), "auth URL missing {param}: {u}");
    }
}

#[test]
fn catalog_capabilities_sane() {
    let all: Vec<parzi_providers::Model> = parzi_providers::PROVIDERS
        .iter()
        .flat_map(|p| parzi_providers::catalog::for_provider(p))
        .collect();
    assert!(!all.is_empty());
    for m in &all {
        assert!(
            !m.id.is_empty() && !m.name.is_empty(),
            "model needs id+name"
        );
    }
    // Flagships support tools; legacy marks are deliberate and few.
    let opus = all.iter().find(|m| m.id == "claude-opus-5").unwrap();
    assert!(opus.tools && opus.vision && !opus.legacy && opus.is_default);
    let legacy: Vec<_> = all.iter().filter(|m| m.legacy).collect();
    assert!(!legacy.is_empty() && legacy.len() < all.len() / 2);
    // Exactly one default per provider list.
    for list in [
        parzi_providers::catalog::claude(),
        parzi_providers::catalog::codex(),
    ] {
        assert_eq!(list.iter().filter(|m| m.is_default).count(), 1);
    }
}

#[test]
fn catalog_limits_match_spec() {
    let all = parzi_providers::catalog::antigravity();
    // Verified live via `agy models` 2026-09-09.
    let pro = all
        .iter()
        .find(|m| m.id == "gemini-3.8-flash-medium")
        .unwrap();
    assert_eq!(pro.context_limit, 1_048_576);
    assert!(pro.is_default);
    let claude = all.iter().find(|m| m.id == "claude-sonnet-4-6").unwrap();
    assert_eq!(claude.context_limit, 200_000);
    assert!(all.iter().any(|m| m.id == "gpt-oss-120b-medium"));
}

#[test]
fn effort_variants_map() {
    use parzi_providers::antigravity::with_effort;
    assert_eq!(
        with_effort("gemini-3.8-flash", "low"),
        "gemini-3.8-flash-low"
    );
    assert_eq!(
        with_effort("gemini-3.8-flash", "medium"),
        "gemini-3.8-flash-medium"
    );
    // Legacy id still resolves.
    assert_eq!(
        with_effort("gemini-3.8-flash", "med"),
        "gemini-3.8-flash-medium"
    );
    assert_eq!(
        with_effort("gemini-3.8-flash", "high"),
        "gemini-3.8-flash-high"
    );
    // Only low/medium/high variants exist: upper rungs ride high.
    assert_eq!(
        with_effort("gemini-3.8-flash", "extra"),
        "gemini-3.8-flash-high"
    );
    assert_eq!(
        with_effort("gemini-3.8-flash", "ultra"),
        "gemini-3.8-flash-high"
    );
    assert_eq!(
        with_effort("gemini-3.1-pro", "medium"),
        "gemini-3.1-pro-high"
    );
    assert_eq!(
        with_effort("gemini-3.8-flash-low", "high"),
        "gemini-3.8-flash-low"
    );
    assert_eq!(with_effort("claude-sonnet-4-6", "low"), "claude-sonnet-4-6");
}
