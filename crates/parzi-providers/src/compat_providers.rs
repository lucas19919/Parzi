//! Thin OpenAI-compatible adapters. Engine lives in openai_compat.

use crate::catalog;
use crate::openai_compat::OpenAiCompat;
use crate::types::{env_key, keyring_get, read_json_file, Billing};

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
    // H-10: exact keys only — never scrape the first string under any
    // `token`/`key`, which could exfiltrate another provider's credential.
    let home = dirs::home_dir()?;
    for rel in [".grok/user-settings.json", ".grok/settings.json"] {
        let v = read_json_file(&home.join(rel))?;
        for k in ["api_key", "apiKey", "key", "grok_api_key"] {
            if let Some(s) = v
                .get(k)
                .and_then(|x| x.as_str())
                .filter(|s| s.trim().len() > 10)
            {
                return Some(s.to_string());
            }
        }
    }
    None
}
