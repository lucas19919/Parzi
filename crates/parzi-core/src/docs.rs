use std::path::Path;

const SKIP: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".venv",
    "__pycache__",
];

const FIRST: &[&str] = &[
    "PLAN.md",
    "README.md",
    "AGENTS.md",
    "CLAUDE.md",
    "PROGRESS.md",
    "TODO.md",
];

/// Markdown files under a workspace root (depth ≤ 2, well-known names
/// first). Pure scan: no config, no writes, no filtering by trust.
pub fn scan_root(root: &Path) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = vec![];
    let mut stack = vec![(root.to_path_buf(), 0u8)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 2 || found.len() >= 80 {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                stack.push((p, depth + 1));
            } else if name.to_lowercase().ends_with(".md") {
                if let Ok(rel) = p.strip_prefix(root) {
                    let label = rel.to_string_lossy().replace('\\', "/");
                    found.push((label, p.to_string_lossy().to_string()));
                }
            }
        }
    }
    found.sort_by(|a, b| {
        let ra = FIRST.iter().position(|f| *f == a.0).unwrap_or(FIRST.len());
        let rb = FIRST.iter().position(|f| *f == b.0).unwrap_or(FIRST.len());
        ra.cmp(&rb)
            .then_with(|| a.0.matches('/').count().cmp(&b.0.matches('/').count()))
            .then_with(|| a.0.cmp(&b.0))
    });
    found
}
