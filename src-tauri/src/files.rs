use std::path::{Path, PathBuf};

use parzi_core::store::SessionStore;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use crate::AppState;

static GRANTED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

fn grant_folder(path: &Path) {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Ok(mut g) = GRANTED.lock() {
        if !g.contains(&canon) {
            g.push(canon);
        }
    }
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle, start: Option<String>) -> Result<Option<String>, String> {
    let mut builder = app.dialog().file().set_title("Choose a folder");
    if let Some(dir) = start.filter(|s| !s.trim().is_empty()) {
        builder = builder.set_directory(dir);
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
    grant_folder(&path);
    Ok(Some(path.to_string_lossy().into_owned()))
}

fn file_roots(store: &SessionStore) -> Vec<PathBuf> {
    let mut roots = vec![];
    if let Ok(home) = parzi_core::paths::parzi_dir() {
        roots.push(home.canonicalize().unwrap_or(home));
    }
    if let Ok(g) = GRANTED.lock() {
        roots.extend(g.iter().cloned());
    }
    let cwds: std::collections::BTreeSet<String> = store
        .list()
        .unwrap_or_default()
        .into_iter()
        .map(|m| m.cwd.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    for cwd in cwds {
        let p = normalize_lexical(Path::new(&cwd));
        if !p.is_absolute() || p.parent().is_none() {
            continue;
        }
        roots.push(p.canonicalize().unwrap_or(p));
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

fn confined_path(store: &SessionStore, raw: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(raw.trim());
    if p.as_os_str().is_empty() {
        return Err("empty path".into());
    }
    if !p.is_absolute() {
        return Err("path must be absolute".into());
    }
    let normal = normalize_lexical(&p);
    let mut probe: Option<&Path> = Some(&normal);
    let mut canon: Option<PathBuf> = None;
    while let Some(q) = probe {
        if let Ok(c) = q.canonicalize() {
            canon = Some(c);
            break;
        }
        probe = q.parent();
    }
    let canon = canon.ok_or_else(|| "cannot resolve path".to_string())?;
    if contained_in(&canon, &file_roots(store)) {
        Ok(normal)
    } else {
        Err("that location is outside the workspace — choose the folder first".into())
    }
}

#[tauri::command]
pub async fn git_branch(state: State<'_, AppState>, cwd: String) -> Result<String, String> {
    if cwd.is_empty() {
        return Ok(String::new());
    }
    let cwd = confined_path(state.orch.store(), &cwd)?;
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
    let root_path = confined_path(state.orch.store(), &root)?;
    let q = query.to_lowercase();
    // FS walks run on the blocking pool so per-keystroke @-mentions
    // never stall the 3 async workers that serve all other commands.
    let out = tokio::task::spawn_blocking(move || {
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
        out
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(out)
}

const READ_IMAGE_MAX: u64 = 8 * 1024 * 1024;

#[tauri::command]
pub async fn read_image_data_url(
    state: State<'_, AppState>,
    path: String,
    cwd: String,
) -> Result<String, String> {
    let abs = if Path::new(&path).is_absolute() {
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
    let p = confined_path(state.orch.store(), &abs)?;
    let meta = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", p.display()));
    }
    if meta.len() > READ_IMAGE_MAX {
        return Err(format!(
            "{} is {} KiB — over the {} KiB image limit",
            p.display(),
            meta.len() / 1024,
            READ_IMAGE_MAX / 1024
        ));
    }
    let img = parzi_core::context::encode_image_file(&p)
        .ok_or_else(|| format!("{} is not a supported image", p.display()))?;
    Ok(img.data_url())
}

#[tauri::command]
pub async fn stage_image(name: String, base64_data: String) -> Result<String, String> {
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
    open_url_native(&url)
}

#[tauri::command]
pub async fn open_file_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(path.trim());
    if !p.exists() {
        return Err(format!("path does not exist: {}", p.display()));
    }
    open_url_native(&p.to_string_lossy())
}

fn open_url_native(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let status = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .status()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open")
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let status = std::process::Command::new("xdg-open")
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("couldn't open the browser".into())
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
}
