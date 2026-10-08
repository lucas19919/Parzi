use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::{atomic_write, paths};

pub const CONFIG_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParziConfig {
    pub version: u32,
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
    #[serde(default)]
    pub budget: Budget,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub favorite_models: Vec<String>,
    #[serde(default)]
    pub image: ImageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
}

impl Budget {
    pub fn is_unlimited(self) -> bool {
        self.max_cost_usd.is_none() && self.max_tokens.is_none()
    }

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
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub binary: String,
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
    #[serde(default = "default_mode")]
    pub default_mode: String,
    #[serde(default)]
    pub default_allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: HashMap<String, McpServerCfg>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageConfig {
    #[serde(default = "default_image_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_image_model")]
    pub model: String,
}

fn default_image_endpoint() -> String {
    "https://image.pollinations.ai/prompt/{prompt}?width={width}&height={height}&model={model}&nologo=true".into()
}
fn default_image_model() -> String {
    "flux".into()
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            endpoint: default_image_endpoint(),
            model: default_image_model(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpServerCfg {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default)]
    pub tool_modes: HashMap<String, String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl McpServerCfg {
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
    #[serde(default = "default_true")]
    pub queue_when_busy: bool,
}

fn default_mode() -> String {
    "ask".into()
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
            },
            mcp: McpConfig::default(),
            orchestrator: OrchLimits::default(),
            routing: RoutingConfig::default(),
            budget: Budget::default(),
            favorite_models: vec![],
            image: ImageConfig::default(),
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

    fn migrate(&mut self) {
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
auto_order = ["antigravity", "codex", "claude-code", "ollama", "opencode"]
"#;
        let mut cfg: ParziConfig = toml::from_str(toml_text).unwrap();
        cfg.check_version().unwrap();
        cfg.migrate();
        assert_eq!(cfg.version, CONFIG_VERSION);
        assert!(cfg.providers.contains_key("claude"));
        assert!(cfg.providers.contains_key("grok"));
        assert!(!cfg.providers.contains_key("anthropic"));
        assert!(!cfg.providers.contains_key("ollama"));
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
