//! This machine's name among the user's devices. Synced data keys anything
//! machine-specific by it: a project's folder (`folder@<id>` in its note)
//! and the sessions a device mirrors to its server.
//!
//! The id is the host name as a slug plus the OS (`lucas-pc-windows`), so a
//! Linux server under WSL on the same PC still gets its own. It is written
//! to `~/.parzi/device.json` once and kept, so renaming the machine later
//! does not orphan its folders. `PARZI_DEVICE` overrides it (tests).

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::paths;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
}

fn host() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|h| !h.trim().is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|h| h.trim().to_string())
                .filter(|h| !h.is_empty())
        })
        .unwrap_or_else(|| "device".into())
}

/// Lowercase letters, digits and single dashes, at most 48 characters.
#[must_use]
pub fn slug(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_end_matches('-').chars().take(48).collect();
    if out.is_empty() {
        "device".into()
    } else {
        out
    }
}

/// True for an id this module could have made: safe in a frontmatter key
/// and as one path component.
#[must_use]
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn fresh() -> Device {
    let name = host();
    Device {
        id: slug(&format!("{name}-{}", std::env::consts::OS)),
        name,
    }
}

fn file() -> Option<std::path::PathBuf> {
    paths::parzi_dir().ok().map(|d| d.join("device.json"))
}

/// This machine. Reads `device.json`, else derives the same values it
/// would write; never writes (see [`ensure`]).
#[must_use]
pub fn this() -> Device {
    if let Ok(id) = std::env::var("PARZI_DEVICE") {
        if valid_id(&id) {
            return Device {
                name: id.clone(),
                id,
            };
        }
    }
    // Read once per home: a vault scan asks for every note.
    static CACHE: std::sync::Mutex<Option<(Option<std::path::PathBuf>, Device)>> =
        std::sync::Mutex::new(None);
    let path = file();
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((at, d)) = cache.as_ref() {
        if *at == path {
            return d.clone();
        }
    }
    let d = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<Device>(&raw).ok())
        .filter(|d| valid_id(&d.id))
        .unwrap_or_else(fresh);
    *cache = Some((path, d.clone()));
    d
}

#[must_use]
pub fn id() -> String {
    this().id
}

/// Write `device.json` if it is missing, so the id survives a rename.
pub fn ensure() -> Result<Device> {
    let d = this();
    if let Some(path) = file().filter(|p| !p.exists()) {
        crate::atomic_write(&path, serde_json::to_string_pretty(&d)?.as_bytes())?;
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_safe_keys() {
        assert_eq!(slug("LUCAS-PC"), "lucas-pc");
        assert_eq!(slug("Lucas's MacBook Pro"), "lucas-s-macbook-pro");
        assert_eq!(slug("  ---  "), "device");
        assert!(valid_id(&slug("Ünïcode Host!")));
        assert!(!valid_id("../x"));
        assert!(!valid_id("A"));
        assert!(!valid_id(""));
    }
}
