//! `GIT_ASKPASS`: how the token reaches git without ever being an argument.
//!
//! A command line is world-readable on every OS Parzi runs on (`ps`,
//! `wmic process get commandline`, Task Manager's "Command line" column), so
//! a PAT in the clone URL is a leak to every other process on the machine.
//! Git's own answer is `GIT_ASKPASS`: a program git runs when it needs a
//! password. Ours is a two-line script in a private temp directory that
//! prints one environment variable — the token is passed to the child
//! process's environment, the script holds nothing, and argv holds nothing.
//!
//! The directory is removed on drop, including when the clone fails.

use std::path::{Path, PathBuf};

/// Environment variable the script echoes. Read by nothing else.
pub const TOKEN_ENV: &str = "PARZI_GIT_TOKEN";

#[cfg(windows)]
const SCRIPT_NAME: &str = "parzi-askpass.bat";
#[cfg(not(windows))]
const SCRIPT_NAME: &str = "parzi-askpass.sh";

/// Delayed expansion on Windows and a quoted `printf` elsewhere: the value is
/// never re-parsed as script text, whatever characters a token contains.
#[cfg(windows)]
const SCRIPT_BODY: &str =
    "@echo off\r\nsetlocal enabledelayedexpansion\r\necho !PARZI_GIT_TOKEN!\r\n";
#[cfg(not(windows))]
const SCRIPT_BODY: &str = "#!/bin/sh\nprintf '%s\\n' \"$PARZI_GIT_TOKEN\"\n";

/// A temp directory holding one askpass script, deleted when this is dropped.
#[derive(Debug)]
pub struct Askpass {
    dir: PathBuf,
    script: PathBuf,
}

impl Askpass {
    /// Write the script into a fresh private directory. Nothing secret is
    /// written: the script only names an environment variable.
    pub fn new() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(unique_name());
        std::fs::create_dir_all(&dir).map_err(|e| format!("askpass dir: {e}"))?;
        let script = dir.join(SCRIPT_NAME);
        std::fs::write(&script, SCRIPT_BODY).map_err(|e| format!("askpass script: {e}"))?;
        restrict(&script)?;
        Ok(Self { dir, script })
    }

    #[must_use]
    pub fn script(&self) -> &Path {
        &self.script
    }
}

impl Drop for Askpass {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.dir) {
            tracing::debug!("askpass cleanup {}: {e}", self.dir.display());
        }
    }
}

/// One directory per clone: pid plus a counter, so two clones at once never
/// share a script (and one finishing never deletes the other's).
fn unique_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!(
        "parzi-askpass-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

/// Owner-only, so no other account on the machine can replace the script git
/// is about to run with the token in its environment.
#[cfg(unix)]
fn restrict(script: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("askpass permissions: {e}"))
}

/// Windows inherits the temp directory's ACL, which is already per-user.
/// The `Result` matches the unix arm, so the caller stays one line.
#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn restrict(_script: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Askpass, SCRIPT_BODY, TOKEN_ENV};

    #[test]
    fn the_script_names_the_variable_and_never_holds_a_token() {
        assert!(SCRIPT_BODY.contains(TOKEN_ENV));
        let ask = Askpass::new().expect("askpass");
        let body = std::fs::read_to_string(ask.script()).expect("read script");
        assert!(!body.contains("ghp_"));
        assert_eq!(body, SCRIPT_BODY);
        let dir = ask.script().parent().expect("dir").to_path_buf();
        drop(ask);
        // The script is gone the moment the clone is over.
        assert!(!dir.exists());
    }

    #[test]
    fn two_clones_get_two_scripts() {
        let a = Askpass::new().expect("a");
        let b = Askpass::new().expect("b");
        assert_ne!(a.script(), b.script());
    }
}
