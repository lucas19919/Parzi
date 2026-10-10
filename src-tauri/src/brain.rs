use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use parzi_core::brain::{self, BrainContext, NoteMeta, Project, Vault};

use crate::files::{launch, open_with_default, remote_or_device, reveal, Opener};

async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> parzi_core::Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn brain_dir() -> Result<String, String> {
    parzi_core::paths::brain_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn brain_list() -> Result<Vec<NoteMeta>, String> {
    blocking(brain::list).await
}

#[tauri::command]
pub async fn brain_read(path: String) -> Result<String, String> {
    blocking(move || brain::read(&path)).await
}

#[tauri::command]
pub async fn brain_write(path: String, content: String) -> Result<NoteMeta, String> {
    blocking(move || brain::write(&path, &content)).await
}

#[tauri::command]
pub async fn brain_save_answer(content: String) -> Result<NoteMeta, String> {
    blocking(move || {
        let slug: String = content
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("note")
            .trim_start_matches('#')
            .trim()
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ')
            .collect::<String>()
            .split_whitespace()
            .take(6)
            .collect::<Vec<_>>()
            .join("-")
            .to_lowercase();
        let slug = if slug.is_empty() { "note".into() } else { slug };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        brain::write(&format!("research/{slug}-{stamp}"), &content)
    })
    .await
}

#[tauri::command]
pub async fn brain_delete(path: String) -> Result<(), String> {
    blocking(move || brain::delete(&path)).await
}

#[tauri::command]
pub async fn brain_projects() -> Result<Vec<Project>, String> {
    blocking(brain::projects).await
}

#[tauri::command]
pub async fn brain_project_upsert(title: String, folder: String) -> Result<Project, String> {
    // The vault resolves project folders; a network one would be contacted.
    if remote_or_device(&folder) {
        return Err("network and device folders cannot be projects".into());
    }
    blocking(move || brain::project_upsert(None, &title, &folder)).await
}

#[tauri::command]
pub async fn brain_map(note: String, project: String, on: bool) -> Result<NoteMeta, String> {
    blocking(move || brain::map(&note, &project, on)).await
}

#[tauri::command]
pub async fn brain_context(cwd: String) -> Result<Option<BrainContext>, String> {
    if remote_or_device(&cwd) {
        return Ok(None);
    }
    blocking(move || Ok(brain::context_for(Path::new(cwd.trim())))).await
}

#[tauri::command]
pub async fn brain_pin(path: String, on: bool) -> Result<NoteMeta, String> {
    blocking(move || brain::pin(&path, on)).await
}

#[tauri::command]
pub async fn brain_open(path: String, target: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let vault = Vault::open().map_err(|e| e.to_string())?;
        launch(&plan(&vault, &path, &target)?)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn brain_obsidian() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let brain = parzi_core::paths::brain_dir().map_err(|e| e.to_string())?;
        let state = match obsidian_config().map(std::fs::read_to_string) {
            None => "missing",
            Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => "missing",
            Some(Err(_)) => "unregistered",
            Some(Ok(raw)) => obsidian_state(&raw, &brain),
        };
        Ok(state.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn plan(vault: &Vault, path: &str, target: &str) -> Result<Opener, String> {
    let root = path.trim().is_empty();
    let full = vault.resolve(path).map_err(|e| e.to_string())?;
    let full = std::path::absolute(&full).map_err(|e| e.to_string())?;
    if root {
        std::fs::create_dir_all(&full).map_err(|e| e.to_string())?;
    } else if !full.is_file() {
        return Err(format!("no note at {}", path.trim()));
    }
    match target {
        "file" => Ok(open_with_default(full.into_os_string())),
        "folder" => Ok(reveal(&full, root)),
        "obsidian" => Ok(open_with_default(obsidian_url(&full).into())),
        other => Err(format!("cannot open a note in `{other}`")),
    }
}

fn obsidian_url(full: &Path) -> String {
    format!(
        "obsidian://open?path={}",
        percent_encode(&full.to_string_lossy())
    )
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn obsidian_config() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::home_dir().map(|h| h.join("Library").join("Application Support"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".config")));
    base.map(|b| b.join("obsidian").join("obsidian.json"))
}

fn obsidian_state(raw: &str, brain: &Path) -> &'static str {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return "unregistered";
    };
    let ready = v
        .get("vaults")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|vaults| {
            vaults
                .values()
                .filter_map(|x| x.get("path")?.as_str())
                .any(|p| sees(Path::new(p), brain))
        });
    if ready {
        "ready"
    } else {
        "unregistered"
    }
}

fn sees(vault: &Path, brain: &Path) -> bool {
    brain::inside(vault, brain)
        && brain
            .ancestors()
            .take_while(|a| !brain::inside(a, vault))
            .all(|a| {
                a.file_name()
                    .is_none_or(|n| !n.to_string_lossy().starts_with('.'))
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::OPEN;
    use std::ffi::OsString;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("parzi-brain-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn obsidian_is_ready_only_when_a_vault_sees_the_brain() {
        let base = std::env::temp_dir().join("parzi-obsidian-nowhere");
        let brain = base.join("home").join(".parzi").join("brain");
        let state = |vault: &Path| {
            let raw = serde_json::json!({"vaults": {"a1": {"path": vault, "ts": 1, "open": true}}});
            obsidian_state(&raw.to_string(), &brain)
        };
        assert_eq!(state(&brain), "ready");
        assert_eq!(state(&base.join("home").join(".parzi")), "ready");
        assert_eq!(state(&base.join("home")), "unregistered");
        assert_eq!(state(&base.join("elsewhere")), "unregistered");
        assert_eq!(state(&brain.join("sub")), "unregistered");
        assert_eq!(state(&base.join("home").join(".par")), "unregistered");
        assert_eq!(obsidian_state("not json", &brain), "unregistered");
        assert_eq!(obsidian_state("{}", &brain), "unregistered");
        assert_eq!(
            obsidian_state(r#"{"vaults": {"x": {"path": 3}}}"#, &brain),
            "unregistered"
        );
        let open = base.join("notes").join("brain");
        let raw = serde_json::json!({"vaults": {"b": {"path": brain}, "c": {"path": base.join("notes")}}});
        assert_eq!(obsidian_state(&raw.to_string(), &open), "ready");
        #[cfg(windows)]
        {
            let shouty = brain.to_string_lossy().to_uppercase().replace('\\', "/");
            let raw = serde_json::json!({"vaults": {"x": {"path": format!("{shouty}/")}}});
            assert_eq!(obsidian_state(&raw.to_string(), &brain), "ready");
        }
    }

    #[test]
    fn obsidian_links_percent_encode_everything_but_unreserved() {
        assert_eq!(
            percent_encode("C:\\my notes/ä~x-y_z.md"),
            "C%3A%5Cmy%20notes%2F%C3%A4~x-y_z.md"
        );
        assert_eq!(obsidian_url(Path::new("a b")), "obsidian://open?path=a%20b");
    }

    #[test]
    fn opening_stays_inside_the_vault() {
        let dir = temp("open");
        let vault = Vault::at(dir.join("brain"));
        vault.write("notes/a b.md", "# A").unwrap();
        for bad in [
            "../x.md",
            "C:\\x.md",
            "/etc/x.md",
            ".obsidian/x.md",
            "notes/a b",
        ] {
            assert!(plan(&vault, bad, "file").is_err(), "{bad}");
        }
        assert!(plan(&vault, "notes/missing.md", "file")
            .unwrap_err()
            .starts_with("no note at"));
        assert!(plan(&vault, "notes/a b.md", "browser").is_err());
        let note = std::path::absolute(dir.join("brain").join("notes").join("a b.md")).unwrap();
        let root = std::path::absolute(dir.join("brain")).unwrap();
        let file = plan(&vault, "notes/a b.md", "file").unwrap();
        assert_eq!(file.program, OPEN.0);
        assert_eq!(file.args.last().unwrap(), note.as_os_str());
        let obsidian = plan(&vault, "notes\\a b.md", "obsidian").unwrap();
        assert_eq!(
            obsidian.args.last().unwrap().to_string_lossy(),
            obsidian_url(&note)
        );
        assert!(!obsidian_url(&note).contains(' '));
        let folder = plan(&vault, "notes/a b.md", "folder").unwrap();
        let top = plan(&vault, " ", "folder").unwrap();
        #[cfg(windows)]
        {
            assert_eq!(
                (file.args[0].to_str(), file.wait),
                (Some("url.dll,FileProtocolHandler"), true)
            );
            assert_eq!(
                folder,
                Opener {
                    program: "explorer.exe",
                    args: vec!["/select,".into(), note.clone().into()],
                    wait: false,
                }
            );
            assert_eq!(top.args, vec![OsString::from(&root)]);
        }
        #[cfg(not(windows))]
        {
            assert!(folder.args.last().is_some());
            assert!(top.args.last().unwrap() == root.as_os_str());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
