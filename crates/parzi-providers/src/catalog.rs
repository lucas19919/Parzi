//! Static model catalogs: display names, limits, prices, capability overlay.
//! Display pattern borrowed from the MIT T3 clones: name + provider + cost tier.
//! Capability overlay follows t3code's ModelManifest (simplified, no remote file):
//! bundled truth + live /models merge + disk cache + never-fail fallback.

use std::collections::HashMap;

use crate::types::Model;

fn m(id: &str, name: &str, ctx: u32, out: u32, pin: f64, pout: f64) -> Model {
    Model {
        id: id.into(),
        name: name.into(),
        context_limit: ctx,
        output_limit: out,
        price_in: pin,
        price_out: pout,
        tools: true,
        vision: false,
        legacy: false,
        is_default: false,
        family: id.into(),
        family_name: name.into(),
        variant: None,
    }
}

/// Bundled catalog for a roster id (aliases accepted). Empty for unknown ids.
pub fn for_provider(id: &str) -> Vec<Model> {
    match crate::canonical_id(id) {
        Some("claude") => claude(),
        Some("codex") => codex(),
        Some("antigravity") => antigravity(),
        Some("opencode") => opencode(),
        Some("xai") => xai(),
        _ => vec![],
    }
}

/// Anthropic Messages API ids (API-key rates; a Claude Max/Pro sign-in bills 0).
/// Ids are the bare aliases — never date-suffixed.
pub fn claude() -> Vec<Model> {
    vec![
        m("claude-opus-5", "Claude Opus 5", 1_000_000, 128_000, 5.0, 25.0)
            .default()
            .vision(),
        m("claude-sonnet-5", "Claude Sonnet 5", 1_000_000, 128_000, 2.0, 10.0).vision(),
        m("claude-haiku-4-5", "Claude Haiku 4.5", 200_000, 64_000, 1.0, 5.0).vision(),
        m("claude-fable-5-1", "Claude Fable 5.1", 1_000_000, 128_000, 10.0, 50.0).vision(),
        m("claude-opus-4-8", "Claude Opus 4.8", 1_000_000, 128_000, 5.0, 25.0)
            .vision()
            .legacy(),
        m("claude-sonnet-4-6", "Claude Sonnet 4.6", 1_000_000, 128_000, 3.0, 15.0)
            .vision()
            .legacy(),
    ]
}

pub fn xai() -> Vec<Model> {
    vec![
        m("grok-4", "Grok 4", 256_000, 32_768, 3.0, 15.0)
            .default()
            .vision(),
        m("grok-4-code", "Grok 4 Code", 256_000, 32_768, 3.0, 15.0).vision(),
        m("grok-3", "Grok 3", 131_072, 32_768, 3.0, 15.0).legacy(),
    ]
}

/// `opencode serve` fronts whatever the user's opencode account routes to;
/// the live /models pull replaces this placeholder on refresh.
pub fn opencode() -> Vec<Model> {
    vec![m("opencode-default", "opencode", 200_000, 64_000, 0.0, 0.0).default()]
}

pub fn codex() -> Vec<Model> {
    vec![
        m(
            "gpt-5.3-codex",
            "GPT-5.3 Codex",
            400_000,
            128_000,
            1.25,
            10.0,
        )
        .default()
        .vision(),
        m("gpt-5-codex", "GPT-5 Codex", 400_000, 128_000, 1.25, 10.0).vision(),
    ]
}

/// From the opencode-antigravity-auth model table (MIT, attributed in antigravity.rs).
/// Live Antigravity catalog, verified against `agy models` on 2026-09-09.
/// Effort variants are explicit ids (-low/-medium/-high); the adapter maps
/// family bases to the matching variant for the requested effort.
pub fn antigravity() -> Vec<Model> {
    vec![
        m(
            "gemini-3.8-flash-high",
            "Gemini 3.8 Flash (High)",
            1_048_576,
            65_536,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "gemini-3.8-flash-medium",
            "Gemini 3.8 Flash (Medium)",
            1_048_576,
            65_536,
            0.0,
            0.0,
        )
        .default()
        .vision(),
        m(
            "gemini-3.8-flash-low",
            "Gemini 3.8 Flash (Low)",
            1_048_576,
            65_536,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "gemini-3.7-flash-high",
            "Gemini 3.7 Flash (High)",
            1_048_576,
            65_536,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "gemini-3.7-flash-medium",
            "Gemini 3.7 Flash (Medium)",
            1_048_576,
            65_536,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "gemini-3.7-flash-low",
            "Gemini 3.7 Flash (Low)",
            1_048_576,
            65_536,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "gemini-3.6-flash-high",
            "Gemini 3.6 Flash (High)",
            1_048_576,
            65_536,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "gemini-3.6-flash-medium",
            "Gemini 3.6 Flash (Medium)",
            1_048_576,
            65_536,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "gemini-3.6-flash-low",
            "Gemini 3.6 Flash (Low)",
            1_048_576,
            65_536,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "gemini-3.1-pro-high",
            "Gemini 3.1 Pro (High)",
            1_048_576,
            65_535,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "gemini-3.1-pro-low",
            "Gemini 3.1 Pro (Low)",
            1_048_576,
            65_535,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "claude-sonnet-4-6",
            "Claude Sonnet 4.6 (Thinking)",
            200_000,
            64_000,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "claude-opus-4-6-thinking",
            "Claude Opus 4.6 (Thinking)",
            200_000,
            64_000,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "gpt-oss-120b-medium",
            "GPT-OSS 120B (Medium)",
            131_072,
            32_768,
            0.0,
            0.0,
        )
        .family("gpt-oss-120b", "GPT-OSS 120B")
        .variant("medium"),
    ]
    .into_iter()
    .map(|mut m| {
        // Collapse -low/-medium/-high effort variants into families.
        if let Some((base, var)) = split_variant(&m.id) {
            m.family = base.to_string();
            m.family_name = match base {
                "gemini-3.8-flash" => "Gemini 3.8 Flash",
                "gemini-3.7-flash" => "Gemini 3.7 Flash",
                "gemini-3.6-flash" => "Gemini 3.6 Flash",
                "gemini-3.1-pro" => "Gemini 3.1 Pro",
                other => other,
            }
            .to_string();
            m.variant = Some(var.to_string());
        }
        m
    })
    .collect()
}

/// Picker ordering, shared by every surface: the default flagship first,
/// then A–Z by display name, legacy entries last. Stable for ties.
pub fn sort_models(models: &mut [Model]) {
    models.sort_by(|a, b| {
        (b.is_default, a.legacy, a.name.to_lowercase()).cmp(&(
            a.is_default,
            b.legacy,
            b.name.to_lowercase(),
        ))
    });
}

/// Split `gemini-3.8-flash-medium` → ("gemini-3.8-flash", "medium").
fn split_variant(id: &str) -> Option<(&str, &str)> {
    for suffix in ["-high", "-medium", "-low"] {
        if let Some(base) = id.strip_suffix(suffix) {
            if base.starts_with("gemini-") {
                return Some((base, suffix.trim_start_matches('-')));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Disk cache: last-seen live /models responses. Offline keeps working.
// t3code Manifest discipline: TTL-gated refresh, retry backoff, never fail,
// user kill-switch lives in config (`catalog_refresh`).
// ---------------------------------------------------------------------------

const CACHE_TTL_SECS: u64 = 24 * 3600;
const CACHE_RETRY_SECS: u64 = 5 * 60;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CacheFile {
    fetched_at: u64,
    models: HashMap<String, Vec<Model>>,
}

fn cache_path() -> Option<std::path::PathBuf> {
    parzi_core::paths::parzi_dir()
        .ok()
        .map(|d| d.join("catalog.json"))
}

/// Last-seen models for a provider, if cached and fresh.
pub fn cached_models(provider: &str) -> Option<Vec<Model>> {
    let text = std::fs::read_to_string(cache_path()?).ok()?;
    let cache: CacheFile = serde_json::from_str(&text).ok()?;
    if crate::types::now_secs().saturating_sub(cache.fetched_at) > CACHE_TTL_SECS {
        return None;
    }
    cache.models.get(provider).cloned()
}

/// Persist live-discovered models. Best-effort: failures are silent by design.
pub fn store_models(provider: &str, models: &[Model]) {
    let Some(path) = cache_path() else { return };
    let mut cache: CacheFile = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(CacheFile {
            fetched_at: 0,
            models: HashMap::new(),
        });
    // Don't let a failed/empty discovery wipe a good cache.
    if models.is_empty() {
        return;
    }
    cache.fetched_at = crate::types::now_secs();
    cache.models.insert(provider.into(), models.to_vec());
    if let Ok(bytes) = serde_json::to_string(&cache) {
        let _ = parzi_core::atomic_write(&path, bytes.as_bytes());
    }
}

/// Minimum gap between failed refresh attempts (callers track last failure).
pub fn retry_wait_secs() -> u64 {
    CACHE_RETRY_SECS
}
