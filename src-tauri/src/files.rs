use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use crate::AppState;

/// Runs blocking work off the async threads; the one helper every command uses.
pub(crate) async fn off_thread<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

pub(crate) fn remote_or_device(raw: &str) -> bool {
    let b = raw.trim().as_bytes();
    let sep = |c: &u8| *c == b'\\' || *c == b'/';
    let unc = b.len() >= 2 && sep(&b[0]) && sep(&b[1]);
    let nt = b.len() >= 4 && sep(&b[0]) && b[1] == b'?' && b[2] == b'?' && sep(&b[3]);
    unc || nt
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

fn resolve_path(raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty path".into());
    }
    Ok(normalize_lexical(Path::new(raw)))
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle, start: Option<String>) -> Result<Option<String>, String> {
    let mut builder = app.dialog().file().set_title("Choose a folder");
    if let Some(dir) = start
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
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
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn git_branch(cwd: String) -> Result<String, String> {
    if cwd.is_empty() {
        return Ok(String::new());
    }
    let cwd = resolve_path(&cwd)?;
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
pub async fn git_branches(cwd: String) -> Result<crate::worktree::Branches, String> {
    if cwd.is_empty() {
        return Ok(crate::worktree::Branches::default());
    }
    off_thread(move || {
        let dir = resolve_path(&cwd)?;
        Ok(crate::worktree::branches(&dir))
    })
    .await
}

#[tauri::command]
pub async fn git_worktrees(cwd: String) -> Result<Vec<crate::worktree::WorktreeEntry>, String> {
    if cwd.is_empty() {
        return Ok(vec![]);
    }
    off_thread(move || {
        let dir = resolve_path(&cwd)?;
        Ok(crate::worktree::list_worktrees(&dir))
    })
    .await
}

#[tauri::command]
pub async fn git_switch_branch(cwd: String, branch: String) -> Result<String, String> {
    if cwd.is_empty() {
        return Err("No workspace folder.".into());
    }
    off_thread(move || {
        let dir = resolve_path(&cwd)?;
        crate::worktree::switch_branch(&dir, &branch)
    })
    .await
}

#[tauri::command]
pub async fn git_create_worktree(
    cwd: String,
    base: String,
    name: String,
) -> Result<String, String> {
    if cwd.is_empty() {
        return Err("No workspace folder.".into());
    }
    off_thread(move || {
        let dir = resolve_path(&cwd)?;
        let base_ref = if base.trim().is_empty() {
            "HEAD"
        } else {
            base.trim()
        };
        let label = if name.trim().is_empty() {
            "session"
        } else {
            name.trim()
        };
        let path = crate::worktree::create(&dir, base_ref, label)?;
        Ok(path.display().to_string())
    })
    .await
}

#[tauri::command]
pub async fn list_files(root: String, query: String) -> Result<Vec<String>, String> {
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
    let q = query.to_lowercase();
    off_thread(move || {
        let root_path = resolve_path(&root)?;
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
const READ_TEXT_MAX: u64 = 1_000_000;

#[tauri::command]
pub async fn read_text_file(path: String) -> Result<String, String> {
    off_thread(move || read_text(&resolve_path(&path)?)).await
}

/// A text file's contents, refusing what is too large or not text.
fn read_text(p: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", p.display()));
    }
    if meta.len() > READ_TEXT_MAX {
        return Err("That file is too large to open here".into());
    }
    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
    if bytes.contains(&0) {
        return Err("That file isn't text".into());
    }
    String::from_utf8(bytes).map_err(|_| "That file isn't text".into())
}

#[tauri::command]
pub async fn read_image_data_url(path: String, cwd: String) -> Result<String, String> {
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
    off_thread(move || {
        let p = resolve_path(&abs)?;
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
        let img = parzi_core::context::encode_picture_file(&p)
            .ok_or_else(|| format!("{} is not a supported image", p.display()))?;
        Ok(img.data_url())
    })
    .await
}

#[tauri::command]
pub async fn tool_image(
    state: State<'_, AppState>,
    session: String,
    path: String,
) -> Result<String, String> {
    let orch = state.orch.clone();
    off_thread(move || {
        let fold = |s: &str| s.replace('\\', "/").to_lowercase();
        let want = fold(&path);
        let named = orch
            .store()
            .events(&session)
            .map_err(|e| e.to_string())?
            .iter()
            .any(|e| match e {
                parzi_core::store::Event::ToolResult { output, .. } => fold(output)
                    .replace("//", "/")
                    .contains(&want.replace("//", "/")),
                _ => false,
            });
        if !named {
            return Err("no tool in this session made that picture".into());
        }
        let p = resolve_path(&path)?;
        let meta = std::fs::metadata(&p).map_err(|e| format!("{path}: {e}"))?;
        if !meta.is_file() || meta.len() > READ_IMAGE_MAX {
            return Err(format!("{path} is not a picture Parzi can show"));
        }
        let img = parzi_core::context::encode_picture_file(&p)
            .ok_or_else(|| format!("{path} is not a supported image"))?;
        Ok(img.data_url())
    })
    .await
}

fn file_name_for(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_matches('.').to_string();
    if clean.is_empty() {
        "file.txt".into()
    } else {
        clean.chars().take(120).collect()
    }
}

#[tauri::command]
pub async fn save_as(
    app: AppHandle,
    name: String,
    content: String,
    base64: Option<bool>,
) -> Result<Option<String>, String> {
    use base64::Engine as _;
    const SAVE_MAX: usize = 64 * 1024 * 1024;
    let bytes = if base64.unwrap_or(false) {
        base64::engine::general_purpose::STANDARD
            .decode(content.trim())
            .map_err(|_| "that file's data is damaged".to_string())?
    } else {
        content.into_bytes()
    };
    if bytes.len() > SAVE_MAX {
        return Err("that is too big to save from here (64 MB at most)".into());
    }
    let name = file_name_for(&name);
    let mut builder = app
        .dialog()
        .file()
        .set_title("Save as")
        .set_file_name(&name);
    if let Ok(dir) = tauri::Manager::path(&app).download_dir() {
        builder = builder.set_directory(dir);
    }
    if let Some(ext) = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()) {
        if !ext.is_empty() && ext.len() <= 8 {
            builder = builder.add_filter(ext.to_ascii_uppercase(), &[ext.as_str()]);
        }
    }
    let (tx, rx) = oneshot::channel();
    builder.save_file(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.map_err(|_| "the save dialog closed".to_string())? else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    let target = path.clone();
    off_thread(move || {
        std::fs::write(&target, &bytes).map_err(|e| format!("couldn't save: {e}"))?;
        Ok(())
    })
    .await?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn reveal_saved(path: String) -> Result<(), String> {
    off_thread(move || {
        let target = resolve_path(&path)?;
        launch(&reveal(&target, false))
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

#[tauri::command]
pub async fn open_file_path(path: String) -> Result<(), String> {
    off_thread(move || {
        let target = resolve_path(&path)?;
        let meta = std::fs::metadata(&target)
            .map_err(|_| format!("path does not exist: {}", target.display()))?;
        let opener = if meta.is_dir() {
            reveal(&target, true)
        } else {
            open_with_default(target.into_os_string())
        };
        launch(&opener)
    })
    .await
}

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
        #[cfg(windows)]
        {
            assert_eq!(norm("C:\\proj\\a\\..\\b"), PathBuf::from("C:\\proj\\b"));
            assert_eq!(norm("C:\\..\\x"), PathBuf::from("C:\\x"));
        }
        assert_eq!(norm("/proj/a/../b"), PathBuf::from("/proj/b"));
        assert_eq!(norm("/../x"), PathBuf::from("/x"));
        assert_eq!(norm("/a/./b"), PathBuf::from("/a/b"));
    }

    #[test]
    fn resolve_path_accepts_any_non_empty_path() {
        assert!(resolve_path("").is_err());
        assert!(resolve_path("   ").is_err());
        #[cfg(windows)]
        assert_eq!(
            resolve_path("  D:\\Repos\\Parzi\\a\\..\\b.txt  ").unwrap(),
            PathBuf::from("D:\\Repos\\Parzi\\b.txt")
        );
        assert_eq!(
            resolve_path("/tmp/foo/../bar.md").unwrap(),
            PathBuf::from("/tmp/bar.md")
        );
    }

    #[test]
    fn remote_or_device_recognizes_unc_and_nt_paths() {
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
        }
        assert!(!remote_or_device(r"C:\proj\a.png"));
        assert!(!remote_or_device("/home/me/a.png"));
        assert!(!remote_or_device("a/b.png"));
    }
}
