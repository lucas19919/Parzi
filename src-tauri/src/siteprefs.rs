
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Url};

const FILE: &str = "browser.json";

pub const KINDS: &[&str] = &[
    "camera",
    "microphone",
    "geolocation",
    "notifications",
    "sensors",
    "clipboard",
    "downloads",
    "files",
    "fonts",
    "midi",
    "windows",
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub devtools: bool,
    pub agent_sign_ins: bool,
    pub sites: BTreeMap<String, BTreeMap<String, bool>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SiteRow {
    origin: String,
    kind: String,
    allow: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefsView {
    devtools: bool,
    agent_sign_ins: bool,
    sites: Vec<SiteRow>,
    windows: bool,
}

impl Prefs {
    pub fn decision(&self, origin: &str, kind: &str) -> Option<bool> {
        self.sites.get(origin)?.get(kind).copied()
    }

    pub fn remember(&mut self, origin: &str, kind: &str, allow: bool) -> bool {
        let Some(origin) = site_origin(origin) else {
            return false;
        };
        if !KINDS.contains(&kind) {
            return false;
        }
        self.sites
            .entry(origin)
            .or_default()
            .insert(kind.to_string(), allow);
        true
    }

    pub fn forget(&mut self, origin: &str, kind: Option<&str>) {
        match kind {
            Some(kind) => {
                if let Some(site) = self.sites.get_mut(origin) {
                    site.remove(kind);
                    if site.is_empty() {
                        self.sites.remove(origin);
                    }
                }
            }
            None => {
                self.sites.remove(origin);
            }
        }
    }

    pub fn rows(&self) -> Vec<SiteRow> {
        self.sites
            .iter()
            .flat_map(|(origin, kinds)| {
                kinds.iter().map(|(kind, allow)| SiteRow {
                    origin: origin.clone(),
                    kind: kind.clone(),
                    allow: *allow,
                })
            })
            .collect()
    }
}

pub fn site_origin(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    Some(url.origin().ascii_serialization().to_ascii_lowercase())
}

static PREFS: RwLock<Option<Prefs>> = RwLock::new(None);

fn path() -> Option<PathBuf> {
    parzi_core::paths::parzi_dir().ok().map(|d| d.join(FILE))
}

fn load() -> Prefs {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn get() -> Prefs {
    if let Some(p) = PREFS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        return p.clone();
    }
    let mut slot = PREFS.write().unwrap_or_else(PoisonError::into_inner);
    slot.get_or_insert_with(load).clone()
}

fn update(change: impl FnOnce(&mut Prefs)) -> Prefs {
    let mut slot = PREFS.write().unwrap_or_else(PoisonError::into_inner);
    let prefs = slot.get_or_insert_with(load);
    change(prefs);
    prefs.clone()
}

pub fn devtools() -> bool {
    get().devtools
}

pub fn agent_sign_ins() -> bool {
    get().agent_sign_ins
}

#[cfg_attr(not(windows), allow(dead_code, reason = "WebView2 hooks only"))]
pub fn decision(origin: &str, kind: &str) -> Option<bool> {
    get().decision(origin, kind)
}

pub async fn remember(origin: String, kind: String, allow: bool) -> Result<(), String> {
    let mut valid = false;
    update(|p| valid = p.remember(&origin, &kind, allow));
    if !valid {
        return Err("that permission can't be remembered".into());
    }
    save_off_thread().await
}

async fn save_off_thread() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(save)
        .await
        .map_err(|e| e.to_string())?
}

fn save() -> Result<(), String> {
    static SAVE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = SAVE.lock().unwrap_or_else(PoisonError::into_inner);
    let path = path().ok_or("no Parzi folder")?;
    let text = serde_json::to_vec_pretty(&get()).map_err(|e| e.to_string())?;
    parzi_core::error::atomic_write(&path, &text).map_err(|e| e.to_string())
}

fn view(p: &Prefs) -> PrefsView {
    PrefsView {
        devtools: p.devtools,
        agent_sign_ins: p.agent_sign_ins,
        sites: p.rows(),
        windows: cfg!(windows),
    }
}

#[tauri::command]
pub fn browser_prefs() -> PrefsView {
    view(&get())
}

#[tauri::command]
pub async fn browser_set_pref(app: AppHandle, name: String, on: bool) -> Result<PrefsView, String> {
    let prefs = match name.as_str() {
        "devtools" => update(|p| p.devtools = on),
        "agentSignIns" => update(|p| p.agent_sign_ins = on),
        _ => return Err(format!("unknown browser setting {name}")),
    };
    save_off_thread().await?;
    if name == "devtools" {
        crate::browser::apply_devtools(&app, on);
    }
    Ok(view(&prefs))
}

#[tauri::command]
pub async fn browser_forget_site(
    origin: String,
    kind: Option<String>,
) -> Result<PrefsView, String> {
    let prefs = update(|p| p.forget(&origin, kind.as_deref()));
    save_off_thread().await?;
    Ok(view(&prefs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_web_origins_only() {
        assert_eq!(
            site_origin("https://Meet.Google.com/abc?x=1#y").as_deref(),
            Some("https://meet.google.com")
        );
        assert_eq!(
            site_origin("http://localhost:3000/a").as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(site_origin("data:text/html,hi"), None);
        assert_eq!(site_origin("file:///C:/x.html"), None);
        assert_eq!(site_origin("not a url"), None);
    }

    #[test]
    fn decisions_are_per_site_and_kind() {
        let mut p = Prefs::default();
        assert!(p.remember("https://meet.google.com/abc", "camera", true));
        assert!(p.remember("https://meet.google.com", "microphone", false));
        assert!(!p.remember("https://meet.google.com", "teleport", true));
        assert!(!p.remember("about:blank", "camera", true));
        assert_eq!(p.decision("https://meet.google.com", "camera"), Some(true));
        assert_eq!(
            p.decision("https://meet.google.com", "microphone"),
            Some(false)
        );
        assert_eq!(p.decision("https://zoom.us", "camera"), None);
        assert_eq!(p.rows().len(), 2);
        p.forget("https://meet.google.com", Some("camera"));
        assert_eq!(p.decision("https://meet.google.com", "camera"), None);
        p.forget("https://meet.google.com", Some("microphone"));
        assert!(p.sites.is_empty(), "an empty site is dropped");
        p.remember("https://a.com", "camera", true);
        p.remember("https://a.com", "geolocation", false);
        p.forget("https://a.com", None);
        assert!(p.rows().is_empty());
    }

    #[test]
    fn prefs_round_trip_and_default_off() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert!(!p.devtools && !p.agent_sign_ins);
        let mut p = Prefs {
            devtools: true,
            ..Prefs::default()
        };
        p.remember("https://a.com", "notifications", false);
        let back: Prefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }
}
