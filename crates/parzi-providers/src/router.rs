//! Smart Auto router: subscriptions first, keys only when asked.
//!
//! Every provider reports the billing class of the credential it actually
//! resolved (`Provider::billing`). `auto` walks the configured order and
//! takes each signed-in subscription; API keys join only when
//! `routing.keys_in_auto` is on. An explicit pick of a keyed model is always
//! honoured (slot 0), and its failover slots follow the same rule — a dead
//! key never silently spends another key. A failed refresh never fails
//! routing: unauthed providers are skipped.

use parzi_core::config::ParziConfig;

use crate::types::{AuthStatus, Billing};

#[derive(Debug, Clone)]
pub struct Route {
    pub provider: String,
    pub model: String,
    /// Why this slot is where it is (surfaced on failover).
    pub reason: &'static str,
}

/// Default auto order when the config says nothing. xai is key-only, so it
/// sits last and only enters with `keys_in_auto`.
pub const DEFAULT_AUTO_ORDER: &[&str] = &["claude", "codex", "antigravity", "opencode", "xai"];

/// Effort-appropriate model per provider. Ids must exist in the catalog.
pub fn pick(provider: &str, effort: &str) -> &'static str {
    match (provider, effort) {
        ("claude", "low") => "claude-sonnet-5",
        ("claude", _) => "claude-opus-5",
        ("codex", _) => "gpt-5.3-codex",
        ("antigravity", "low") => "gemini-3.8-flash-low",
        ("antigravity", "high" | "extra" | "ultra") => "gemini-3.8-flash-high",
        ("antigravity", _) => "gemini-3.8-flash-medium",
        ("opencode", _) => "opencode-default",
        ("xai", "low") => "grok-3",
        ("xai", _) => "grok-4",
        _ => "claude-opus-5",
    }
}

/// Configured auto order, normalised: aliases mapped, unknowns dropped,
/// duplicates removed, missing roster ids appended in default order.
pub fn auto_order(cfg: &ParziConfig) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for p in cfg
        .routing
        .auto_order
        .iter()
        .filter_map(|p| crate::canonical_id(p))
        .chain(DEFAULT_AUTO_ORDER.iter().copied())
    {
        if !out.iter().any(|x| x == p) {
            out.push(p.to_string());
        }
    }
    out
}

/// Live credential class for a provider: None when it is not signed in.
pub fn billing_of(cfg: &ParziConfig, provider: &str) -> Option<Billing> {
    let p = super::provider(provider, cfg).ok()?;
    match p.auth_status() {
        AuthStatus::Ok => Some(p.billing()),
        _ => None,
    }
}

/// True when the router may spend this provider without an explicit pick.
fn routable(cfg: &ParziConfig, provider: &str) -> Option<&'static str> {
    match billing_of(cfg, provider)? {
        Billing::Subscription => Some("subscription"),
        Billing::ApiKey if cfg.routing.keys_in_auto => Some("api key (opt-in)"),
        _ => None,
    }
}

/// Ordered chain for `auto`. Empty when nothing routable is signed in —
/// the caller turns that into a "sign in" error rather than spending a key.
pub fn auto_chain(effort: &str, cfg: &ParziConfig) -> Vec<Route> {
    auto_order(cfg)
        .into_iter()
        .filter_map(|p| {
            let reason = routable(cfg, &p)?;
            Some(Route {
                model: pick(&p, effort).to_string(),
                provider: p,
                reason,
            })
        })
        .collect()
}

/// Failover chain for an explicit selection. Slot 0 is always the requested
/// model (any class, including API keys). Later slots are the routable
/// providers in auto order, primary excluded.
pub fn tier_fallback_chain(
    primary_provider: &str,
    primary_model: &str,
    effort: &str,
    cfg: &ParziConfig,
) -> Vec<Route> {
    let primary = crate::canonical_id(primary_provider).unwrap_or(primary_provider);
    let mut out = vec![Route {
        provider: primary.to_string(),
        model: primary_model.to_string(),
        reason: "explicit pick",
    }];
    for r in auto_chain(effort, cfg) {
        if r.provider != primary {
            out.push(Route {
                reason: if r.reason == "subscription" {
                    "subscription fallback"
                } else {
                    "api key fallback (opt-in)"
                },
                ..r
            });
        }
    }
    out
}

/// Extract cooldown seconds from a 429 error or header description.
pub fn parse_cooldown_secs(err: &str) -> Option<u64> {
    let lower = err.to_lowercase();
    if let Some(pos) = lower.find("retry after ") {
        let rest = &lower[pos + 12..];
        if let Some(num_str) = rest.split(|c: char| !c.is_numeric()).next() {
            if let Ok(n) = num_str.parse::<u64>() {
                return Some(n);
            }
        }
    }
    if let Some(pos) = lower.find("retry-after: ") {
        let rest = &lower[pos + 13..];
        if let Some(num_str) = rest.split(|c: char| !c.is_numeric()).next() {
            if let Ok(n) = num_str.parse::<u64>() {
                return Some(n);
            }
        }
    }
    if let Some(pos) = lower.find("try again in ") {
        let rest = &lower[pos + 13..];
        let token = rest.split_whitespace().next().unwrap_or("");
        if let Some(s) = token.strip_suffix('s') {
            if let Ok(n) = s.parse::<u64>() {
                return Some(n);
            }
        } else if let Some(m) = token.strip_suffix('m') {
            if let Ok(n) = m.parse::<u64>() {
                return Some(n * 60);
            }
        } else if let Ok(n) = token.parse::<u64>() {
            return Some(n);
        }
    }
    if lower.contains("429") || lower.contains("rate limit") || lower.contains("rate_limit") {
        Some(60)
    } else if is_retriable(err) {
        Some(30)
    } else {
        None
    }
}

/// True for errors worth failing over: rate limits and overloaded backends.
/// Auth errors, bad requests, and context overflows must NOT fail over
/// (the next provider would fail the same way or mask a real problem).
pub fn is_retriable(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("429")
        || e.contains("rate limit")
        || e.contains("rate_limit")
        || e.contains("rate-limited")
        || e.contains("overloaded")
        || e.contains("overload")
        || e.contains("503")
        || e.contains("504")
        || e.contains("529")
        || e.contains("gateway timeout")
        || e.contains("capacity")
        || e.contains("timed out")
        || e.contains("timeout")
        || e.contains("try again")
}

// ---------------------------------------------------------------------------
// Provider-aware effort schemes. The Low/Med/High pill is NOT global: each
// provider maps effort to its own native knob (effort level, reasoning
// level, variant) or, when it has none, to output budget alone.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct EffortOption {
    pub id: String,
    pub label: String,
    /// Native meaning, shown under the pill ("effort medium", "flash-medium").
    pub hint: String,
}

fn opt(id: &str, label: &str, hint: &str) -> EffortOption {
    EffortOption {
        id: id.into(),
        label: label.into(),
        hint: hint.into(),
    }
}

/// Thinking budget for the few Anthropic models that still take one
/// (Haiku 4.5 and older); current models take `output_config.effort`.
/// Clamped under max_tokens by the caller, so upper rungs are safe.
pub fn thinking_budget(effort: &str) -> u32 {
    match effort {
        "low" => 4_096,
        "medium" | "med" => 16_384,
        "high" => 32_768,
        "extra" => 65_536,
        "ultra" => 131_072,
        _ => 16_384,
    }
}

pub fn effort_options(provider: &str) -> Vec<EffortOption> {
    match crate::canonical_id(provider).unwrap_or(provider) {
        "antigravity" => vec![
            opt("low", "Low", "flash-low variant"),
            opt("medium", "Medium", "flash-medium variant"),
            opt("high", "High", "flash-high variant"),
            opt("extra", "Extra", "flash-high variant · 128k output"),
            opt("ultra", "Ultra", "flash-high variant · 256k output"),
        ],
        "claude" => vec![
            opt("low", "Low", "adaptive thinking · effort low"),
            opt("medium", "Medium", "adaptive thinking · effort medium"),
            opt("high", "High", "adaptive thinking · effort high"),
            opt("extra", "Extra", "effort high · 128k output"),
            opt("ultra", "Ultra", "effort high · 256k output"),
        ],
        "codex" => vec![
            opt("low", "Low", "low reasoning"),
            opt("medium", "Medium", "medium reasoning"),
            opt("high", "High", "high reasoning"),
            opt("extra", "Extra", "xhigh reasoning"),
            opt("ultra", "Ultra", "xhigh reasoning · max output"),
        ],
        // No native reasoning knob: effort sizes the output budget.
        _ => vec![
            opt("low", "Low", "4k output"),
            opt("medium", "Medium", "16k output"),
            opt("high", "High", "64k output"),
            opt("extra", "Extra", "128k output"),
            opt("ultra", "Ultra", "256k output"),
        ],
    }
}
