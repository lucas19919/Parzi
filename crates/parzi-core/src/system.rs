use std::path::PathBuf;

use crate::paths;

pub const SYSTEM_CAP: usize = 8_000;

fn read_capped(path: PathBuf) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    // The cap is in chars; a byte-length check flags short non-ASCII files.
    match text.char_indices().nth(SYSTEM_CAP) {
        None => Some(text.to_string()),
        Some((cut, _)) => Some(format!("{}\n…(truncated)", &text[..cut])),
    }
}

#[must_use]
pub fn global() -> Option<String> {
    let path = paths::parzi_dir().ok()?.join("SYSTEM.md");
    read_capped(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cap_counts_chars_not_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SYSTEM.md");
        let full = "é".repeat(SYSTEM_CAP);
        std::fs::write(&path, &full).unwrap();
        assert_eq!(read_capped(path.clone()).unwrap(), full);
        std::fs::write(&path, format!("{full}ü")).unwrap();
        let cut = read_capped(path).unwrap();
        assert_eq!(cut, format!("{full}\n…(truncated)"));
    }
}
