//! Files pages download: saved to the user's Downloads folder, listed in
//! History › Downloads (kept in ~/.parzi/downloads.json).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

const FILE: &str = "downloads.json";
const KEEP: usize = 100;
const EVENT: &str = "parzi://download";
// Progress reaches the UI at most this often per download.
const PROGRESS_EVERY: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Progress,
    Done,
    Failed,
    Canceled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: String,
    pub tab: String,
    pub url: String,
    pub name: String,
    pub path: String,
    pub received: u64,
    pub total: u64,
    pub state: State,
    pub started: i64,
}

struct Book {
    list: Vec<Download>,
    told: Vec<(String, Instant)>,
}

static BOOK: Mutex<Option<Book>> = Mutex::new(None);

fn book() -> MutexGuard<'static, Option<Book>> {
    let mut guard = BOOK.lock().unwrap_or_else(PoisonError::into_inner);
    if guard.is_none() {
        // A download still "in progress" in the file died with the last run.
        let mut list = load();
        for d in &mut list {
            if d.state == State::Progress {
                d.state = State::Failed;
            }
        }
        *guard = Some(Book {
            list,
            told: Vec::new(),
        });
    }
    guard
}

fn path() -> Option<PathBuf> {
    parzi_core::paths::parzi_dir().ok().map(|d| d.join(FILE))
}

fn load() -> Vec<Download> {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_soon() {
    // Download events arrive on the UI thread: never write files there.
    std::thread::spawn(|| {
        static SAVE: Mutex<()> = Mutex::new(());
        let _one = SAVE.lock().unwrap_or_else(PoisonError::into_inner);
        let list = book().as_ref().map(|b| b.list.clone()).unwrap_or_default();
        let (Some(path), Ok(text)) = (path(), serde_json::to_vec_pretty(&list)) else {
            return;
        };
        if let Err(e) = parzi_core::error::atomic_write(&path, &text) {
            tracing::debug!("downloads list not saved: {e}");
        }
    });
}

/// A file name that is safe to create: no folders, no characters Windows
/// refuses, no trailing dots or spaces, never empty.
pub fn safe_name(raw: &str) -> String {
    let leaf = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = leaf
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_end_matches(['.', ' ']).to_string();
    let reserved = {
        let stem = clean.split('.').next().unwrap_or("").to_ascii_uppercase();
        matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit())
    };
    if clean.is_empty() || clean == "." || clean == ".." {
        "download".into()
    } else if reserved {
        format!("_{clean}")
    } else {
        clean.chars().take(180).collect()
    }
}

/// `name`, else `name (1)`, `name (2)`... before the extension, like a browser.
pub fn unique_path(dir: &Path, name: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let first = dir.join(name);
    if !taken(&first) {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(at) if at > 0 => (&name[..at], &name[at..]),
        _ => (name, ""),
    };
    for n in 1..10_000 {
        let next = dir.join(format!("{stem} ({n}){ext}"));
        if !taken(&next) {
            return next;
        }
    }
    dir.join(format!("{stem} ({}){ext}", uuid::Uuid::new_v4().simple()))
}

/// The user's Downloads folder (made if missing).
pub fn downloads_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().download_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Where a new download goes in `dir`, unique among files on disk and
/// downloads still running.
pub fn target(dir: &Path, suggested: &str) -> PathBuf {
    let running: Vec<String> = book()
        .as_ref()
        .map(|b| {
            b.list
                .iter()
                .filter(|d| d.state == State::Progress)
                .map(|d| d.path.clone())
                .collect()
        })
        .unwrap_or_default();
    unique_path(dir, &safe_name(suggested), |p| {
        p.exists() || running.iter().any(|r| Path::new(r) == p)
    })
}

fn tell(app: &AppHandle, d: &Download) {
    let _ = app.emit_to("main", EVENT, d);
}

pub fn started(app: &AppHandle, id: &str, tab: &str, url: &str, path: &Path, total: u64) {
    let d = Download {
        id: id.to_string(),
        tab: tab.to_string(),
        url: url.chars().take(2048).collect(),
        name: path
            .file_name()
            .map_or_else(|| "download".into(), |n| n.to_string_lossy().to_string()),
        path: path.to_string_lossy().to_string(),
        received: 0,
        total,
        state: State::Progress,
        started: chrono::Utc::now().timestamp_millis(),
    };
    if let Some(b) = book().as_mut() {
        b.list.insert(0, d.clone());
        b.list.truncate(KEEP);
    }
    tell(app, &d);
    save_soon();
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "WebKit reports no progress here")
)]
pub fn progress(app: &AppHandle, id: &str, received: u64, total: u64) {
    let mut guard = book();
    let Some(b) = guard.as_mut() else { return };
    let Some(d) = b.list.iter_mut().find(|d| d.id == id) else {
        return;
    };
    d.received = received;
    if total > 0 {
        d.total = total;
    }
    let due = match b.told.iter_mut().find(|(t, _)| t == id) {
        Some((_, at)) if at.elapsed() < PROGRESS_EVERY => false,
        Some((_, at)) => {
            *at = Instant::now();
            true
        }
        None => {
            b.told.push((id.to_string(), Instant::now()));
            true
        }
    };
    let d = d.clone();
    drop(guard);
    if due {
        tell(app, &d);
    }
}

pub fn finished(app: &AppHandle, id: &str, state: State, path: Option<&str>) {
    let mut guard = book();
    let Some(b) = guard.as_mut() else { return };
    b.told.retain(|(t, _)| t != id);
    let Some(d) = b.list.iter_mut().find(|d| d.id == id) else {
        return;
    };
    d.state = state;
    if let Some(path) = path.filter(|p| !p.is_empty()) {
        d.path = path.to_string();
        if let Some(name) = Path::new(path).file_name() {
            d.name = name.to_string_lossy().to_string();
        }
    }
    if state == State::Done && d.total > d.received {
        d.received = d.total;
    }
    let d = d.clone();
    drop(guard);
    tell(app, &d);
    save_soon();
}

/// The newest running download of `url`, for platforms that only report
/// the URL when a download ends.
#[cfg(not(windows))]
pub fn running_id(url: &str) -> Option<String> {
    book().as_ref().and_then(|b| {
        b.list
            .iter()
            .find(|d| d.state == State::Progress && d.url == url)
            .map(|d| d.id.clone())
    })
}

pub fn new_id() -> String {
    format!("d{}", uuid::Uuid::new_v4().simple())
}

#[tauri::command]
pub fn browser_downloads() -> Vec<Download> {
    book().as_ref().map(|b| b.list.clone()).unwrap_or_default()
}

#[tauri::command]
pub async fn browser_download_reveal(id: String) -> Result<(), String> {
    let path = book()
        .as_ref()
        .and_then(|b| b.list.iter().find(|d| d.id == id).map(|d| d.path.clone()))
        .ok_or("that download is gone from the list")?;
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err("the file was moved or deleted".to_string());
        }
        crate::files::launch(&crate::files::reveal(&path, false))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn browser_download_cancel(app: AppHandle, id: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        app.run_on_main_thread(move || crate::dwm::cancel_download(&id))
            .map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (app, id);
        Err("Cancelling a download needs Windows for now".into())
    }
}

/// Drops finished entries from the list; the files stay.
#[tauri::command]
pub fn browser_downloads_clear() -> Vec<Download> {
    let list = {
        let mut guard = book();
        let Some(b) = guard.as_mut() else {
            return Vec::new();
        };
        b.list.retain(|d| d.state == State::Progress);
        b.list.clone()
    };
    save_soon();
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_folders_and_forbidden_characters() {
        assert_eq!(safe_name("report.pdf"), "report.pdf");
        assert_eq!(safe_name("..\\..\\Windows\\evil.exe"), "evil.exe");
        assert_eq!(safe_name("a/b/c:d?.txt"), "c_d_.txt");
        assert_eq!(safe_name("name. . "), "name");
        assert_eq!(safe_name(""), "download");
        assert_eq!(safe_name(".."), "download");
        assert_eq!(safe_name("CON.txt"), "_CON.txt");
        assert_eq!(safe_name("com1"), "_com1");
        assert_eq!(safe_name("compact.zip"), "compact.zip");
        assert!(safe_name(&"x".repeat(500)).len() <= 180);
    }

    #[test]
    fn unique_paths_count_up_before_the_extension() {
        let dir = Path::new("D:/dl");
        let taken = [dir.join("a.pdf"), dir.join("a (1).pdf"), dir.join("README")];
        let is_taken = |p: &Path| taken.iter().any(|t| t == p);
        assert_eq!(unique_path(dir, "b.pdf", is_taken), dir.join("b.pdf"));
        assert_eq!(unique_path(dir, "a.pdf", is_taken), dir.join("a (2).pdf"));
        assert_eq!(unique_path(dir, "README", is_taken), dir.join("README (1)"));
        assert_eq!(
            unique_path(dir, ".bashrc", |p| p == dir.join(".bashrc")),
            dir.join(".bashrc (1)")
        );
    }

    #[test]
    fn records_serialize_for_the_ui() {
        let d = Download {
            id: "d1".into(),
            tab: "t".into(),
            url: "https://x/a.zip".into(),
            name: "a.zip".into(),
            path: "C:/a.zip".into(),
            received: 1,
            total: 2,
            state: State::Progress,
            started: 0,
        };
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["state"], "progress");
        assert_eq!(v["received"], 1);
        assert!(v.get("started").is_some());
    }
}
