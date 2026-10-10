use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{io_at, toml_brief, ParziError, Result};
use crate::paths;

const HOOK_TIMEOUT_DEFAULT: u64 = 5;
const HOOK_MAX_ENTRIES: usize = 16;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDef {
    #[serde(default)]
    pub r#match: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    timeout_secs: u64,
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
        // Blank entries first, so they never use up one of the 16 slots.
        self.pre_tool.retain(|h| !h.command.trim().is_empty());
        self.post_tool.retain(|h| !h.command.trim().is_empty());
        self.pre_tool.truncate(HOOK_MAX_ENTRIES);
        self.post_tool.truncate(HOOK_MAX_ENTRIES);
        self
    }
}

fn read_file(path: &Path) -> Result<HookSet> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HookSet::default()),
        Err(e) => return Err(io_at(path, e)),
    };
    if raw.trim().is_empty() {
        return Ok(HookSet::default());
    }
    toml::from_str::<HookSet>(&raw)
        .map(HookSet::capped)
        .map_err(|e| ParziError::Config(format!("hooks.toml: {}", toml_brief(&e))))
}

/// The hooks in `~/.parzi/hooks.toml`, or why that file is unusable, for
/// doctor to show. A missing or blank file is no hooks, not an error.
pub fn load_global() -> Result<HookSet> {
    read_file(&paths::parzi_dir()?.join("hooks.toml"))
}

/// Like `load_global`, but a broken file runs with no hooks and a warning.
#[must_use]
pub fn global() -> HookSet {
    load_global().unwrap_or_else(|e| {
        tracing::warn!("hooks disabled: {e}");
        HookSet::default()
    })
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

    #[test]
    fn a_broken_file_is_an_error_without_its_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.toml");
        assert_eq!(super::read_file(&path).unwrap(), HookSet::default());
        std::fs::write(&path, "[[pre_tool]]\ncommand = token-abc123\n").unwrap();
        let err = super::read_file(&path).unwrap_err().to_string();
        assert!(
            err.contains("hooks.toml") && err.contains("line 2"),
            "{err}"
        );
        assert!(!err.contains("token-abc123"), "{err}");
    }

    #[test]
    fn blank_entries_do_not_use_up_the_cap() {
        let mut raw = "[[pre_tool]]\ncommand = \"\"\n".repeat(20);
        raw.push_str("[[pre_tool]]\ncommand = \"guard\"\n");
        let set = toml::from_str::<HookSet>(&raw).unwrap().capped();
        assert_eq!(set.pre_tool.len(), 1);
        assert_eq!(set.pre_tool[0].command, "guard");
    }
}
