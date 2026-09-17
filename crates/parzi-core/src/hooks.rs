//! Workspace event hooks: user scripts that watch tool calls.
//!
//! ```toml
//! [[pre_tool]]
//! match = "shell.exec"
//! command = "python .parzi/guard.py"
//! timeout_secs = 5
//!
//! [[post_tool]]
//! match = "*"
//! command = "notify-send Parzi done"
//! ```
//!
//! `match` is an exact tool name, a `prefix.*` family, or `*`. `pre_tool`
//! hooks run before the approval gate: exit 0 allows, exit 2 denies (stderr
//! becomes the reason), anything else or a timeout allows with a warning.
//! `post_tool` hooks are notify-only; failures surface as notices, never as
//! tool errors. Global `~/.parzi/hooks.toml` runs first, then the
//! workspace (or legacy project) file. Commands run with the workspace dir
//! as cwd and the event as JSON on stdin.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{lanes, paths, workspace};

/// Seconds a hook may run before it is killed (and, for pre-hooks, treated
/// as an allow-with-warning rather than hanging the run).
pub const HOOK_TIMEOUT_DEFAULT: u64 = 5;
/// Hard cap so a junk drawer of hooks cannot stall every tool call.
pub const HOOK_MAX_ENTRIES: usize = 16;

/// One hook entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDef {
    /// Exact name (`shell.exec`), family prefix (`fs.*`), or `*`.
    #[serde(default)]
    pub r#match: String,
    /// Shell command. `sh -c` on unix, `cmd /C` on Windows.
    #[serde(default)]
    pub command: String,
    /// Kill after this long. Zero or missing = default.
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

    /// Does this hook watch `tool`?
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

/// Both hook points of one file.
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

/// Global hooks: `~/.parzi/hooks.toml`. Missing or unparsable = none.
#[must_use]
pub fn global() -> HookSet {
    paths::parzi_dir()
        .map(|root| read_file(root.join("hooks.toml")))
        .unwrap_or_default()
}

/// Workspace hooks: `workspaces/<ws>/hooks.toml`, else legacy
/// `projects/<name>/hooks.toml`. Anything else = none.
#[must_use]
pub fn workspace_hooks(name: &str) -> HookSet {
    let key = name.trim();
    if key.is_empty() || key == "default" {
        return HookSet::default();
    }
    if workspace::load(key).is_ok() {
        return read_file(workspace::dir(key).join("hooks.toml"));
    }
    if lanes::safe_name(key).is_ok() {
        if let Ok(root) = paths::projects_dir() {
            return read_file(root.join(key).join("hooks.toml"));
        }
    }
    HookSet::default()
}

/// Merged hooks for a scope: global first, then the workspace file.
#[must_use]
pub fn for_scope(project: &str) -> HookSet {
    let global = global();
    let ws = workspace_hooks(project);
    let mut pre_tool = global.pre_tool;
    pre_tool.extend(ws.pre_tool);
    let mut post_tool = global.post_tool;
    post_tool.extend(ws.post_tool);
    HookSet {
        pre_tool,
        post_tool,
    }
    .capped()
}

#[cfg(test)]
mod tests {
    use super::{for_scope, HookDef, HookSet};

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
        // Pure parsing is covered without touching PARZI_HOME.
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
    fn unknown_scopes_resolve_to_no_hooks() {
        assert!(for_scope("").pre_tool.is_empty());
        assert!(for_scope("default").post_tool.is_empty());
        assert!(for_scope("../evil").pre_tool.is_empty());
    }
}
