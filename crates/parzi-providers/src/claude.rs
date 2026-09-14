//! `claude` adapter: Anthropic Messages API behind whichever credential wins.
//!
//! Resolution order (first hit decides the billing class):
//! 1. keyring `claude-code` — an OAuth bearer pasted by hand      → subscription
//! 2. `ANTHROPIC_AUTH_TOKEN`                                       → subscription
//! 3. `~/.claude/.credentials.json` `claudeAiOauth.accessToken`
//!    (Claude Code CLI sign-in; read-only, never written)         → subscription
//! 4. keyring `anthropic` / `ANTHROPIC_API_KEY`                    → api key
//!
//! OAuth bearers go on `Authorization` with the `oauth-2025-04-20` beta.

use crate::anthropic::AnthropicNative;
use crate::types::{env_key, keyring_get, now_secs, read_json_file};

/// Claude Code CLI sign-in as seen on disk.
struct CliSignIn {
    access: String,
    /// Unix ms from the file; None when absent.
    expires_at_ms: Option<u64>,
    /// "max" | "pro" | … from the file, None when absent.
    plan: Option<String>,
}

fn cli_sign_in() -> Option<CliSignIn> {
    let v = read_json_file(&dirs::home_dir()?.join(".claude/.credentials.json"))?;
    let o = v.get("claudeAiOauth")?;
    let access = o
        .get("accessToken")
        .and_then(|t| t.as_str())
        .filter(|t| !t.trim().is_empty())?
        .to_string();
    Some(CliSignIn {
        access,
        expires_at_ms: o.get("expiresAt").and_then(|n| n.as_u64()),
        plan: o
            .get("subscriptionType")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
    })
}

fn plan_label(plan: Option<&str>) -> String {
    match plan {
        Some("max") => "Claude Max".into(),
        Some("pro") => "Claude Pro".into(),
        Some("team") => "Claude Team".into(),
        Some("enterprise") => "Claude Enterprise".into(),
        Some(other) if !other.is_empty() => format!("Claude ({other})"),
        _ => "Claude subscription".into(),
    }
}

/// Build the adapter. Subscription bearers win; an API key is the fallback.
pub fn claude() -> AnthropicNative {
    let api_key = keyring_get("anthropic").or_else(|| env_key("ANTHROPIC_API_KEY"));
    let mut expired = false;
    let mut account: Option<String> = None;
    let oauth = keyring_get("claude-code")
        .map(|t| {
            account = Some("Claude subscription".into());
            t
        })
        .or_else(|| {
            env_key("ANTHROPIC_AUTH_TOKEN").map(|t| {
                account = Some("Claude subscription (env)".into());
                t
            })
        })
        .or_else(|| {
            cli_sign_in().map(|s| {
                if let Some(ms) = s.expires_at_ms {
                    expired = ms / 1000 < now_secs();
                }
                account = Some(plan_label(s.plan.as_deref()));
                s.access
            })
        });
    AnthropicNative {
        id: "claude",
        api_key,
        oauth_token: oauth,
        oauth_expired: expired,
        account,
        beta: vec!["oauth-2025-04-20".into()],
        missing_hint: "sign in with the Claude Code CLI (`claude`) or add an Anthropic API key".into(),
        static_models: crate::catalog::claude(),
    }
}
