//! Clone-or-map: the half of the wizard that puts a repo on this machine.
//!
//! Each machine maps `remote -> local path` for itself (hub PLAN.md §1.1), so
//! mapping an existing checkout is the first-class path and cloning is the
//! fallback. Neither ever leaves a credential in `.git/config`, and neither
//! ever puts one on a command line.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use parzi_core::workspace::{self, RepoRef};

use super::askpass::{self, Askpass};
use super::token;

/// The username git sends with a personal access token. Not a secret: the
/// token itself answers the password prompt, through `GIT_ASKPASS`.
const TOKEN_USER: &str = "x-access-token";

/// `remote` of an existing checkout, `""` when the path is not a git repo.
async fn remote_of(path: &Path) -> String {
    let out = tokio::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(path)
        .output()
        .await;
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

/// `https://github.com/org/repo.git` and `git@github.com:org/repo` are the
/// same remote. Compare host + path, without scheme, user or `.git`.
fn same_remote(a: &str, b: &str) -> bool {
    fn key(s: &str) -> String {
        let s = s.trim().trim_end_matches('/');
        let s = s.strip_suffix(".git").unwrap_or(s);
        let s = s.rsplit('@').next().unwrap_or(s);
        let s = s
            .strip_prefix("https://")
            .or_else(|| s.strip_prefix("http://"))
            .or_else(|| s.strip_prefix("ssh://"))
            .unwrap_or(s);
        s.replace(':', "/").to_lowercase()
    }
    !a.is_empty() && !b.is_empty() && key(a) == key(b)
}

/// Verify a local path is the checkout of `remote`. The map half of
/// clone-or-map: no network, no writes.
pub async fn verify_map(path: &Path, remote: &str) -> Result<(), String> {
    if !path.join(".git").exists() {
        return Err(format!("{} is not a git repository", path.display()));
    }
    let found = remote_of(path).await;
    if found.is_empty() {
        return Err(format!("{} has no `origin` remote", path.display()));
    }
    if !same_remote(&found, remote) {
        return Err(format!(
            "{} points at {found}, not {remote}",
            path.display()
        ));
    }
    Ok(())
}

/// The URL git is given. It carries the *username* only — the token answers
/// the password prompt through `GIT_ASKPASS`, so nothing secret is ever in
/// argv, in `.git/config`, or in a progress line.
fn login_url(remote: &str, authed: bool) -> String {
    match remote.strip_prefix("https://") {
        Some(rest) if authed && !rest.split('/').next().unwrap_or(rest).contains('@') => {
            format!("https://{TOKEN_USER}@{rest}")
        }
        _ => remote.to_string(),
    }
}

/// The exact argv of the clone. A pure function so a test can prove what the
/// command line contains — and what it does not.
fn clone_argv(url: &str, dest: &Path) -> Vec<String> {
    vec![
        // An empty helper resets the list: no credential manager on this
        // machine answers for us, and none stores what the askpass says.
        "-c".into(),
        "credential.helper=".into(),
        "clone".into(),
        "--progress".into(),
        // `--` before the URL: a remote that begins with a dash must never
        // be read as a git option (`--upload-pack=…` is a shell).
        "--".into(),
        url.into(),
        dest.display().to_string(),
    ]
}

/// Keep a token out of anything we emit or log.
fn redact(line: &str, tok: &str) -> String {
    if tok.is_empty() {
        return line.to_string();
    }
    line.replace(tok, "***")
}

/// Where a clone lands: the root the wizard named, else `~/Parzi/<ws>/<repo>`.
fn destination(workspace: &str, repo: &str, dest_root: Option<&str>) -> PathBuf {
    match dest_root.map(str::trim).filter(|r| !r.is_empty()) {
        Some(root) => PathBuf::from(root).join(repo),
        None => home_dir().join("Parzi").join(workspace).join(repo),
    }
}

/// `~` — where a clone lands when the wizard does not say otherwise.
fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// What clone-or-map decided before any network happens: an existing checkout
/// (verified and recorded) or the path a clone will land in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prepared {
    Mapped(PathBuf),
    Clone(PathBuf),
}

/// Map an existing checkout, or say where the clone will go. The shell calls
/// this first so the wizard can show a path before the network is touched.
pub async fn prepare(
    workspace: &str,
    repo: &RepoRef,
    local_path: Option<&str>,
    dest_root: Option<&str>,
) -> Result<Prepared, String> {
    // The name becomes a directory under the destination root: it is a name,
    // never a path.
    let name = repo.name.trim();
    if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
        return Err(format!("`{}` is not a repository name", repo.name));
    }
    match local_path.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            let path = PathBuf::from(p);
            verify_map(&path, &repo.remote).await?;
            workspace::map_repo(workspace, &repo.name, &path).map_err(|e| e.to_string())?;
            Ok(Prepared::Mapped(path))
        }
        None => Ok(Prepared::Clone(destination(
            workspace, &repo.name, dest_root,
        ))),
    }
}

/// Clone into `dest` and record this machine's mapping (§1.1). The background
/// half of clone-or-map; progress lines go to `on_line`, redacted.
pub async fn clone_and_map(
    workspace: &str,
    repo: &RepoRef,
    dest: &Path,
    on_line: impl FnMut(String) + Send,
) -> Result<PathBuf, String> {
    let path = clone(&repo.remote, dest, on_line).await?;
    workspace::map_repo(workspace, &repo.name, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Clone `remote` into `dest`, reporting git's progress lines through
/// `on_line`. The token reaches git through `GIT_ASKPASS` and the child's
/// environment — never argv, never the URL, never the config.
async fn clone(
    remote: &str,
    dest: &Path,
    mut on_line: impl FnMut(String) + Send,
) -> Result<PathBuf, String> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    if dest.exists() && dest.read_dir().is_ok_and(|mut d| d.next().is_some()) {
        return Err(format!(
            "{} already exists and is not empty",
            dest.display()
        ));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tok = token().unwrap_or_default();
    // ssh remotes are left alone — the agent's own key does the talking.
    let authed = !tok.is_empty() && remote.starts_with("https://");
    let ask = if authed { Some(Askpass::new()?) } else { None };

    let mut cmd = tokio::process::Command::new("git");
    cmd.args(clone_argv(&login_url(remote, authed), dest))
        // No credential prompt may ever block a background clone.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(a) = &ask {
        cmd.env("GIT_ASKPASS", a.script())
            .env(askpass::TOKEN_ENV, &tok);
    }
    let mut child = cmd.spawn().map_err(|e| format!("git clone: {e}"))?;
    if let Some(err) = child.stderr.take() {
        let mut lines = BufReader::new(err).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            let l = l.trim().to_string();
            if !l.is_empty() {
                on_line(redact(&l, &tok));
            }
        }
    }
    let st = child.wait().await.map_err(|e| e.to_string())?;
    drop(ask);
    if !st.success() {
        return Err(format!("git clone failed for {remote}"));
    }
    if let Err(e) = clean_remote(dest, remote, &tok).await {
        return Err(discard(dest, e));
    }
    Ok(dest.to_path_buf())
}

/// `origin` must be exactly the clean remote, and we read it back to prove
/// it: a credential left in `.git/config` would be pushed, logged and backed
/// up for the rest of the checkout's life.
async fn clean_remote(dest: &Path, remote: &str, tok: &str) -> Result<(), String> {
    let out = tokio::process::Command::new("git")
        .args(["remote", "set-url", "origin", remote])
        .current_dir(dest)
        .output()
        .await
        .map_err(|e| format!("git remote set-url: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git remote set-url origin failed: {}",
            redact(String::from_utf8_lossy(&out.stderr).trim(), tok)
        ));
    }
    let found = remote_of(dest).await;
    if found.trim() != remote.trim() {
        return Err(format!(
            "origin is {} after the clone, not {remote}",
            redact(&found, tok)
        ));
    }
    Ok(())
}

/// A checkout we cannot prove is credential-free is not a checkout we keep.
fn discard(dest: &Path, why: String) -> String {
    match std::fs::remove_dir_all(dest) {
        Ok(()) => why,
        Err(e) => format!("{why}; {} could not be deleted either: {e}", dest.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::{clone_argv, destination, discard, login_url, redact, same_remote, TOKEN_USER};
    use std::path::{Path, PathBuf};

    #[test]
    fn remotes_match_across_scheme_and_suffix() {
        assert!(same_remote(
            "https://github.com/org/repo.git",
            "git@github.com:org/repo"
        ));
        assert!(same_remote(
            "https://github.com/Org/Repo",
            "https://github.com/org/repo.git"
        ));
        assert!(!same_remote(
            "https://github.com/org/repo.git",
            "https://github.com/org/other.git"
        ));
        // An empty side is never a match: "no remote" must not pass as one.
        assert!(!same_remote("", ""));
    }

    #[test]
    fn the_command_line_carries_a_username_and_never_a_token() {
        let tok = "ghp_secret";
        let url = login_url("https://github.com/org/repo.git", true);
        assert_eq!(url, "https://x-access-token@github.com/org/repo.git");
        let argv = clone_argv(&url, Path::new("/tmp/repo"));
        let line = argv.join(" ");
        assert!(!line.contains(tok), "token in argv: {line}");
        // A password in the URL would show up as `user:secret@host`.
        let after_scheme = url.trim_start_matches("https://");
        assert!(
            !after_scheme.contains(':'),
            "userinfo with a password: {url}"
        );
        // The machine's credential manager neither answers nor stores.
        assert!(argv.windows(2).any(|w| w == ["-c", "credential.helper="]));
        // ssh and unauthenticated https are left exactly as they are.
        assert_eq!(
            login_url("git@github.com:org/repo.git", true),
            "git@github.com:org/repo.git"
        );
        assert_eq!(
            login_url("https://github.com/org/repo.git", false),
            "https://github.com/org/repo.git"
        );
        // A remote that already names a user keeps it.
        assert_eq!(
            login_url("https://me@example.com/org/repo.git", true),
            "https://me@example.com/org/repo.git"
        );
        assert!(TOKEN_USER.is_ascii());
    }

    #[test]
    fn a_progress_line_never_shows_the_token() {
        assert_eq!(
            redact("Cloning into ghp_secret…", "ghp_secret"),
            "Cloning into ***…"
        );
        assert_eq!(redact("Cloning…", ""), "Cloning…");
    }

    #[test]
    fn the_destination_is_the_named_root_or_the_home_tree() {
        assert_eq!(
            destination("acme", "shop-api", Some("  D:/code  ")),
            PathBuf::from("D:/code").join("shop-api")
        );
        let fallback = destination("acme", "shop-api", Some("   "));
        assert!(fallback.ends_with(Path::new("Parzi/acme/shop-api")));
        assert_eq!(destination("s", "r", None), destination("s", "r", Some("")));
    }

    #[test]
    fn a_checkout_that_cannot_be_cleaned_is_deleted() {
        let dir = std::env::temp_dir().join(format!("parzi-discard-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).expect("tree");
        let why = discard(&dir, "origin is dirty".to_string());
        assert_eq!(why, "origin is dirty");
        assert!(!dir.exists(), "the clone outlived its failure");
    }
}
