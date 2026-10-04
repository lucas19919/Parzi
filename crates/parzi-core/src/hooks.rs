use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::paths;

pub const HOOK_TIMEOUT_DEFAULT: u64 = 5;
pub const HOOK_MAX_ENTRIES: usize = 16;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDef {
    #[serde(default)]
    pub r#match: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub timeout_secs: u64,
}

impl HookDef {
    #[must_use]
    pub fn timeout(&self) -> u64 {
        if self.timeout_secs == 0 {
            HOOK_TIMEOUT_DEFAULT
        } else {
            self.timeout_secs.min(120)
        }
    }

    #[must_use]
    pub fn matches(&self, tool: &str) -> bool {
        let m = self.r#match.trim();
        if m.is_empty() || m == "*" {
            return true;
        }
        if let Some(prefix) = m.strip_suffix(".*") {
            return tool == prefix || tool.starts_with(&format!("{prefix}."));
        }
        m == tool
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookSet {
    #[serde(default)]
    pub pre_tool: Vec<HookDef>,
    #[serde(default)]
    pub post_tool: Vec<HookDef>,
}

impl HookSet {
    fn capped(mut self) -> Self {
        self.pre_tool.truncate(HOOK_MAX_ENTRIES);
        self.post_tool.truncate(HOOK_MAX_ENTRIES);
        self.pre_tool.retain(|h| !h.command.trim().is_empty());
        self.post_tool.retain(|h| !h.command.trim().is_empty());
        self
    }
}

fn read_file(path: PathBuf) -> HookSet {
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    if raw.trim().is_empty() {
        return HookSet::default();
    }
    toml::from_str::<HookSet>(&raw).unwrap_or_default().capped()
}

#[must_use]
pub fn global() -> HookSet {
    paths::parzi_dir()
        .map(|root| read_file(root.join("hooks.toml")))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{HookDef, HookSet};

    #[test]
    fn matching_covers_exact_family_and_wildcard() {
        let any = HookDef {
            r#match: "*".into(),
            ..Default::default()
        };
        assert!(any.matches("shell.exec"));
        let fam = HookDef {
            r#match: "fs.*".into(),
            ..Default::default()
        };
        assert!(fam.matches("fs.read"));
        assert!(fam.matches("fs.*"));
        assert!(!fam.matches("shell.exec"));
        let exact = HookDef {
            r#match: "shell.exec".into(),
            ..Default::default()
        };
        assert!(exact.matches("shell.exec"));
        assert!(!exact.matches("shell.exec2"));
        let blank = HookDef::default();
        assert!(blank.matches("anything"));
    }

    #[test]
    fn timeouts_default_and_cap() {
        assert_eq!(HookDef::default().timeout(), super::HOOK_TIMEOUT_DEFAULT);
        assert_eq!(
            HookDef {
                timeout_secs: 9999,
                ..Default::default()
            }
            .timeout(),
            120
        );
    }

    #[test]
    fn empty_and_garbage_configs_are_no_hooks() {
        let set: HookSet = toml::from_str("not = [valid").unwrap_or_default();
        assert!(set.pre_tool.is_empty() && set.post_tool.is_empty());
        let set: HookSet = toml::from_str(
            "[[pre_tool]]\nmatch = \"fs.*\"\ncommand = \"guard\"\n[[post_tool]]\nmatch = \"*\"\ncommand = \"log\"\n",
        )
        .unwrap();
        assert_eq!(set.pre_tool.len(), 1);
        assert_eq!(set.post_tool.len(), 1);
        assert_eq!(set.pre_tool[0].timeout(), super::HOOK_TIMEOUT_DEFAULT);
    }
}
