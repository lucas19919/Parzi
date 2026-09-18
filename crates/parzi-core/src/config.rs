use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::{atomic_write, paths};

/// v2: providers are vendor programs Parzi drives (Claude Code, Codex, the
/// ACP agents), not HTTP endpoints. v1 files migrate on load.
pub const CONFIG_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParziConfig {
    pub version: u32,
    /// Per-provider settings, by roster id. A provider with no entry is on,
    /// found on PATH, and starts on its vendor's default model.
    #[serde(default)]
    pub providers: HashMap<String, ProviderEntry>,
    #[serde(default)]
    pub lanes: LaneDefaults,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub orchestrator: OrchLimits,
    #[serde(default)]
    pub routing: RoutingConfig,
    /// R-4: the spend a single run may cost before it pauses. Unset = no cap.
    #[serde(default)]
    pub budget: Budget,
    /// Starred model specs (`provider/model`) — picker favorites, Ctrl+1..5.
    #[serde(default)]
    pub favorite_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    /// Smart Auto starts a new thread on the first ready provider in this
    /// order. A started thread stays on its provider.
    #[serde(default = "default_order", alias = "auto_order")]
    pub order: Vec<String>,
}

fn default_order() -> Vec<String> {
    [
        "claude",
        "codex",
        "opencode",
        "grok",
        "antigravity",
        "cursor",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            order: default_order(),
        }
    }
}

/// R-4: a spend cap the runtime keeps. Both limits are optional and the
/// tighter of config and project (`budget:` in PROJECT.md) wins. A run that
/// hits one pauses (Idle + `budget_exceeded`) — it is never silently cut.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
}

impl Budget {
    /// The tighter of two caps, limit by limit. `None` never tightens.
    #[must_use]
    pub fn tightest(self, other: Self) -> Self {
        Self {
            max_cost_usd: match (self.max_cost_usd, other.max_cost_usd) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
            max_tokens: match (self.max_tokens, other.max_tokens) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
        }
    }

    pub fn is_unlimited(self) -> bool {
        self.max_cost_usd.is_none() && self.max_tokens.is_none()
    }

    /// The reason this run must pause, or `None` while it is inside the cap.
    pub fn exceeded(self, tokens: u64, cost_usd: f64) -> Option<String> {
        if let Some(max) = self.max_tokens {
            if tokens >= max {
                return Some(format!("token budget reached ({tokens}/{max} tokens)"));
            }
        }
        if let Some(max) = self.max_cost_usd {
            if cost_usd >= max {
                return Some(format!("cost budget reached (${cost_usd:.4}/${max:.2})"));
            }
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    /// Off = the picker and Smart Auto leave this provider out.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The vendor program. Empty = its usual name, found on PATH.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub binary: String,
    /// The model a new thread starts on. Empty = the vendor's own default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_model: String,
}

impl Default for ProviderEntry {
    fn default() -> Self {
        Self {
            enabled: true,
            binary: String::new(),
            default_model: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LaneDefaults {
    /// auto | ask | deny
    #[serde(default = "default_mode")]
    pub default_mode: String,
    #[serde(default)]
    pub default_allowed_tools: Vec<String>,
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: HashMap<String, McpServerCfg>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpServerCfg {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Exposure allowlist (short tool names): empty = expose all tools.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Exposure denylist (short tool names): takes precedence over `allow`.
    #[serde(default)]
    pub deny: Vec<String>,
    /// Per-tool approval override: short tool name -> `auto` | `ask` | `deny`.
    /// Absent = follow the lane mode. `deny` blocks execution even when exposed.
    #[serde(default)]
    pub tool_modes: HashMap<String, String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl McpServerCfg {
    /// Is `tool` (short name) exposed by this server at all?
    pub fn is_tool_exposed(&self, tool: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if self.deny.iter().any(|d| d == tool) {
            return false;
        }
        if self.allow.is_empty() {
            return true;
        }
        self.allow.iter().any(|a| a == tool)
    }

    /// Per-tool approval override, normalised. None = follow lane mode.
    pub fn tool_mode(&self, tool: &str) -> Option<String> {
        self.tool_modes.get(tool).map(|m| {
            match m.as_str() {
                "auto" => "auto",
                "deny" => "deny",
                _ => "ask",
            }
            .to_string()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchLimits {
    #[serde(default = "default_concurrent")]
    pub max_concurrent: usize,
    #[serde(default = "default_idle_kill")]
    pub mcp_idle_kill_secs: u64,
    /// When busy: queue the run (true) or reject loudly (false).
    #[serde(default = "default_true")]
    pub queue_when_busy: bool,
}

fn default_mode() -> String {
    "ask".into()
}
fn default_max_steps() -> u32 {
    32
}
fn default_timeout_ms() -> u64 {
    30_000
}
fn default_true() -> bool {
    true
}
fn default_concurrent() -> usize {
    4
}
fn default_idle_kill() -> u64 {
    60
}

impl Default for ParziConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            providers: HashMap::new(),
            lanes: LaneDefaults {
                default_mode: default_mode(),
                default_allowed_tools: vec![],
                max_steps: 32,
            },
            mcp: McpConfig::default(),
            orchestrator: OrchLimits {
                max_concurrent: 4,
                mcp_idle_kill_secs: 60,
                queue_when_busy: true,
            },
            routing: RoutingConfig::default(),
            budget: Budget::default(),
            favorite_models: vec![],
        }
    }
}

impl Default for OrchLimits {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            mcp_idle_kill_secs: 60,
            queue_when_busy: true,
        }
    }
}

impl ParziConfig {
    pub fn load() -> Result<Self> {
        let path = paths::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let mut cfg: Self = toml::from_str(&text)?;
        cfg.check_version()?;
        cfg.migrate();
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::config_path()?;
        atomic_write(&path, toml::to_string(self)?.as_bytes())
    }

    fn check_version(&self) -> Result<()> {
        if self.version != 1 && self.version != CONFIG_VERSION {
            return Err(ParziError::Config(format!(
                "config version {} unsupported (want {}); delete {} to regenerate",
                self.version,
                CONFIG_VERSION,
                paths::config_path()?.display()
            )));
        }
        Ok(())
    }

    /// Bring an older file up to v2. v1 named HTTP adapters and their model
    /// catalog: provider ids fold onto the roster, and model defaults and
    /// favourites are dropped, because vendor programs name models their
    /// own way. v2 files only get their ids folded.
    pub fn migrate(&mut self) {
        let alias = |id: &str| -> Option<&'static str> {
            match id {
                "claude" | "claude-code" | "anthropic" => Some("claude"),
                "codex" | "openai" => Some("codex"),
                "opencode" => Some("opencode"),
                "grok" | "xai" | "grok-cli" => Some("grok"),
                "antigravity" => Some("antigravity"),
                "cursor" => Some("cursor"),
                _ => None,
            }
        };
        let from_v1 = self.version < CONFIG_VERSION;
        let old = std::mem::take(&mut self.providers);
        for (id, mut entry) in old {
            let Some(canon) = alias(&id) else { continue };
            if from_v1 {
                entry = ProviderEntry::default();
            }
            // A canonical entry wins over an alias entry for the same slot.
            if id == canon || !self.providers.contains_key(canon) {
                self.providers.insert(canon.to_string(), entry);
            }
        }
        let mut order: Vec<String> = vec![];
        for p in self.routing.order.iter().filter_map(|p| alias(p)) {
            if !order.iter().any(|x| x == p) {
                order.push(p.to_string());
            }
        }
        for p in default_order() {
            if !order.contains(&p) {
                order.push(p);
            }
        }
        self.routing.order = order;
        if from_v1 {
            self.favorite_models.clear();
        }
        self.version = CONFIG_VERSION;
    }

    /// The entry for a roster id, or the defaults when it has none.
    pub fn provider(&self, id: &str) -> ProviderEntry {
        self.providers.get(id).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_v1_file_moves_to_v2() {
        let toml_text = r#"
version = 1
default_provider = "anthropic"
catalog_refresh = true
favorite_models = ["claude/claude-opus-5"]
[providers.anthropic]
default_model = "claude-sonnet-4"
[providers.xai]
default_model = "grok-4"
base_url = "https://api.x.ai/v1"
[providers.ollama]
default_model = "llama3.1"
[routing]
auto_failover = true
keys_in_auto = false
auto_order = ["antigravity", "codex", "claude-code", "t3", "opencode"]
"#;
        let mut cfg: ParziConfig = toml::from_str(toml_text).unwrap();
        cfg.check_version().unwrap();
        cfg.migrate();
        assert_eq!(cfg.version, CONFIG_VERSION);
        assert!(cfg.providers.contains_key("claude"));
        assert!(cfg.providers.contains_key("grok"));
        assert!(!cfg.providers.contains_key("anthropic"));
        assert!(!cfg.providers.contains_key("ollama"));
        // v1 model ids named the old HTTP catalog: gone, vendor default instead.
        assert_eq!(cfg.provider("claude").default_model, "");
        assert!(cfg.provider("grok").enabled);
        assert!(cfg.favorite_models.is_empty());
        assert_eq!(
            cfg.routing.order,
            vec![
                "antigravity",
                "codex",
                "claude",
                "opencode",
                "grok",
                "cursor"
            ]
        );
        let saved = toml::to_string(&cfg).unwrap();
        for gone in [
            "default_provider",
            "catalog_refresh",
            "auto_failover",
            "keys_in_auto",
            "base_url",
        ] {
            assert!(!saved.contains(gone), "{gone} survived: {saved}");
        }
    }

    #[test]
    fn a_v2_file_keeps_its_choices() {
        let toml_text = r#"
version = 2
favorite_models = ["claude/opus"]
[providers.claude]
default_model = "opus"
binary = "~/bin/claude"
[providers.cursor]
enabled = false
[routing]
order = ["codex", "claude"]
"#;
        let mut cfg: ParziConfig = toml::from_str(toml_text).unwrap();
        cfg.migrate();
        assert_eq!(cfg.provider("claude").default_model, "opus");
        assert_eq!(cfg.provider("claude").binary, "~/bin/claude");
        assert!(!cfg.provider("cursor").enabled);
        assert!(cfg.provider("codex").enabled, "no entry = on");
        assert_eq!(cfg.favorite_models, vec!["claude/opus"]);
        assert_eq!(&cfg.routing.order[..2], ["codex", "claude"]);
    }
}
