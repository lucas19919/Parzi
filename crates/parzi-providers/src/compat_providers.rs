//! Thin OpenAI-compatible adapters. Engine lives in openai_compat.

use crate::catalog;
use crate::openai_compat::OpenAiCompat;
use crate::types::{Billing, env_key, find_token, keyring_get, read_json_file};

/// xAI (Grok). Keys only — there is no flat-rate API plan to route on.
/// Sources: keyring `xai`, `XAI_API_KEY`, `GROK_API_KEY`, then the Grok CLI's
/// own settings file (read-only scrape, shape-drift proof).
pub fn xai(base_url: Option<String>, refresh: bool) -> OpenAiCompat {
    let key = keyring_get("xai")
        .or_else(|| env_key("XAI_API_KEY"))
        .or_else(|| env_key("GROK_API_KEY"))
        .or_else(grok_cli_key);
    OpenAiCompat {
        id: "xai",
        base_url: base_url.unwrap_or_else(|| "https://api.x.ai/v1".into()),
        api_key: key,
        extra_headers: vec![],
        static_models: catalog::xai(),
        missing_hint: "add an xAI API key (or sign in with the Grok CLI)".into(),
        refresh,
        billing: Billing::ApiKey,
        account: None,
    }
}

fn grok_cli_key() -> Option<String> {
    let home = dirs::home_dir()?;
    for rel in [".grok/user-settings.json", ".grok/settings.json"] {
        if let Some(v) = read_json_file(&home.join(rel)) {
            if let Some(t) = find_token(&v) {
                return Some(t);
            }
        }
    }
    None
}

/// opencode in serve mode exposes an OpenAI-compatible endpoint that fronts
/// the user's own opencode account, so it routes like a subscription.
/// Credentials resolve: keyring/env `OPENCODE_API_KEY`, else best-effort
/// token scrape from opencode's own auth file (read-only, never written).
pub fn opencode(base_url: Option<String>, refresh: bool) -> OpenAiCompat {
    let key = keyring_get("opencode")
        .or_else(|| env_key("OPENCODE_API_KEY"))
        .or_else(opencode_file_token);
    OpenAiCompat {
        id: "opencode",
        base_url: base_url.unwrap_or_else(|| "http://localhost:4096/v1".into()),
        api_key: key,
        extra_headers: vec![],
        static_models: catalog::opencode(),
        missing_hint: "run `opencode serve` (or set OPENCODE_API_KEY)".into(),
        refresh,
        billing: Billing::Subscription,
        account: Some("opencode serve".into()),
    }
}

fn opencode_file_token() -> Option<String> {
    let home = dirs::home_dir()?;
    for rel in [
        ".local/share/opencode/auth.json",
        ".config/opencode/auth.json",
    ] {
        if let Some(v) = read_json_file(&home.join(rel)) {
            // Shape drifts across opencode versions; recursive scrape.
            if let Some(t) = find_token(&v) {
                return Some(t);
            }
        }
    }
    None
}
