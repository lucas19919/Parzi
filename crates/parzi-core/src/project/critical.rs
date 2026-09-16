//! The `critical:` matcher (PLAN §4): which paths need a human before their
//! lease moves. Kept next to the project because `critical:` is a PROJECT.md
//! field, and used by the lease tools, which never read PROJECT.md themselves.

use super::Project;

/// Is this repo-relative, repo-prefixed path under the project's `critical:`
/// globs? The caller resolves what "path" means; the lease table never reads
/// PROJECT.md itself.
#[must_use]
pub fn is_critical(project: &Project, path: &str) -> bool {
    project.critical.iter().any(|g| glob_match(g, path))
}

/// Minimal glob: `**` spans any number of path segments (including none),
/// `*` any run inside one segment, `?` one character. A pattern with no
/// wildcard also matches everything under it, so `src/payments` covers
/// `src/payments/intent.rs`. No glob crate is in the lockfile and the
/// sandbox does prefix work by hand, so this stays local and tested.
#[must_use]
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pat = normalize_path(pattern);
    let target = normalize_path(path);
    if !pat.contains(['*', '?']) {
        return pat == target || target.starts_with(&format!("{pat}/"));
    }
    let pat_segs: Vec<&str> = pat.split('/').filter(|s| !s.is_empty()).collect();
    let path_segs: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    match_segments(&pat_segs, &path_segs)
}

/// Repo-prefixed, `/`-separated, no leading or trailing slash. Leases and
/// globs must agree on what a path looks like, so both call this.
#[must_use]
pub fn normalize_path(p: &str) -> String {
    p.trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string()
}

fn match_segments(pat: &[&str], segs: &[&str]) -> bool {
    match pat.split_first() {
        None => segs.is_empty(),
        Some((&"**", rest)) => (0..=segs.len()).any(|i| match_segments(rest, &segs[i..])),
        Some((head, rest)) => {
            !segs.is_empty() && match_one(head, segs[0]) && match_segments(rest, &segs[1..])
        }
    }
}

/// One segment against one segment: `*` and `?`, backtracking on `*`.
fn match_one(pat: &str, seg: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let s: Vec<char> = seg.chars().collect();
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}
