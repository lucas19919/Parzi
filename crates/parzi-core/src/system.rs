//! Standing instructions, Claude-style: `SYSTEM.md` files at three scopes.
//!
//! ```text
//! ~/.parzi/SYSTEM.md                  user scope: every chat, every project
//! ~/.parzi/workspaces/<ws>/SYSTEM.md  workspace scope: chats in that workspace
//! ```
//!
//! (Legacy `projects/<name>/SYSTEM.md` and lane `SYSTEM.md` already flow
//! through `lanes::scan_projects` into the chat prompt; deck projects carry
//! their goal in `PROJECT.md`. This module covers the two scopes that had no
//! reader: global and hub-workspace.)
//!
//! Files are plain markdown, auto-loaded into context, capped. A missing file
//! is normal — most scopes have none — never an error.

use std::path::PathBuf;

use crate::{lanes, paths, workspace};

/// Bytes kept per scope. Standing instructions should read like CLAUDE.md:
/// short. Anything longer is capped with a note, not silently swallowed.
pub const SYSTEM_CAP: usize = 8_000;

/// One scope's instructions, ready to render under a heading.
pub struct ScopedSystem {
    /// `user` or the workspace name. Rendered, never interpolated into a path.
    pub scope: String,
    pub text: String,
}

fn read_capped(path: PathBuf) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let text = raw.trim().to_string();
    if text.is_empty() {
        return None;
    }
    if text.len() <= SYSTEM_CAP {
        return Some(text);
    }
    let cut: String = text.chars().take(SYSTEM_CAP).collect();
    Some(format!("{cut}\n…(truncated)"))
}

/// `~/.parzi/SYSTEM.md`: you, everywhere.
#[must_use]
pub fn global() -> Option<String> {
    let path = paths::parzi_dir().ok()?.join("SYSTEM.md");
    read_capped(path)
}

/// `workspaces/<ws>/SYSTEM.md`: everyone working in that workspace.
/// Anything but a clean hub name is refused, never sanitised into a path.
#[must_use]
pub fn workspace(name: &str) -> Option<String> {
    let clean = lanes::safe_name(name).ok()?;
    read_capped(workspace::dir(&clean).join("SYSTEM.md"))
}

/// Ordered chain for a normal chat: global first, then the workspace file
/// when the chat key names a hub workspace. `default` (the inbox) gets the
/// global file only. Legacy project/lane `SYSTEM.md` files are handled
/// separately by the `scan_projects` pass in the prompt builder.
#[must_use]
pub fn for_chat(project: &str) -> Vec<ScopedSystem> {
    let mut out = Vec::new();
    if let Some(text) = global() {
        out.push(ScopedSystem {
            scope: "user".into(),
            text,
        });
    }
    if !project.trim().is_empty() && project != "default" && workspace::load(project).is_ok() {
        if let Some(text) = workspace(project) {
            out.push(ScopedSystem {
                scope: project.trim().into(),
                text,
            });
        }
    }
    out
}

/// Ordered chain for a deck role run: global, then its workspace file.
#[must_use]
pub fn for_role(workspace_name: &str) -> Vec<ScopedSystem> {
    let mut out = Vec::new();
    if let Some(text) = global() {
        out.push(ScopedSystem {
            scope: "user".into(),
            text,
        });
    }
    if !workspace_name.trim().is_empty() {
        if let Some(text) = workspace(workspace_name) {
            out.push(ScopedSystem {
                scope: workspace_name.trim().into(),
                text,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{for_chat, for_role, global, workspace, SYSTEM_CAP};

    /// One hermetic home for the whole file: PARZI_HOME is process-global,
    /// so a single test owns it rather than racing per-test overrides.
    fn home() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("PARZI_HOME", dir.path());
        dir
    }

    #[test]
    fn chains_stack_user_then_workspace_and_cap_missing() {
        let _home = home();
        // Nothing on disk: empty chains, no errors.
        assert!(global().is_none());
        assert!(for_chat("default").is_empty());
        assert!(for_role("acme").is_empty());

        std::fs::write(
            crate::paths::parzi_dir().unwrap().join("SYSTEM.md"),
            "Be direct.\n",
        )
        .unwrap();
        // A real hub workspace, so the chat key counts as one.
        crate::workspace::create_for("acme", crate::workspace::Kind::Solo, "tester").unwrap();
        let ws = crate::workspace::dir("acme");
        std::fs::write(ws.join("SYSTEM.md"), "Runner: bun, never npm.\n").unwrap();

        let chat = for_chat("acme");
        assert_eq!(chat.len(), 2);
        assert_eq!(chat[0].scope, "user");
        assert!(chat[0].text.contains("Be direct"));
        assert_eq!(chat[1].scope, "acme");
        assert!(chat[1].text.contains("bun"));
        // Inbox and unknown keys get the global file only.
        assert_eq!(for_chat("default").len(), 1);
        assert_eq!(for_chat("nope").len(), 1);
        // Unsafe names never become paths.
        assert!(workspace("../evil").is_none());

        let role = for_role("acme");
        assert_eq!(role.len(), 2);
        assert_eq!(role[1].scope, "acme");

        // Long files cap with a note instead of flooding context.
        let big = "x".repeat(SYSTEM_CAP + 100);
        std::fs::write(ws.join("SYSTEM.md"), &big).unwrap();
        let capped = for_chat("acme");
        assert_eq!(capped.len(), 2);
        assert!(capped[1].text.ends_with("…(truncated)"));
        assert!(capped[1].text.len() < big.len());
        // Blank files count as missing.
        std::fs::write(ws.join("SYSTEM.md"), "   \n").unwrap();
        assert_eq!(for_chat("acme").len(), 1);
    }
}
