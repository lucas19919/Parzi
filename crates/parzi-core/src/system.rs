use std::path::PathBuf;

use crate::paths;

pub const SYSTEM_CAP: usize = 8_000;

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

#[must_use]
pub fn global() -> Option<String> {
    let path = paths::parzi_dir().ok()?.join("SYSTEM.md");
    read_capped(path)
}
