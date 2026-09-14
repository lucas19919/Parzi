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
        m(
            "claude-opus-5",
            "Claude Opus 5",
            1_000_000,
            128_000,
            5.0,
            25.0,
        )
        .default()
        .vision(),
        m(
            "claude-sonnet-5",
            "Claude Sonnet 5",
            1_000_000,
            128_000,
            3.0,
            15.0,
        )
        .vision(),
        m(
            "claude-haiku-4-5",
            "Claude Haiku 4.5",
            200_000,
            64_000,
            1.0,
            5.0,
        )
        .vision(),
        m(
            "claude-fable-5-1",
            "Claude Fable 5.1",
            1_000_000,
            128_000,
            10.0,
            50.0,
        )
        .vision(),
        m(
            "claude-opus-4-8",
            "Claude Opus 4.8",
            1_000_000,
            128_000,
            5.0,
            25.0,
        )
        .vision()
        .legacy(),
        m(
            "claude-sonnet-4-6",
            "Claude Sonnet 4.6",
            1_000_000,
            128_000,
            3.0,
            15.0,
        )
        .vision()
        .legacy(),
    ]
}

/// Current xAI lineup (verified 2026-09-14 against xAI docs):
/// grok-4.3 is the recommended default, grok-4.6 the flagship, grok-4.1-fast
/// the cheap tier, grok-code-fast-1 the coding model. grok-3/grok-4 were
/// RETIRED 2026-05-15 (server-side redirect to grok-4.3 + rebill) and stay
/// only as legacy rows so old threads resolve. Chat Completions is xAI's
/// legacy endpoint (Responses recommended) — migration is open, wire unchanged.
pub fn xai() -> Vec<Model> {
    vec![
        m("grok-4.3", "Grok 4.3", 1_000_000, 128_000, 1.25, 2.5)
            .default()
            .vision(),
        m("grok-4.6", "Grok 4.6", 500_000, 500_000, 2.0, 6.0).vision(),
        m("grok-4.5", "Grok 4.5", 500_000, 500_000, 2.0, 6.0).vision(),
        m(
            "grok-4.1-fast",
            "Grok 4.1 Fast",
            2_000_000,
            128_000,
            0.2,
            0.5,
        ),
        m(
            "grok-code-fast-1",
            "Grok Code Fast 1",
            256_000,
            32_768,
            1.0,
            2.0,
        ),
        m(
            "grok-build-0.1",
            "Grok Build 0.1",
            256_000,
            32_768,
            1.0,
            2.0,
        )
        .legacy(),
        m(
            "grok-4.20-0309-reasoning",
            "Grok 4.20 Reasoning",
            1_000_000,
            128_000,
            1.25,
            2.5,
        ),
        m(
            "grok-4.20-0309-non-reasoning",
            "Grok 4.20 Non-Reasoning",
            1_000_000,
            128_000,
            1.25,
            2.5,
        ),
        m(
            "grok-4.20-multi-agent-0309",
            "Grok 4.20 Multi-Agent",
            1_000_000,
            128_000,
            1.25,
            2.5,
        ),
        m("grok-4", "Grok 4", 256_000, 32_768, 3.0, 15.0)
            .vision()
            .legacy(),
        m("grok-3", "Grok 3", 131_072, 32_768, 3.0, 15.0).legacy(),
    ]
}

/// OpenCode Zen (free, $0) + Go ($10/mo subscription) rosters, verified
/// against the local serve `/config/providers` on 2026-09-14 and the Go
/// docs table. Limits/prices/capabilities are the serve values verbatim;
/// the adapter merges live `go/v1/models` ids on top without wiping these.
/// Local `opencode serve` exposes NO OpenAI-compatible surface (only its own
/// API + web UI), so Parzi talks to the hosted Zen bases directly.
pub fn opencode() -> Vec<Model> {
    let mut out = vec![
        // -- Go flagships & coding picks (chat/completions kind) --
        m("kimi-k3", "Kimi K3", 1_048_576, 131_072, 3.0, 15.0)
            .default()
            .vision(),
        m(
            "kimi-k2.7-code",
            "Kimi K2.7 Code",
            262_144,
            262_144,
            0.95,
            4.0,
        )
        .vision(),
        m("kimi-k2.6", "Kimi K2.6", 262_144, 65_536, 0.95, 4.0).vision(),
        m(
            "glm-5.3-flash",
            "GLM-5.3-Flash",
            1_000_000,
            131_072,
            0.15,
            0.5,
        )
        .vision(),
        m("glm-5.3", "GLM-5.3", 1_000_000, 131_072, 1.4, 4.4),
        m("glm-5.2", "GLM-5.2", 1_000_000, 131_072, 1.4, 4.4),
        m("glm-5.1", "GLM-5.1", 202_752, 32_768, 1.4, 4.4),
        m("qwen3.8-max", "Qwen3.8 Max", 1_000_000, 131_072, 2.0, 6.0).vision(),
        m("qwen3.7-max", "Qwen3.7 Max", 1_000_000, 65_536, 2.5, 7.5),
        m("qwen3.7-plus", "Qwen3.7 Plus", 1_000_000, 65_536, 0.4, 1.6).vision(),
        m("qwen3.6-plus", "Qwen3.6 Plus", 1_000_000, 65_536, 0.5, 3.0).vision(),
        m(
            "deepseek-v4-pro",
            "DeepSeek V4 Pro",
            1_000_000,
            384_000,
            0.66,
            1.98,
        ),
        m(
            "deepseek-v4-flash",
            "DeepSeek V4 Flash",
            1_000_000,
            384_000,
            0.15,
            0.6,
        ),
        m(
            "deepseek-v4-flash-vision-exp",
            "DeepSeek V4 Flash Vision Exp",
            1_000_000,
            384_000,
            0.15,
            0.6,
        )
        .vision(),
        m(
            "deepseek-v4.1-flash",
            "DeepSeek V4.1 Flash",
            1_000_000,
            384_000,
            0.15,
            0.6,
        )
        .vision(),
        m("longcat-2.0", "LongCat-2.0", 1_000_000, 131_072, 0.3, 1.2),
        m("mimo-v2.5", "MiMo V2.5", 1_000_000, 128_000, 0.14, 0.28).vision(),
        m(
            "mimo-v2.5-pro",
            "MiMo V2.5 Pro",
            1_048_576,
            128_000,
            0.435,
            0.87,
        ),
        m("hy3", "Hy3", 256_000, 128_000, 0.14, 0.58),
        m(
            "hy4-preview",
            "Hy4 preview",
            1_024_000,
            64_000,
            0.834,
            2.501,
        ),
        m(
            "grok-build-0.1",
            "Grok Build 0.1",
            256_000,
            256_000,
            1.0,
            3.0,
        )
        .vision(),
        // -- Go messages kind (Anthropic wire) --
        m("minimax-m3", "MiniMax M3", 1_000_000, 131_072, 0.3, 1.2).vision(),
        m("minimax-m2.7", "MiniMax M2.7", 204_800, 131_072, 0.3, 1.2),
        m(
            "qwen3.8-flash",
            "Qwen3.8 Flash",
            1_000_000,
            131_072,
            0.15,
            0.47,
        )
        .vision(),
        // -- Go responses kind (OpenAI responses wire) --
        m(
            "muse-spark-1.3-contributor",
            "Muse Spark 1.3 Contributor",
            1_048_576,
            131_072,
            0.1,
            0.2,
        )
        .vision(),
        m(
            "muse-spark-1.2-contributor",
            "Muse Spark 1.2 Contributor",
            1_048_576,
            131_072,
            0.1,
            0.2,
        )
        .vision(),
        m("gpt-5.6-luna", "GPT-5.6 Luna", 1_050_000, 128_000, 0.2, 1.2).vision(),
        m("grok-4.6", "Grok 4.6", 500_000, 500_000, 2.0, 6.0).vision(),
        // -- Zen free tier (keep working after Go caps; chat kind) --
        m("big-pickle", "Big Pickle", 200_000, 32_000, 0.0, 0.0),
        m(
            "nemotron-3-ultra-free",
            "Nemotron 3 Ultra Free",
            1_000_000,
            128_000,
            0.0,
            0.0,
        ),
        m(
            "nemotron-3.5-lightning-free",
            "Nemotron 3.5 Lightning Free",
            262_144,
            262_144,
            0.0,
            0.0,
        ),
        m(
            "mimo-v2.5-free",
            "MiMo V2.5 Free",
            200_000,
            32_000,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "ling-3.0-flash-fin-free",
            "Ling 3.0 Flash Fin Free",
            262_144,
            32_768,
            0.0,
            0.0,
        ),
        // -- Zen free tier (responses kind) --
        m(
            "muse-spark-1.3-contributor-free",
            "Muse Spark 1.3 Free",
            1_048_576,
            131_072,
            0.0,
            0.0,
        )
        .vision(),
        m(
            "muse-spark-1.2-contributor-free",
            "Muse Spark 1.2 Free",
            1_048_576,
            131_072,
            0.0,
            0.0,
        )
        .vision(),
    ];
    sort_models(&mut out);
    out
}

/// Codex roster, split by access path (verified 2026-09-14):
/// ChatGPT sign-in serves the current generation (gpt-5.5 default, terra/luna
/// effort tiers); the API-key path additionally serves the frozen `-codex`
/// snapshots. The adapter remaps dead ids per path, so old threads survive.
pub fn codex() -> Vec<Model> {
    vec![
        m("gpt-5.5", "GPT-5.5", 400_000, 128_000, 5.0, 30.0)
            .default()
            .vision(),
        m("gpt-5.6-terra", "GPT-5.6 Terra", 400_000, 128_000, 0.0, 0.0).vision(),
        m("gpt-5.6-luna", "GPT-5.6 Luna", 400_000, 128_000, 0.0, 0.0).vision(),
        m(
            "gpt-5.3-codex",
            "GPT-5.3 Codex",
            400_000,
            128_000,
            1.75,
            14.0,
        )
        .vision(),
        m("gpt-5-codex", "GPT-5 Codex", 400_000, 128_000, 1.25, 10.0)
            .vision()
            .legacy(),
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
