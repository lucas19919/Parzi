//! parzi-providers: one trait, thin natives. Five providers, one bar:
//! claude · codex · antigravity · opencode · xai. Adding a vendor = one file
//! plus one line in `PROVIDERS`.

pub mod anthropic;
pub mod antigravity;
pub mod antigravity_oauth;
pub mod catalog;
pub mod claude;
pub mod codex;
pub mod compat_providers;
pub mod openai_compat;
pub mod opencode;
mod opencode_wire;
pub mod router;
pub mod types;

pub use types::{
    AuthStatus, Billing, ChatReq, EventRx, EventTx, Model, Provider, ProviderHealth, StreamEvent,
    ToolDef, desanitize_tool, sanitize_tool,
};

use parzi_core::config::ParziConfig;

/// The whole roster, in picker order. Every surface (picker, settings,
/// doctor, CLI, health) iterates this list, so it can never disagree.
pub const PROVIDERS: &[&str] = &["claude", "codex", "antigravity", "opencode", "xai"];

/// Display name per provider id.
pub fn display_name(id: &str) -> &'static str {
    match canonical_id(id) {
        Some("claude") => "Claude",
        Some("codex") => "Codex",
        Some("antigravity") => "Antigravity",
        Some("opencode") => "OpenCode",
        Some("xai") => "Grok",
        _ => "Unknown",
    }
}

/// Map legacy / alias ids onto the roster. Old sessions and configs still
/// say `claude-code` or `anthropic`; both are the `claude` adapter now.
pub fn canonical_id(id: &str) -> Option<&'static str> {
    match id {
        "claude" | "claude-code" | "anthropic" => Some("claude"),
        "codex" | "openai" => Some("codex"),
        "antigravity" => Some("antigravity"),
        "opencode" => Some("opencode"),
        "xai" | "grok" | "grok-cli" => Some("xai"),
        _ => None,
    }
}

/// Keyring entry that holds the *API key* for a provider (subscription
/// tokens live under other names and are never written by Settings).
/// None = provider takes no key (antigravity is OAuth only).
pub fn key_entry(provider: &str) -> Option<&'static str> {
    match canonical_id(provider)? {
        "claude" => Some("anthropic"),
        "codex" => Some("openai"),
        "xai" => Some("xai"),
        "opencode" => Some("opencode"),
        _ => None,
    }
}

/// Build the adapter for a router id. `base_url` from config overrides default.
pub fn provider(
    router_id: &str,
    cfg: &ParziConfig,
) -> Result<Box<dyn Provider>, parzi_core::error::ParziError> {
    let id = canonical_id(router_id).ok_or_else(|| {
        parzi_core::error::ParziError::Provider(
            router_id.into(),
            "unknown provider (Parzi routes claude, codex, antigravity, opencode, xai)".into(),
        )
    })?;
    let entry = cfg.providers.get(id).or_else(|| cfg.providers.get(router_id));
    let base = entry.and_then(|e| e.base_url.clone());
    let refresh = cfg.catalog_refresh;
    let boxed: Box<dyn Provider> = match id {
        "claude" => Box::new(claude::claude()),
        "codex" => Box::new(codex::Codex::new(base)),
        "antigravity" => Box::new(antigravity::Antigravity::new()),
        "opencode" => Box::new(opencode::opencode(base, refresh)),
        "xai" => Box::new(compat_providers::xai(base, refresh)),
        _ => unreachable!("canonical_id only returns roster ids"),
    };
    Ok(boxed)
}
