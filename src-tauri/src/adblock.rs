use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, SystemTime};

use adblock::lists::{FilterSet, ParseOptions, RuleTypes};
use adblock::request::Request;
use adblock::Engine;
use serde::{Deserialize, Serialize};
use tauri::Url;

const LISTS: &[(&str, &str)] = &[
    ("easylist.txt", "https://easylist.to/easylist/easylist.txt"),
    (
        "easyprivacy.txt",
        "https://easylist.to/easylist/easyprivacy.txt",
    ),
];
const STALE: Duration = Duration::from_hours(96);
const ENGINE_FILE: &str = "engine.dat";
const SETTINGS_FILE: &str = "adblock.json";

static ENGINE: RwLock<Option<Arc<Engine>>> = RwLock::new(None);
static ENABLED: AtomicBool = AtomicBool::new(true);
static ALLOWED: RwLock<BTreeSet<String>> = RwLock::new(BTreeSet::new());

#[derive(Serialize, Deserialize)]
struct Settings {
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    allow: BTreeSet<String>,
}

fn yes() -> bool {
    true
}

#[derive(Serialize)]
pub struct AdblockState {
    enabled: bool,
    allowed: bool,
    host: String,
}

pub fn start() {
    if let Some(s) = read_settings() {
        ENABLED.store(s.enabled, Ordering::Relaxed);
        *ALLOWED.write().unwrap_or_else(PoisonError::into_inner) = s.allow;
    }
    std::thread::spawn(|| {
        let Some(dir) = lists_dir() else {
            return;
        };
        if let Some(engine) = cached_engine(&dir) {
            install(engine);
        }
        if stale(&dir) || current().is_none() {
            match rebuild(&dir) {
                Ok(engine) => install(engine),
                Err(e) => tracing::warn!("ad block lists not updated: {e}"),
            }
        }
    });
}

pub fn should_block(url: &str, source: &str, kind: &str) -> bool {
    if !ENABLED.load(Ordering::Relaxed) || source.is_empty() {
        return false;
    }
    let Some(engine) = current() else {
        return false;
    };
    if allowed(&host_of(source)) {
        return false;
    }
    Request::new(url, source, kind, "GET")
        .is_ok_and(|request| engine.check_network_request(&request).should_block())
}

#[tauri::command]
pub fn adblock_state(url: &str) -> AdblockState {
    state(&host_of(url))
}

// Sync commands run on the main thread; the settings write must not.
#[tauri::command]
pub async fn adblock_enable(on: bool) -> Result<AdblockState, String> {
    ENABLED.store(on, Ordering::Relaxed);
    save_off_thread().await?;
    Ok(state(""))
}

#[tauri::command]
pub async fn adblock_site(url: String, allow: bool) -> Result<AdblockState, String> {
    let host = host_of(&url);
    if host.is_empty() {
        return Err("no site to change".into());
    }
    {
        let mut set = ALLOWED.write().unwrap_or_else(PoisonError::into_inner);
        if allow {
            set.insert(host.clone());
        } else {
            set.remove(&host);
        }
    }
    if !allow {
        ENABLED.store(true, Ordering::Relaxed);
    }
    save_off_thread().await?;
    Ok(state(&host))
}

async fn save_off_thread() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(save)
        .await
        .map_err(|e| e.to_string())?
}

fn state(host: &str) -> AdblockState {
    AdblockState {
        enabled: ENABLED.load(Ordering::Relaxed),
        allowed: allowed(host),
        host: host.to_string(),
    }
}

fn current() -> Option<Arc<Engine>> {
    ENGINE
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn install(engine: Engine) {
    *ENGINE.write().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(engine));
}

fn allowed(host: &str) -> bool {
    !host.is_empty()
        && ALLOWED
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(host)
}

fn host_of(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .map(|h| h.strip_prefix("www.").map_or(h.clone(), str::to_string))
        .unwrap_or_default()
}

fn settings_path() -> Option<PathBuf> {
    parzi_core::paths::parzi_dir()
        .ok()
        .map(|d| d.join(SETTINGS_FILE))
}

fn read_settings() -> Option<Settings> {
    let raw = std::fs::read_to_string(settings_path()?).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save() -> Result<(), String> {
    // Saves race on the blocking pool: one at a time, each snapshotting
    // the state once it holds the lock, so the last write is the newest.
    static SAVE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = SAVE.lock().unwrap_or_else(PoisonError::into_inner);
    let path = settings_path().ok_or("no Parzi folder")?;
    let settings = Settings {
        enabled: ENABLED.load(Ordering::Relaxed),
        allow: ALLOWED
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
    };
    let text = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
    parzi_core::error::atomic_write(&path, &text).map_err(|e| e.to_string())
}

fn lists_dir() -> Option<PathBuf> {
    let dir = parzi_core::paths::cache_dir().ok()?.join("adblock");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn cached_engine(dir: &Path) -> Option<Engine> {
    let bytes = std::fs::read(dir.join(ENGINE_FILE)).ok()?;
    let mut engine = Engine::default();
    engine.deserialize(&bytes).ok()?;
    Some(engine)
}

fn stale(dir: &Path) -> bool {
    LISTS.iter().any(|(name, _)| {
        std::fs::metadata(dir.join(name))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_none_or(|age| age > STALE)
    })
}

fn rebuild(dir: &Path) -> Result<Engine, String> {
    let mut set = FilterSet::new(false);
    let options = ParseOptions {
        rule_types: RuleTypes::NetworkOnly,
        ..ParseOptions::default()
    };
    for (name, url) in LISTS {
        let path = dir.join(name);
        let text = match download(url) {
            Ok(text) => {
                let _ = parzi_core::error::atomic_write(&path, text.as_bytes());
                text
            }
            Err(e) => std::fs::read_to_string(&path).map_err(|_| format!("{url}: {e}"))?,
        };
        set.add_filter_list(text, options);
    }
    let engine = Engine::new_with_filter_set(set);
    let _ = parzi_core::error::atomic_write(&dir.join(ENGINE_FILE), &engine.serialize());
    Ok(engine)
}

fn download(url: &str) -> Result<String, String> {
    ureq::get(url)
        .header("User-Agent", "Parzi")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_rules_block_third_party_ads_only() {
        let mut set = FilterSet::new(false);
        set.add_filter_list(
            "||ads.example.net^\n@@||ads.example.net/ok.js".into(),
            ParseOptions::default(),
        );
        install(Engine::new_with_filter_set(set));
        assert!(should_block(
            "https://ads.example.net/banner.js",
            "https://news.example.com/",
            "script"
        ));
        assert!(!should_block(
            "https://ads.example.net/ok.js",
            "https://news.example.com/",
            "script"
        ));
        assert!(!should_block(
            "https://cdn.example.com/app.js",
            "https://news.example.com/",
            "script"
        ));
        ALLOWED
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert("news.example.com".into());
        assert!(!should_block(
            "https://ads.example.net/banner.js",
            "https://www.news.example.com/",
            "script"
        ));
    }

    #[test]
    fn hosts_drop_www_and_case() {
        assert_eq!(host_of("https://WWW.Example.com/a"), "example.com");
        assert_eq!(host_of("not a url"), "");
    }
}
