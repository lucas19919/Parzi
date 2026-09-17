//! Path-scoped rules, Claude-style: `<workspace>/rules/*.md` files whose
//! frontmatter names the paths they govern. Attaching a file whose path
//! matches pulls the rule body into that run's context — domain knowledge
//! that loads only when it is relevant, instead of a global wall of text.
//!
//! ```markdown
//! ---
//! paths:
//!   - "apps/web/**"
//!   - "*.tsx"
//! ---
//! Use the shared Button; never raw <button>.
//! ```
//!
//! Matching reuses the lease glob (`**` spans segments, `*` stays inside
//! one, `?` is one character). A pattern without a `/` also matches the bare
//! file name, so `*.tsx` covers `src/app.tsx`. A rule with no `paths:` (or an
//! empty one) always applies — explicit file, explicit intent. Bodies cap at
//! 8 KB, directories cap at 32 files, sorted by file name for determinism.
//!
//! Rules live in hub workspaces (`workspaces/<ws>/rules/`) and legacy
//! projects (`projects/<name>/rules/`). Deck slugs have no rules dir: role
//! sessions get PROJECT.md instead, and coders stay task-scoped.

use std::path::PathBuf;

use crate::project::{glob_match, normalize_path};
use crate::{lanes, paths, workspace};

/// Bytes kept per rule body.
pub const RULE_CAP: usize = 8_000;
/// Rules read per directory. Past this the directory is a junk drawer, not
/// configuration — first 32 by file name win, and the count is reported.
pub const RULE_MAX_FILES: usize = 32;

/// One parsed rule file.
pub struct Rule {
    /// File name, for headings and diagnostics.
    pub name: String,
    /// Glob list from frontmatter. Empty = always applies.
    pub paths: Vec<String>,
    pub body: String,
}

/// Does this rule govern `path` (an attachment path, any separators)?
#[must_use]
pub fn governs(rule: &Rule, path: &str) -> bool {
    if rule.paths.is_empty() {
        return true;
    }
    let target = normalize_path(path);
    let base = target.rsplit('/').next().unwrap_or(&target).to_string();
    rule.paths.iter().any(|g| {
        let g = g.trim();
        if g.is_empty() {
            return false;
        }
        glob_match(g, &target) || (!g.contains('/') && glob_match(g, &base))
    })
}

/// Rules of one directory: every `*.md` file, sorted, parsed, capped.
fn read_dir(dir: PathBuf) -> Vec<Rule> {
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.to_lowercase().ends_with(".md"))
        .collect();
    names.sort();
    names
        .into_iter()
        .take(RULE_MAX_FILES)
        .filter_map(|n| {
            let raw = std::fs::read_to_string(dir_join(&dir, &n)).ok()?;
            Some(parse(&n, &raw))
        })
        .collect()
}

/// Join a directory with a name read back from it: the name came from
/// `read_dir`, so it cannot escape — but belt-and-braces anyway.
fn dir_join(dir: &std::path::Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.starts_with(dir) {
        return dir.join("_invalid_");
    }
    p
}

/// Split `---` frontmatter (`paths:` list only — a deliberate subset, not
/// YAML) from the body. No frontmatter = whole file is the body, always
/// applied.
fn parse(name: &str, raw: &str) -> Rule {
    let mut paths = Vec::new();
    let mut body = raw;
    let mut lines = raw.lines();
    if lines.next().is_some_and(|l| l.trim() == "---") {
        let mut in_paths = false;
        let mut closed = false;
        for line in lines {
            if line.trim() == "---" {
                closed = true;
                break;
            }
            let t = line.trim();
            if t.starts_with("paths:") {
                in_paths = true;
                continue;
            }
            if in_paths {
                if let Some(item) = t.strip_prefix("- ") {
                    let item = item.trim().trim_matches('"').trim_matches('\'').trim();
                    if !item.is_empty() {
                        paths.push(item.to_string());
                    }
                    continue;
                }
                // A non-list line ends the paths block; anything else in the
                // frontmatter is ignored, never an error.
                if !t.is_empty() && !t.starts_with('#') {
                    in_paths = false;
                }
            }
        }
        if closed {
            // Body is everything after the closing fence; recompute from the
            // raw text so no line goes missing.
            if let Some(idx) = raw.find("---") {
                let after_first = &raw[idx + 3..];
                if let Some(end) = after_first.find("---") {
                    body = after_first[end + 3..].trim_start_matches(['\n', '\r']);
                }
            }
        } else {
            // Unclosed fence: the whole file is body, frontmatter ignored.
            paths.clear();
            body = raw;
        }
    }
    let body = body.trim().to_string();
    let body = if body.len() <= RULE_CAP {
        body
    } else {
        let cut: String = body.chars().take(RULE_CAP).collect();
        format!("{cut}\n…(truncated)")
    };
    Rule {
        name: name.to_string(),
        paths,
        body,
    }
}

/// Rules governing `files` in the scope `project` names: hub workspace
/// `rules/` when it is one, else legacy project `rules/`. Empty when neither
/// exists. Sorted by file name; bodies already capped.
#[must_use]
pub fn matching(project: &str, files: &[&str]) -> Vec<Rule> {
    let key = project.trim();
    if key.is_empty() || key == "default" {
        return vec![];
    }
    let dir = if workspace::load(key).is_ok() {
        workspace::dir(key).join("rules")
    } else if lanes::safe_name(key).is_ok() {
        paths::projects_dir()
            .map(|root| root.join(key).join("rules"))
            .unwrap_or_else(|_| PathBuf::from("rules"))
    } else {
        return vec![];
    };
    read_dir(dir)
        .into_iter()
        .filter(|r| !r.body.is_empty() && files.iter().any(|f| governs(r, f)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{governs, parse};

    #[test]
    fn frontmatter_parses_and_governs_paths() {
        // Frontmatter subset: paths list, comments and unknown keys ignored.
        let r = parse(
            "fe.md",
            "---\n# shared config\npaths:\n  - \"apps/web/**\"\n  - '*.tsx'\n---\nUse the shared Button.\n",
        );
        assert_eq!(r.paths, vec!["apps/web/**", "*.tsx"]);
        assert_eq!(r.body, "Use the shared Button.");
        assert!(governs(&r, "apps/web/src/app.tsx"));
        assert!(governs(&r, "other/app.tsx"));
        assert!(!governs(&r, "apps/api/main.rs"));
        // No frontmatter: whole file is an always-apply body.
        let plain = parse("all.md", "Be kind.\n");
        assert!(plain.paths.is_empty());
        assert!(governs(&plain, "anything/at/all.rs"));
        // Unclosed fence: frontmatter ignored, body kept.
        let broken = parse("b.md", "---\npaths:\n  - x\nBody here.");
        assert!(broken.paths.is_empty());
        assert_eq!(broken.body, "---\npaths:\n  - x\nBody here.");
    }
}
