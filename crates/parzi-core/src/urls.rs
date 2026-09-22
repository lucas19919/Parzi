//! External URL gate for everything that leaves the app (browser opens,
//! pasted links, model-emitted URLs). No network, no shell: parse, match,
//! and let the caller exec a single argv.

const MAX_LEN: usize = 8192;

/// Owners whose pages Parzi may open. Exact hosts only — never prefixes.
pub const LINK_HOSTS: &[&str] = &["github.com"];
pub const LINK_PATHS: &[&str] = &["/lucas19919/Parzi/", "/parzi/parzi/"];

/// Reject anything a browser or a shell could read as more than one
/// destination. The query set (`? & = % + #`) stays allowed — the issue
/// reporter needs it — because opening never goes through a shell
/// (single argv by contract, see callers). Everything else outside the
/// URL-legal set is refused.
///
/// # Errors
///
/// Rejects empty, overlong, non-https, off-allowlist, and metachar URLs.
pub fn check_external_url(url: &str) -> Result<(), String> {
    if url.is_empty() || url.len() > MAX_LEN {
        return Err("that link isn't allowed".into());
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("that link isn't allowed".into());
    }
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "that link isn't allowed".to_string())?;
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, after) = rest.split_at(host_end);
    let host = host.to_lowercase();
    if host.is_empty()
        || host.contains('@')
        || host.contains(':')
        || host.contains('\\')
        || !LINK_HOSTS.contains(&host.as_str())
    {
        return Err("that link isn't allowed".into());
    }
    let path = if after.starts_with('/') { after } else { "/" };
    if !LINK_PATHS.iter().any(|base| path.starts_with(base)) {
        return Err("that link isn't allowed".into());
    }
    if url.contains([
        '\\', '"', '<', '>', '`', '|', '^', '$', ';', '\'', '(', ')', '*',
    ]) {
        return Err("that link isn't allowed".into());
    }
    Ok(())
}

/// Shape check for user-confirmed opens: any https destination with safe
/// characters. Host scoping lives in `check_external_url`; this one runs
/// only after an explicit per-click confirmation in the UI.
///
/// # Errors
///
/// Rejects empty, overlong, non-https, and metachar URLs.
pub fn check_open_url(url: &str) -> Result<(), String> {
    if url.is_empty() || url.len() > MAX_LEN {
        return Err("that link isn't allowed".into());
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("that link isn't allowed".into());
    }
    if !url.starts_with("https://") {
        return Err("that link isn't allowed".into());
    }
    if url.contains([
        '\\', '"', '<', '>', '`', '|', '^', '$', ';', '\'', '(', ')', '*',
    ]) {
        return Err("that link isn't allowed".into());
    }
    Ok(())
}
