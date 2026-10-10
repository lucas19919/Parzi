use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParziError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("toml parse: {}", toml_brief(.0))]
    TomlDe(#[from] toml::de::Error),
    #[error("config: {0}")]
    Config(String),
    #[error("store: {0}")]
    Store(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("provider `{0}`: {1}")]
    Provider(String, String),
    #[error("tool `{0}`: {1}")]
    Tool(String, String),
}

pub type Result<T> = std::result::Result<T, ParziError>;

/// Position and reason only: the full Display quotes the offending line,
/// which in config.toml can be a connector secret.
pub(crate) fn toml_brief(e: &toml::de::Error) -> String {
    let full = e.to_string();
    // A type error can quote the value, and the value may be a key.
    let reason = unquoted(e.message().trim());
    match full
        .lines()
        .next()
        .filter(|l| l.starts_with("TOML parse error at"))
    {
        Some(at) => format!("{at}: {reason}"),
        None => reason,
    }
}

/// Replace every "quoted" or `ticked` stretch with an ellipsis.
fn unquoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut open: Option<char> = None;
    for c in s.chars() {
        match open {
            Some(q) if c == q => open = None,
            Some(_) => {}
            None if c == '"' || c == '`' => {
                open = Some(c);
                out.push('…');
            }
            None => out.push(c),
        }
    }
    out
}

/// Keeps the kind (callers test `NotFound`) and adds which file failed.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn io_at(path: &Path, e: std::io::Error) -> ParziError {
    ParziError::Io(std::io::Error::new(
        e.kind(),
        format!("{}: {e}", path.display()),
    ))
}

/// Replaces `path` in one step: a unique dot-prefixed sibling is written,
/// synced and renamed over it, and removed again on failure. On unix an
/// existing file keeps its permission bits.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic(path, bytes, None)
}

/// `atomic_write` for files that can hold secrets: owner-only (0600) on unix.
pub fn atomic_write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic(path, bytes, Some(0o600))
}

fn write_atomic(path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<()> {
    use std::io::Write;
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| io_at(parent, e))?;
    let name = path
        .file_name()
        .map_or("file".into(), |n| n.to_string_lossy());
    let tmp = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Relaxed)
    ));
    let write = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        keep_mode(&f, path, mode)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(e) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_at(path, e));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_at(path, e));
    }
    Ok(())
}

#[cfg(unix)]
fn keep_mode(f: &std::fs::File, dest: &Path, mode: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let bits = mode.or_else(|| {
        std::fs::metadata(dest)
            .ok()
            .map(|m| m.permissions().mode() & 0o7777)
    });
    match bits {
        Some(bits) => f.set_permissions(std::fs::Permissions::from_mode(bits)),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn keep_mode(_: &std::fs::File, _: &Path, _: Option<u32>) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_errors_never_quote_a_value() {
        let e = toml::from_str::<std::collections::HashMap<String, u32>>("key = \"sk-secret-123\"")
            .unwrap_err();
        let brief = toml_brief(&e);
        assert!(!brief.contains("sk-secret-123"), "{brief}");
        assert!(brief.contains("line 1"), "{brief}");
        assert_eq!(
            unquoted("invalid type: string \"x\", expected `u32`"),
            "invalid type: string …, expected …"
        );
    }

    fn tmp_files(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| Path::new(n).extension().is_some_and(|x| x == "tmp"))
            .collect()
    }

    #[test]
    fn concurrent_writers_never_lose_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let payloads: Vec<String> = (0..8).map(|t| format!("{t}").repeat(4096)).collect();
        std::thread::scope(|s| {
            for p in &payloads {
                let path = &path;
                s.spawn(move || {
                    for _ in 0..40 {
                        atomic_write(path, p.as_bytes()).unwrap();
                        let now = std::fs::read_to_string(path).unwrap();
                        assert_eq!(now.len(), 4096, "a reader saw a torn file");
                    }
                });
            }
        });
        let last = std::fs::read_to_string(&path).unwrap();
        assert!(payloads.contains(&last));
        assert!(
            tmp_files(dir.path()).is_empty(),
            "{:?}",
            tmp_files(dir.path())
        );
    }

    #[test]
    fn a_sibling_tmp_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("theme.tmp"), "mine").unwrap();
        atomic_write(&dir.path().join("theme.toml"), b"x = 1").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("theme.tmp")).unwrap(),
            "mine"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("theme.toml")).unwrap(),
            "x = 1"
        );
    }

    #[test]
    fn a_failed_write_names_the_file_and_leaves_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("taken");
        std::fs::create_dir_all(target.join("inside")).unwrap();
        let err = atomic_write(&target, b"x").unwrap_err().to_string();
        assert!(err.contains("taken"), "{err}");
        assert!(tmp_files(dir.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn unix_modes_survive_a_rewrite() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.toml");
        std::fs::write(&path, "a").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        atomic_write(&path, b"b").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let fresh = dir.path().join("config.toml");
        atomic_write_private(&fresh, b"c").unwrap();
        let mode = std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn toml_errors_keep_the_position_but_not_the_line() {
        let e = toml::from_str::<toml::Value>("a = 1\nsecret = sk-live-123\n").unwrap_err();
        let msg = ParziError::from(e).to_string();
        assert!(msg.contains("line 2"), "{msg}");
        assert!(!msg.contains("sk-live-123"), "{msg}");
    }
}
