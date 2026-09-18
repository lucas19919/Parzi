//! parzi-providers: Parzi drives each vendor's own agent — Claude Code, the
//! Codex app-server, and the ACP agents (OpenCode, Grok, Antigravity,
//! Cursor) — the way t3code does. No model API is called from here and no
//! vendor credential passes through Parzi: every provider signs in,
//! refreshes and bills through its own program.

pub mod acp;
pub mod claude;
pub mod codex;
/// GitHub: the workspace wizard's credential, repo listing and clone-or-map.
pub mod github;
mod jsonrpc;
pub mod process;
pub mod types;

use std::sync::Arc;

use base64::Engine as _;
use parzi_core::config::ParziConfig;

pub use types::{
    now_secs, ErrorClass, EventTx, ModelInfo, PermissionDecision, PermissionGate,
    PermissionRequest, Provider, ProviderError, ProviderEvent, ProviderStatus, State, ToolServer,
    TurnEnd, TurnSpec, UsageWindow,
};

/// The roster, in picker order. Every surface (picker, settings, doctor,
/// CLI) iterates this list, so they can never disagree.
pub const PROVIDERS: &[&str] = &[
    "claude",
    "codex",
    "opencode",
    "grok",
    "antigravity",
    "cursor",
];

pub fn display_name(id: &str) -> &'static str {
    match canonical_id(id) {
        Some("claude") => "Claude",
        Some("codex") => "Codex",
        Some("opencode") => "OpenCode",
        Some("grok") => "Grok",
        Some("antigravity") => "Antigravity",
        Some("cursor") => "Cursor",
        _ => "Unknown",
    }
}

/// Map old ids onto the roster. Threads and configs written before the
/// vendor-agent switch still say `claude-code`, `anthropic`, `openai` or `xai`.
pub fn canonical_id(id: &str) -> Option<&'static str> {
    match id {
        "claude" | "claude-code" | "anthropic" => Some("claude"),
        "codex" | "openai" => Some("codex"),
        "opencode" => Some("opencode"),
        "grok" | "xai" | "grok-cli" => Some("grok"),
        "antigravity" => Some("antigravity"),
        "cursor" => Some("cursor"),
        _ => None,
    }
}

/// Split a `provider/model` spec. The model may itself contain `/`
/// (OpenCode's `anthropic/claude-sonnet-4-5`): only the first `/` splits.
/// `None` when the provider is not on the roster.
pub fn split_spec(spec: &str) -> Option<(&'static str, Option<String>)> {
    let (p, m) = match spec.split_once('/') {
        Some((p, m)) => (p, Some(m.trim().to_string()).filter(|m| !m.is_empty())),
        None => (spec, None),
    };
    Some((canonical_id(p.trim())?, m))
}

/// The driver for a roster id, pointed at the configured program (or the
/// vendor's usual name on PATH).
pub fn provider(id: &str, cfg: &ParziConfig) -> Option<Arc<dyn Provider>> {
    let id = canonical_id(id)?;
    let binary = cfg
        .providers
        .get(id)
        .map(|e| e.binary.clone())
        .unwrap_or_default();
    Some(match id {
        claude::ID => Arc::new(claude::Claude::new(&binary)),
        codex::ID => Arc::new(codex::Codex::new(&binary)),
        other => Arc::new(acp::Acp::new(acp::agent(other)?, &binary)),
    })
}

/// A local image as (media type, base64). `None` for anything that is not
/// a readable png/jpeg/gif/webp under 20 MiB.
pub(crate) fn image_base64(path: &std::path::Path) -> Option<(String, String)> {
    const MAX: u64 = 20 * 1024 * 1024;
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let media = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    if std::fs::metadata(path).ok()?.len() > MAX {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some((
        media.to_string(),
        base64::engine::general_purpose::STANDARD.encode(bytes),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_split_on_the_first_slash_only() {
        assert_eq!(
            split_spec("claude/opus[1m]"),
            Some(("claude", Some("opus[1m]".into())))
        );
        assert_eq!(
            split_spec("opencode/anthropic/claude-sonnet-4-5"),
            Some(("opencode", Some("anthropic/claude-sonnet-4-5".into())))
        );
        assert_eq!(split_spec("codex"), Some(("codex", None)));
        assert_eq!(
            split_spec("xai/grok-4"),
            Some(("grok", Some("grok-4".into())))
        );
        assert_eq!(
            split_spec("claude-code/sonnet"),
            Some(("claude", Some("sonnet".into())))
        );
        assert_eq!(split_spec("ollama/llama3"), None);
        assert_eq!(split_spec("auto"), None);
    }

    #[test]
    fn every_roster_id_builds_a_driver() {
        let cfg = ParziConfig::default();
        for id in PROVIDERS {
            let p = provider(id, &cfg).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(p.id(), *id);
            assert_ne!(display_name(id), "Unknown");
        }
    }
}
