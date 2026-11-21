use std::path::PathBuf;

use crate::paths;

pub const SYSTEM_CAP: usize = 8_000;

fn read_capped(path: PathBuf) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
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

/// The names Claude reads its repository instructions from.
pub const CLAUDE_FILES: [&str; 3] = ["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"];
/// The name most other agents read theirs from.
pub const AGENTS_FILES: [&str; 1] = ["AGENTS.md"];
const REPO_MAX_CHARS: usize = 40_000;

/// A repository's instruction files named `names`, from the outermost folder
/// down to `cwd`, each headed with its path. `~/.claude/CLAUDE.md` is left
/// out: that is the person's own file, not the repository's.
#[must_use]
pub fn repo_instructions(cwd: &std::path::Path, names: &[&str]) -> Option<String> {
    let home = dirs::home_dir();
    let mut dirs: Vec<&std::path::Path> = cwd.ancestors().collect();
    dirs.reverse();
    let mut out = String::new();
    for dir in dirs {
        for name in names {
            if *name == ".claude/CLAUDE.md" && home.as_deref() == Some(dir) {
                continue;
            }
            let path = dir.join(name);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            out.push_str(&format!(
                "Contents of {} (the repository's instructions):\n\n{}\n\n",
                path.display(),
                text.trim()
            ));
        }
    }
    if out.is_empty() {
        return None;
    }
    if out.chars().count() > REPO_MAX_CHARS {
        out = out.chars().take(REPO_MAX_CHARS).collect();
        out.push_str("\n…(the rest of the instruction files was cut)");
    }
    Some(out.trim_end().to_string())
}

/// Instructions for an agent that reads `AGENTS.md` itself, when the
/// repository only has Claude's files: it would otherwise see none.
#[must_use]
pub fn claude_files_for_others(cwd: &std::path::Path) -> Option<String> {
    if repo_instructions(cwd, &AGENTS_FILES).is_some() {
        return None;
    }
    repo_instructions(cwd, &CLAUDE_FILES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_agent_gets_the_repository_file_it_would_not_read_itself() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("app");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("CLAUDE.md"), "use pnpm").unwrap();
        let theirs = claude_files_for_others(&repo).expect("only CLAUDE.md here");
        assert!(theirs.contains("use pnpm"));
        std::fs::write(repo.join("AGENTS.md"), "use pnpm too").unwrap();
        assert!(
            claude_files_for_others(&repo).is_none(),
            "they read AGENTS.md themselves"
        );
        assert!(repo_instructions(&repo, &AGENTS_FILES)
            .unwrap()
            .contains("use pnpm too"));
    }

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
