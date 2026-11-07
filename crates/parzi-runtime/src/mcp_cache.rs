//! What each connector offers, kept on disk so a session lists a connector's
//! tools without waiting for it to start. An entry is keyed by a hash of the
//! connector's name and the settings that decide what it runs, so changing the
//! command, address or keys starts it over.

use std::path::PathBuf;

use parzi_core::config::McpServerCfg;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::mcp::McpTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Saved {
    pub server: String,
    pub saved_ms: u64,
    pub tools: Vec<McpTool>,
}

/// The cache key: the settings that change what the connector runs. Its
/// per-tool switches are left out; they filter the list, not make it.
#[must_use]
pub fn key(name: &str, cfg: &McpServerCfg) -> String {
    fn sorted(m: &std::collections::HashMap<String, String>) -> Vec<(&String, &String)> {
        let mut v: Vec<(&String, &String)> = m.iter().collect();
        v.sort();
        v
    }
    let what = serde_json::json!({
        "name": name,
        "command": cfg.command,
        "args": cfg.args,
        "env": sorted(&cfg.env),
        "url": cfg.url,
        "headers": sorted(&cfg.headers),
        "auth": cfg.auth,
    });
    let digest = Sha256::digest(what.to_string().as_bytes());
    digest[..12].iter().map(|b| format!("{b:02x}")).collect()
}

fn path(key: &str) -> Option<PathBuf> {
    Some(
        parzi_core::paths::cache_dir()
            .ok()?
            .join("connectors")
            .join(format!("{key}.json")),
    )
}

#[must_use]
pub fn load(name: &str, cfg: &McpServerCfg) -> Option<Saved> {
    let text = std::fs::read_to_string(path(&key(name, cfg))?).ok()?;
    let saved: Saved = serde_json::from_str(&text).ok()?;
    (saved.server == name).then_some(saved)
}

pub fn save(name: &str, cfg: &McpServerCfg, tools: &[McpTool]) -> Option<Saved> {
    let saved = Saved {
        server: name.to_string(),
        saved_ms: parzi_core::now_ms(),
        tools: tools.to_vec(),
    };
    let Some(file) = path(&key(name, cfg)) else {
        return Some(saved);
    };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_vec(&saved) {
        Ok(bytes) => {
            if let Err(e) = parzi_core::error::atomic_write(&file, &bytes) {
                tracing::debug!(server = name, "connector tool list not saved: {e}");
            }
        }
        Err(e) => tracing::debug!(server = name, "connector tool list not saved: {e}"),
    }
    Some(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_follows_what_runs_not_the_tool_switches() {
        let base = McpServerCfg {
            command: "uvx".into(),
            args: vec!["a".into()],
            enabled: true,
            ..Default::default()
        };
        let mut switched = base.clone();
        switched.deny = vec!["x".into()];
        switched.tool_modes.insert("y".into(), "auto".into());
        switched.timeout_ms = 1;
        assert_eq!(key("a", &base), key("a", &switched));
        let mut moved = base.clone();
        moved.args = vec!["b".into()];
        assert_ne!(key("a", &base), key("a", &moved));
        assert_ne!(key("a", &base), key("b", &base));
        assert_eq!(key("a", &base).len(), 24);
    }
}
