use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use parzi_core::store::SessionStore;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tokio::sync::oneshot;

use crate::AppState;

static GRANTED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
// Session folders change rarely but @-mentions ask on every keystroke.
static SESSION_ROOTS: Mutex<Option<(Instant, Vec<PathBuf>)>> = Mutex::new(None);
const ROOTS_TTL: Duration = Duration::from_secs(5);
const OUTSIDE: &str = "that location is outside the workspace, choose the folder first";

async fn off_thread<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

/// `\\host\share`, `//host`, `\\?\`, `\\.\` and `\??\` reach the network or
/// raw devices. Merely resolving one makes Windows authenticate to the host,
/// so these are refused on the raw text, before any filesystem call.
pub(crate) fn remote_or_device(raw: &str) -> bool {
    let b = raw.trim().as_bytes();
    let sep = |c: &u8| *c == b'\\' || *c == b'/';
    let unc = b.len() >= 2 && sep(&b[0]) && sep(&b[1]);
    let nt = b.len() >= 4 && sep(&b[0]) && b[1] == b'?' && b[2] == b'?' && sep(&b[3]);
    unc || nt
}

/// A drive-letter absolute path (`C:\...`) on Windows, `/...` elsewhere.
/// Alternate data streams (`name:stream`) are never plain files.
#[cfg(windows)]
pub(crate) fn plain_local(p: &Path) -> bool {
    use std::path::{Component, Prefix};
    if remote_or_device(&p.to_string_lossy()) {
        return false;
    }
    let mut parts = p.components();
    let disk = matches!(
        parts.next(),
        Some(Component::Prefix(pre)) if matches!(pre.kind(), Prefix::Disk(_))
    );
    disk && matches!(parts.next(), Some(Component::RootDir))
        && parts.all(|c| !c.as_os_str().to_string_lossy().contains(':'))
}

#[cfg(not(windows))]
pub(crate) fn plain_local(p: &Path) -> bool {
    !remote_or_device(&p.to_string_lossy()) && p.is_absolute()
}

fn local_abs(raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty path".into());
    }
    if remote_or_device(raw) {
        return Err("network and device paths are not opened".into());
    }
    let p = PathBuf::from(raw);
    if plain_local(&p) {
        Ok(p)
    } else {
        Err("path must be absolute".into())
    }
}

fn grant_folder(path: &Path) {
    if !plain_local(path) {
        return;
    }
    let mut roots = vec![];
    push_root(&mut roots, normalize_lexical(path));
    if let Ok(mut g) = GRANTED.lock() {
        for r in roots {
            if !g.contains(&r) {
                g.push(r);
            }
        }
    }
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle, start: Option<String>) -> Result<Option<String>, String> {
    let mut builder = app.dialog().file().set_title("Choose a folder");
    // A network start folder would be browsed (and authenticated to) on open.
    if let Some(dir) = start.filter(|s| plain_local(Path::new(s.trim()))) {
        builder = builder.set_directory(dir.trim());
    }
    let (tx, rx) = oneshot::channel();
    builder.pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx
        .await
        .map_err(|_| "the folder dialog closed".to_string())?
    else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    let granted = path.clone();
    off_thread(move || {
        grant_folder(&granted);
        Ok(())
    })
    .await?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Both the given and the canonical form: the textual pre-check compares the
/// former, the post-canonicalize check the latter.
fn push_root(roots: &mut Vec<PathBuf>, p: PathBuf) {
    if let Ok(c) = p.canonicalize() {
        if c != p {
            roots.push(c);
        }
    }
    roots.push(p);
}

fn scan_session_roots(store: &SessionStore) -> Vec<PathBuf> {
    let mut roots = vec![];
    if let Ok(home) = parzi_core::paths::parzi_dir() {
        if plain_local(&home) {
            push_root(&mut roots, normalize_lexical(&home));
        }
    }
    let cwds: std::collections::BTreeSet<String> = store
        .list()
        .unwrap_or_default()
        .into_iter()
        .map(|m| m.cwd.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    for cwd in cwds {
        let raw = Path::new(&cwd);
        if !plain_local(raw) {
            continue;
        }
        let p = normalize_lexical(raw);
        if p.parent().is_none() {
            continue;
        }
        push_root(&mut roots, p);
    }
    roots
}

fn file_roots(store: &SessionStore, fresh: bool) -> Vec<PathBuf> {
    let mut roots = GRANTED.lock().map(|g| g.clone()).unwrap_or_default();
    let mut cache = SESSION_ROOTS.lock().unwrap_or_else(PoisonError::into_inner);
    match cache.as_ref() {
        Some((at, cached)) if !fresh && at.elapsed() < ROOTS_TTL => roots.extend_from_slice(cached),
        _ => {
            let scanned = scan_session_roots(store);
            roots.extend_from_slice(&scanned);
            *cache = Some((Instant::now(), scanned));
        }
    }
    roots
}

fn normalize_lexical(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(std::path::MAIN_SEPARATOR.to_string());
    }
    out
}

fn contained_in(target: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| target.starts_with(r))
}

/// `\\?\C:\x` as `C:\x`, so canonical roots compare against typed paths.
fn simplified(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut parts = p.components();
        if let Some(Component::Prefix(pre)) = parts.next() {
            if let Prefix::VerbatimDisk(d) = pre.kind() {
                let mut out = PathBuf::from(format!("{}:\\", char::from(d)));
                out.extend(parts.filter(|c| !matches!(c, Component::RootDir)));
                return out;
            }
        }
    }
    p.to_path_buf()
}

/// Textual containment: decided before the target is touched at all.
fn lexically_inside(target: &Path, roots: &[PathBuf]) -> bool {
    let key = |p: &Path| -> Vec<String> {
        p.components()
            .map(|c| {
                let s = c.as_os_str().to_string_lossy();
                if cfg!(windows) {
                    s.to_lowercase()
                } else {
                    s.into_owned()
                }
            })
            .collect()
    };
    let t = key(target);
    roots.iter().any(|r| {
        let r = key(&simplified(r));
        t.len() >= r.len() && t[..r.len()] == r[..]
    })
}

/// The lexical path plus the canonical form of its deepest existing
/// ancestor. Nothing is resolved until the text alone sits inside a root.
fn confine(store: &SessionStore, raw: &str) -> Result<(PathBuf, PathBuf), String> {
    let normal = normalize_lexical(&local_abs(raw)?);
    let mut roots = file_roots(store, false);
    if !lexically_inside(&normal, &roots) {
        roots = file_roots(store, true);
        if !lexically_inside(&normal, &roots) {
            return Err(OUTSIDE.into());
        }
    }
    let canon = normal
        .ancestors()
        .find_map(|q| q.canonicalize().ok())
        .ok_or_else(|| "cannot resolve path".to_string())?;
    if contained_in(&canon, &roots) {
        Ok((normal, canon))
    } else {
        Err(OUTSIDE.into())
    }
}

fn confined_path(store: &SessionStore, raw: &str) -> Result<PathBuf, String> {
    confine(store, raw).map(|(normal, _)| normal)
}

#[tauri::command]
pub async fn git_branch(state: State<'_, AppState>, cwd: String) -> Result<String, String> {
    if cwd.is_empty() {
        return Ok(String::new());
    }
    let orch = state.orch.clone();
    let cwd = off_thread(move || confined_path(orch.store(), &cwd)).await?;
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&cwd)
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
    let out = cmd.output().await.map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[tauri::command]
pub async fn list_files(
    state: State<'_, AppState>,
    root: String,
    query: String,
) -> Result<Vec<String>, String> {
    const SKIP: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        "dist",
        ".venv",
        "__pycache__",
    ];
    if root.is_empty() {
        return Ok(vec![]);
    }
    let orch = state.orch.clone();
    let q = query.to_lowercase();
    // FS walks run on the blocking pool so per-keystroke @-mentions
    // never stall the 3 async workers that serve all other commands.
    off_thread(move || {
        let root_path = confined_path(orch.store(), &root)?;
        let mut out = vec![];
        let mut stack = vec![(root_path.clone(), 0u8)];
        let mut visited = 0usize;
        while let Some((dir, depth)) = stack.pop() {
            if depth > 4 || out.len() >= 200 || visited >= 5000 {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                visited += 1;
                if visited >= 5000 || out.len() >= 200 {
                    break;
                }
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                    continue;
                }
                let Ok(kind) = e.file_type() else {
                    continue;
                };
                if kind.is_symlink() {
                    continue;
                }
                let p = e.path();
                if kind.is_dir() {
                    stack.push((p, depth + 1));
                } else if let Ok(rel) = p.strip_prefix(&root_path) {
                    let s = rel.to_string_lossy().replace('\\', "/");
                    if q.is_empty() || s.to_lowercase().contains(&q) {
                        out.push(s);
                    }
                    if out.len() >= 200 {
                        break;
                    }
                }
            }
        }
        out.sort();
        Ok(out)
    })
    .await
}

const READ_IMAGE_MAX: u64 = 8 * 1024 * 1024;

#[tauri::command]
pub async fn read_image_data_url(
    state: State<'_, AppState>,
    path: String,
    cwd: String,
) -> Result<String, String> {
    let abs = if Path::new(&path).is_absolute() || remote_or_device(&path) {
        path
    } else if cwd.trim().is_empty() {
        return Err("no workspace folder for a relative image path".into());
    } else {
        format!(
            "{}/{}",
            cwd.trim().trim_end_matches(['/', '\\']),
            path.trim().trim_start_matches(['/', '\\'])
        )
    };
    let orch = state.orch.clone();
    // Confinement, the read and the decode/resize all block.
    off_thread(move || {
        let p = confined_path(orch.store(), &abs)?;
        let meta = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        if !meta.is_file() {
            return Err(format!("{} is not a file", p.display()));
        }
        if meta.len() > READ_IMAGE_MAX {
            return Err(format!(
                "{} is {} KiB, over the {} KiB image limit",
                p.display(),
                meta.len() / 1024,
                READ_IMAGE_MAX / 1024
            ));
        }
        let img = parzi_core::context::encode_image_file(&p)
            .ok_or_else(|| format!("{} is not a supported image", p.display()))?;
        Ok(img.data_url())
    })
    .await
}

#[tauri::command]
pub async fn stage_image(name: String, base64_data: String) -> Result<String, String> {
    off_thread(move || stage_image_blocking(&name, &base64_data)).await
}

fn stage_image_blocking(name: &str, base64_data: &str) -> Result<String, String> {
    use base64::Engine as _;
    const STAGE_MAX: usize = 8 * 1024 * 1024;
    const KEEP: usize = 32;
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    if !matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
    ) {
        return Err(format!("{name} is not a supported image"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data.trim())
        .map_err(|_| "bad image data".to_string())?;
    if bytes.len() > STAGE_MAX {
        return Err("image is over the 8 MiB stage limit".into());
    }
    if parzi_core::context::encode_image(&format!("x.{ext}"), &bytes).is_none() {
        return Err(format!("{name} is not a supported image"));
    }
    let dir = parzi_core::paths::parzi_dir()
        .map_err(|e| e.to_string())?
        .join("attachments");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{}.{ext}", uuid::Uuid::new_v4()));
    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    if let Ok(mut files) = std::fs::read_dir(&dir).map(|rd| {
        rd.flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| {
                e.metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|t| (t, e.path()))
            })
            .collect::<Vec<_>>()
    }) {
        files.sort_by_key(|(t, _)| *t);
        for (_, p) in files.iter().take(files.len().saturating_sub(KEEP)) {
            let _ = std::fs::remove_file(p);
        }
    }
    Ok(dest.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_confirmed_url(url: String) -> Result<(), String> {
    parzi_core::urls::check_open_url(&url)?;
    off_thread(move || launch(&open_with_default(url.into()))).await
}

// Types the shell runs (or that make Windows fetch a remote path) when
// opened. A deny list on purpose: everything else opens in its viewer.
const RUNS_CODE: &[&str] = &[
    "exe",
    "bat",
    "cmd",
    "com",
    "ps1",
    "psm1",
    "psd1",
    "ps1xml",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "ws",
    "msi",
    "msp",
    "mst",
    "lnk",
    "scr",
    "hta",
    "jar",
    "reg",
    "cpl",
    "pif",
    "application",
    "gadget",
    "appref-ms",
    "url",
    "website",
    "msc",
    "scf",
    "sct",
    "inf",
    "chm",
    "hlp",
    "settingcontent-ms",
    "library-ms",
    "searchconnector-ms",
    "theme",
    "themepack",
    "deskthemepack",
    "appx",
    "appxbundle",
    "msix",
    "msixbundle",
    "diagcab",
    "xbap",
    "py",
    "pyw",
    "pyz",
    "pyc",
    "pl",
    "rb",
    "sh",
    "bash",
    "zsh",
    "command",
    "tool",
    "desktop",
    "app",
];

/// True when opening `p` with the default handler would run it. Windows
/// drops trailing dots and spaces, so `x.exe.` is `x.exe`; PATHEXT adds
/// whatever else this machine treats as a program.
fn runs_code(p: &Path) -> bool {
    let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_lowercase()) else {
        return false;
    };
    let name = name.trim_end_matches(['.', ' ']);
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    RUNS_CODE.contains(&ext)
        || std::env::var("PATHEXT").is_ok_and(|all| {
            all.split(';')
                .any(|x| x.trim().trim_start_matches('.').eq_ignore_ascii_case(ext))
        })
}

/// What may be opened: confined, existing, and not a program. Folders open
/// in the file manager; only an `.app` bundle folder would launch.
fn openable(store: &SessionStore, raw: &str) -> Result<(PathBuf, bool), String> {
    let (normal, canon) = confine(store, raw)?;
    let meta = std::fs::metadata(&normal)
        .map_err(|_| format!("path does not exist: {}", normal.display()))?;
    let dir = meta.is_dir();
    let launches = if dir {
        normal
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
    } else {
        runs_code(&normal) || runs_code(&canon)
    };
    if launches {
        return Err(format!(
            "{} is a program or script; Parzi does not open those",
            normal.display()
        ));
    }
    Ok((normal, dir))
}

/// Bidi overrides could make `exe.pdf` read as `pdf.exe` in the dialog.
fn shown_path(p: &Path) -> String {
    p.display()
        .to_string()
        .chars()
        .map(|c| {
            if matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                '?'
            } else {
                c
            }
        })
        .collect()
}

#[tauri::command]
pub async fn open_file_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let (orch, raw) = (state.orch.clone(), path.clone());
    let (target, dir) = off_thread(move || openable(orch.store(), &raw)).await?;
    let msg = format!(
        "Open this {}?\n\n{}",
        if dir { "folder" } else { "file" },
        shown_path(&target)
    );
    let allow = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(msg)
            .title("Parzi: open from your disk")
            .buttons(MessageDialogButtons::OkCancel)
            .blocking_show()
    })
    .await
    .map_err(|e| e.to_string())?;
    if !allow {
        return Err("open cancelled".into());
    }
    let orch = state.orch.clone();
    off_thread(move || {
        // Re-check after the dialog: the path may have changed meanwhile.
        let (again, still_dir) = openable(orch.store(), &path)?;
        if again != target || still_dir != dir {
            return Err("the file changed while the dialog was open".into());
        }
        let opener = if dir {
            reveal(&target, true)
        } else {
            open_with_default(target.into_os_string())
        };
        launch(&opener)
    })
    .await
}

/// One external program run: the OS default handler, or the file manager.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Opener {
    pub(crate) program: &'static str,
    pub(crate) args: Vec<OsString>,
    pub(crate) wait: bool,
}

#[cfg(windows)]
pub(crate) const OPEN: (&str, &[&str]) = ("rundll32", &["url.dll,FileProtocolHandler"]);
#[cfg(target_os = "macos")]
pub(crate) const OPEN: (&str, &[&str]) = ("open", &[]);
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) const OPEN: (&str, &[&str]) = ("xdg-open", &[]);

/// The default handler for a file or URL. Callers vet the target first:
/// on Windows this runs whatever the extension is associated with.
pub(crate) fn open_with_default(target: OsString) -> Opener {
    Opener {
        program: OPEN.0,
        args: OPEN.1.iter().map(OsString::from).chain([target]).collect(),
        wait: true,
    }
}

#[cfg(windows)]
pub(crate) fn reveal(full: &Path, root: bool) -> Opener {
    let args = if root {
        vec![full.into()]
    } else {
        vec!["/select,".into(), full.into()]
    };
    Opener {
        program: "explorer.exe",
        args,
        wait: false,
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn reveal(full: &Path, root: bool) -> Opener {
    let args = if root {
        vec![full.into()]
    } else {
        vec!["-R".into(), full.into()]
    };
    Opener {
        program: "open",
        args,
        wait: true,
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn reveal(full: &Path, root: bool) -> Opener {
    let dir = if root {
        full
    } else {
        full.parent().unwrap_or(full)
    };
    Opener {
        program: "xdg-open",
        args: vec![dir.into()],
        wait: true,
    }
}

/// Blocks until the handler exits when `wait`: call off the async runtime.
pub(crate) fn launch(o: &Opener) -> Result<(), String> {
    let mut cmd = std::process::Command::new(o.program);
    cmd.args(&o.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
    }
    if !o.wait {
        return cmd
            .spawn()
            .map(drop)
            .map_err(|e| format!("couldn't start {}: {e}", o.program));
    }
    let status = cmd
        .status()
        .map_err(|e| format!("couldn't start {}: {e}", o.program))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} couldn't open it", o.program))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_normalize_floors_at_root() {
        let norm = |s: &str| normalize_lexical(Path::new(s));
        assert_eq!(norm("C:\\proj\\a\\..\\b"), PathBuf::from("C:\\proj\\b"));
        assert_eq!(norm("C:\\..\\x"), PathBuf::from("C:\\x"));
        assert_eq!(norm("/a/./b"), PathBuf::from("/a/b"));
    }

    #[test]
    fn containment_is_prefix_based() {
        let roots = vec![PathBuf::from("C:\\parzi")];
        assert!(contained_in(Path::new("C:\\parzi\\a.md"), &roots));
        assert!(!contained_in(Path::new("C:\\other\\a.md"), &roots));
        assert!(!contained_in(Path::new("C:\\parzi-evil\\a.md"), &roots));
    }

    #[test]
    fn network_and_device_paths_are_refused_on_the_text() {
        for bad in [
            r"\\attacker\s\a.png",
            "//attacker/s/a.png",
            r"\/attacker\s",
            r"/\attacker\s",
            r"\\?\C:\x.png",
            r"\\?\UNC\attacker\s\a.png",
            r"\\.\PhysicalDrive0",
            r"\??\UNC\attacker\s\a.png",
            r"  \\attacker\s",
        ] {
            assert!(remote_or_device(bad), "{bad}");
            assert!(local_abs(bad).is_err(), "{bad}");
            assert!(!plain_local(Path::new(bad)), "{bad}");
        }
        assert!(!remote_or_device(r"C:\proj\a.png"));
        assert!(!remote_or_device("/home/me/a.png"));
        assert!(!remote_or_device("a/b.png"));
    }

    #[cfg(windows)]
    #[test]
    fn only_drive_paths_are_plain_on_windows() {
        assert!(plain_local(Path::new(r"C:\proj\a.png")));
        assert!(plain_local(Path::new("d:/proj/a.png")));
        assert!(!plain_local(Path::new(r"C:proj\a.png")), "drive-relative");
        assert!(!plain_local(Path::new(r"\proj\a.png")), "rooted, no drive");
        assert!(
            !plain_local(Path::new(r"C:\proj\a.txt:evil.exe")),
            "data stream"
        );
        assert!(local_abs("relative.png").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn textual_containment_ignores_case_and_verbatim_roots() {
        let roots = vec![PathBuf::from(r"\\?\C:\Users\Me\proj")];
        assert!(lexically_inside(
            Path::new(r"c:\users\me\proj\a.png"),
            &roots
        ));
        assert!(lexically_inside(Path::new(r"C:\Users\Me\proj"), &roots));
        assert!(!lexically_inside(
            Path::new(r"C:\Users\Me\proj-evil\a"),
            &roots
        ));
        assert!(!lexically_inside(Path::new(r"C:\Users\Me"), &roots));
        assert!(!lexically_inside(Path::new(r"D:\Users\Me\proj\a"), &roots));
    }

    #[test]
    fn programs_and_scripts_are_not_opened() {
        for bad in [
            "a.exe",
            "a.EXE",
            "a.bat",
            "a.cmd",
            "a.ps1",
            "a.vbs",
            "a.js",
            "a.hta",
            "a.lnk",
            "a.url",
            "a.msi",
            "a.reg",
            "a.scr",
            "a.jar",
            "a.appref-ms",
            "a.pdf.exe",
            "a.exe.",
            "a.exe . ",
            "a.py",
            "a.sh",
            "a.library-ms",
        ] {
            assert!(runs_code(Path::new(bad)), "{bad}");
        }
        for ok in [
            "a.pdf",
            "a.png",
            "a.md",
            "a.txt",
            "a.html",
            "README",
            "a.exe.txt",
        ] {
            assert!(!runs_code(Path::new(ok)), "{ok}");
        }
    }

    #[test]
    fn dialog_paths_lose_bidi_overrides() {
        let p = PathBuf::from("C:\\x\\a\u{202e}fdp.exe");
        assert!(!shown_path(&p).contains('\u{202e}'));
    }
}
