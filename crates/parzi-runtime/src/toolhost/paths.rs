//! Where a tool's paths point: what ui.show_file opens, paths relative to
//! the session's folder or worktree, and the paths a permission names.

use std::path::{Path, PathBuf};

use parzi_core::paths::strip_prefix_fold;
use parzi_providers::PermissionRequest;
use serde_json::Value;

use crate::tools::edit_paths;

/// What ui.show_file opens: an absolute path as it is, a path relative to
/// the session's folder when that file exists, and otherwise a brain note by
/// its vault path (the bool).
pub(super) fn show_target(cwd: &str, raw: &str) -> (String, bool) {
    let here = std::path::Path::new(cwd).join(raw);
    if std::path::Path::new(raw).is_absolute() {
        (raw.to_string(), false)
    } else if !cwd.is_empty() && here.is_file() {
        (here.to_string_lossy().into_owned(), false)
    } else {
        (raw.replace('\\', "/"), true)
    }
}

/// An http(s) url on this computer, for ui.host_site: localhost names and
/// loopback IPs only, so the side-bar card cannot embed a page the agent
/// does not control.
pub(super) fn loopback_site(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if !(raw.starts_with("http://") || raw.starts_with("https://")) {
        return None;
    }
    let authority = raw
        .split(['/', '?', '#'])
        .nth(2)?
        .rsplit('@')
        .next()
        .unwrap_or("");
    if authority.is_empty() {
        return None;
    }
    let host = if let Some(inner) = authority.strip_prefix('[') {
        inner.split(']').next()?
    } else if authority.matches(':').count() == 1 {
        authority.rsplit(':').nth(1)?
    } else {
        authority
    };
    let host = host.to_ascii_lowercase();
    let loopback = host == "localhost"
        || host.ends_with(".localhost")
        || host == "::1"
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.octets()[0] == 127);
    loopback.then(|| raw.to_string())
}

pub(super) fn worktree_relative(cwd: &str, path: &str) -> Option<String> {
    let rel = relative_text(cwd, path)?;
    let root = std::fs::canonicalize(cwd).ok()?;
    let walk: Vec<String> = if cfg!(windows) {
        rel.split('/').map(str::to_string).collect()
    } else {
        relative_segments(cwd, path)?
    };
    let mut target = root.clone();
    target.extend(&walk);
    let landed = land(&target)?;
    let out: Vec<String> = landed
        .strip_prefix(&root)
        .ok()?
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!out.is_empty()).then(|| out.join("/"))
}

pub(super) fn land(path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let mut here = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !cfg!(windows) && std::fs::symlink_metadata(&here).is_ok() {
                    here = std::fs::canonicalize(&here).ok()?;
                }
                if !here.pop() {
                    return None;
                }
            }
            c => here.push(c),
        }
    }
    let mut probe = here;
    let mut tail = vec![];
    while std::fs::symlink_metadata(&probe).is_err() {
        tail.push(probe.file_name()?.to_os_string());
        if !probe.pop() {
            return None;
        }
    }
    let mut real = std::fs::canonicalize(&probe).ok()?;
    real.extend(tail.iter().rev());
    Some(real)
}

fn relative_segments(cwd: &str, path: &str) -> Option<Vec<String>> {
    let base = cwd.trim().replace('\\', "/");
    let base = base.trim_end_matches('/');
    if base.is_empty() || path.is_empty() || path.trim() != path || path.starts_with('~') {
        return None;
    }
    let p = path.replace('\\', "/");
    let raw = Path::new(path);
    let rooted = raw.has_root()
        || p.starts_with('/')
        || matches!(
            raw.components().next(),
            Some(std::path::Component::Prefix(_))
        );
    let rel = if rooted {
        strip_prefix_fold(&p, base)?.strip_prefix('/')?
    } else {
        p.as_str()
    };
    let mut segs = vec![];
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => segs.push(seg.to_string()),
            s if cfg!(windows) && !windows_plain_name(s) => return None,
            s => segs.push(s.to_string()),
        }
    }
    Some(segs)
}

pub(super) fn relative_text(cwd: &str, path: &str) -> Option<String> {
    let mut parts: Vec<String> = vec![];
    for seg in relative_segments(cwd, path)? {
        if seg == ".." {
            parts.pop()?;
        } else {
            parts.push(seg);
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn windows_plain_name(seg: &str) -> bool {
    const DEVICES: [&str; 6] = ["con", "prn", "aux", "nul", "conin$", "conout$"];
    let stem = seg
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_lowercase();
    let port = |p: &str| {
        stem.strip_prefix(p).is_some_and(|n| {
            n.chars().count() == 1 && n.chars().all(|c| c.is_ascii_digit() || "¹²³".contains(c))
        })
    };
    !seg.contains(':')
        && !seg.ends_with(['.', ' '])
        && !DEVICES.contains(&stem.as_str())
        && !port("com")
        && !port("lpt")
}

pub(super) fn named_paths(req: &PermissionRequest) -> Vec<String> {
    let mut out = req.paths.clone();
    for k in [
        "file_path",
        "filePath",
        "notebook_path",
        "path",
        "target",
        "source",
        "destination",
        "old_path",
        "new_path",
    ] {
        if let Some(p) = req.input.get(k).and_then(Value::as_str) {
            out.push(p.to_string());
        }
    }
    if let Some(list) = req.input.get("paths").and_then(Value::as_array) {
        out.extend(list.iter().filter_map(Value::as_str).map(str::to_string));
    }
    out
}

pub(super) const SECRET_FILE: &str =
    "that file holds Parzi's or an agent's sign-in tokens, so agents cannot open it";

pub(super) fn write_targets(req: &PermissionRequest) -> Vec<String> {
    if !req.paths.is_empty() {
        return req.paths.clone();
    }
    edit_paths("edit", &req.input)
}
