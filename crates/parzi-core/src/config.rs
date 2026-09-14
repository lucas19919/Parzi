use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::{atomic_write, paths};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParziConfig {
    pub version: u32,
    #[serde(default = "default_provider")]
    pub default_provider: String,
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
    /// Kill-switch for catalog disk-cache refresh (t3code Manifest discipline).
    #[serde(default = "default_true")]
    pub catalog_refresh: bool,
    /// Starred model specs (`provider/model`) — picker favorites, Ctrl+1..5.
    #[serde(default)]
    pub favorite_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    /// Adaptive: an explicit pick hops to the next routable provider on
    /// 429/overload. Strict: halt instead.
    #[serde(default = "default_true")]
    pub auto_failover: bool,
    /// Provider order for Smart Auto and for failover slots. Only signed-in
    /// subscriptions are taken from it (keys need `keys_in_auto`).
    /// Old configs called this `preferred_subscriptions`; both names load.
    #[serde(default = "default_auto_order", alias = "preferred_subscriptions")]
    pub auto_order: Vec<String>,
    /// Let API-key providers join Smart Auto and failover after the
    /// subscriptions. Off by default: keys are explicit-pick only.
    #[serde(default)]
    pub keys_in_auto: bool,
}

fn default_auto_order() -> Vec<String> {
    vec![
        "claude".into(),
        "codex".into(),
        "antigravity".into(),
        "opencode".into(),
    ]
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            auto_failover: true,
            auto_order: default_auto_order(),
            keys_in_auto: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    #[serde(default)]
    pub default_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

fn default_provider() -> String {
    "claude".into()
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

/// Bundled default model per roster provider.
pub const PROVIDER_DEFAULTS: &[(&str, &str)] = &[
    ("claude", "claude-opus-5"),
    ("codex", "gpt-5.5"),
    ("antigravity", "gemini-3.8-flash-medium"),
    ("opencode", "kimi-k3"),
    ("xai", "grok-4"),
];

impl Default for ParziConfig {
    fn default() -> Self {
        let mut providers = HashMap::new();
        for (id, model) in PROVIDER_DEFAULTS {
            providers.insert(
                id.to_string(),
                ProviderEntry {
                    default_model: (*model).into(),
                    base_url: None,
                },
            );
        }
        Self {
            version: CONFIG_VERSION,
            default_provider: default_provider(),
            providers,
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
            catalog_refresh: true,
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
        if self.version != CONFIG_VERSION {
            return Err(ParziError::Config(format!(
                "config version {} unsupported (want {}); delete {} to regenerate",
                self.version,
                CONFIG_VERSION,
                paths::config_path()?.display()
            )));
        }
        Ok(())
    }

    /// In-place upgrades within config v1: fold the retired provider ids
    /// (`anthropic`/`claude-code` → `claude`, `openai` → `codex`,
    /// `grok*` → `xai`), drop entries for providers Parzi no longer routes,
    /// and make sure every roster provider has a default model.
    pub fn migrate(&mut self) {
        let alias = |id: &str| -> Option<&'static str> {
            match id {
                "claude" | "claude-code" | "anthropic" => Some("claude"),
                "codex" | "openai" => Some("codex"),
                "antigravity" => Some("antigravity"),
                "opencode" => Some("opencode"),
                "xai" | "grok" | "grok-cli" => Some("xai"),
                _ => None,
            }
        };
        let old = std::mem::take(&mut self.providers);
        for (id, entry) in old {
            let Some(canon) = alias(&id) else { continue };
            // A canonical entry wins over an alias entry for the same slot.
            if id == canon || !self.providers.contains_key(canon) {
                let mut e = entry;
                if let Some((_, m)) = PROVIDER_DEFAULTS.iter().find(|(p, _)| *p == canon) {
                    // Retired ids for the folded providers get the new default.
                    if id != canon {
                        e.default_model = (*m).into();
                    }
                }
                self.providers.insert(canon.to_string(), e);
            }
        }
        for (id, model) in PROVIDER_DEFAULTS {
            self.providers
                .entry(id.to_string())
                .or_insert(ProviderEntry {
                    default_model: (*model).into(),
                    base_url: None,
                });
        }
        // Retired opencode placeholder (pre-Zen adapter): point at the flagship.
        if let Some(e) = self.providers.get_mut("opencode") {
            if e.default_model == "opencode-default" {
                e.default_model = "kimi-k3".into();
            }
        }
        self.default_provider = alias(&self.default_provider)
            .unwrap_or("claude")
            .to_string();
        let mut order: Vec<String> = vec![];
        for p in self.routing.auto_order.iter().filter_map(|p| alias(p)) {
            if !order.iter().any(|x| x == p) {
                order.push(p.to_string());
            }
        }
        if order.is_empty() {
            order = default_auto_order();
        }
        self.routing.auto_order = order;
        self.favorite_models
            .retain(|f| f.split_once('/').is_some_and(|(p, _)| alias(p).is_some()));
    }

    /// Split `provider/model` router strings. Provider defaults from config.
    pub fn resolve_model(&self, spec: &str) -> (String, String) {
        match spec.split_once('/') {
            Some((p, m)) => (p.to_string(), m.to_string()),
            None => (
                self.default_provider.clone(),
                self.providers
                    .get(&self.default_provider)
                    .map_or_else(|| spec.to_string(), |e| e.default_model.clone()),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_folds_retired_provider_ids() {
        let toml_text = r#"
version = 1
default_provider = "anthropic"
[providers.anthropic]
default_model = "claude-sonnet-4"
[providers.claude-code]
default_model = "sonnet"
[providers.ollama]
default_model = "llama3.1"
[providers.t3]
default_model = "kimi-k2.5"
[routing]
auto_failover = true
preferred_subscriptions = ["antigravity", "codex", "claude-code", "t3", "opencode"]
"#;
        let mut cfg: ParziConfig = toml::from_str(toml_text).unwrap();
        cfg.migrate();
        assert_eq!(cfg.default_provider, "claude");
        assert!(cfg.providers.contains_key("claude"));
        assert!(!cfg.providers.contains_key("anthropic"));
        assert!(!cfg.providers.contains_key("claude-code"));
        assert!(!cfg.providers.contains_key("ollama"));
        assert!(!cfg.providers.contains_key("t3"));
        assert!(cfg.providers.contains_key("xai"));
        assert_eq!(cfg.providers["claude"].default_model, "claude-opus-5");
        assert_eq!(
            cfg.routing.auto_order,
            vec!["antigravity", "codex", "claude", "opencode"]
        );
        assert!(!cfg.routing.keys_in_auto);
    }

    #[test]
    fn default_roster_is_five_providers() {
        let cfg = ParziConfig::default();
        assert_eq!(cfg.providers.len(), 5);
        assert_eq!(cfg.default_provider, "claude");
        assert_eq!(
            cfg.routing.auto_order,
            vec!["claude", "codex", "antigravity", "opencode"]
        );
    }
}
