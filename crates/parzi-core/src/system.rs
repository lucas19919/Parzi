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
