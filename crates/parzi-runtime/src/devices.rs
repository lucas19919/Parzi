//! Devices paired with this engine, such as a phone. Each has a token of its
//! own, kept here only as a SHA-256 hash, a level that says what it may do,
//! and when it was last seen. The list lives in `~/.parzi/devices.json`, on
//! this machine only: it is never synced.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// What a device may do, each level including the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Read sessions, notes and settings, and follow runs.
    Read,
    /// Also send messages, stop and fork runs, answer approvals and questions.
    Operate,
    /// Everything, including approving every tool at once, deleting sessions,
    /// changing settings and stopping the engine.
    Admin,
}

impl Scope {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "read" => Some(Self::Read),
            "operate" => Some(Self::Operate),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Operate => "operate",
            Self::Admin => "admin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub scope: Scope,
    /// SHA-256 of the token, hex. The token itself is shown once, when issued.
    hash: String,
    pub created: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
}

/// How often a device's last-seen time is written back, at most.
const SEEN_EVERY: chrono::TimeDelta = chrono::TimeDelta::minutes(1);
/// Tokens start with this, so one pasted somewhere is easy to recognise.
const PREFIX: &str = "pzd_";

static LOCK: Mutex<()> = Mutex::new(());

fn path() -> Result<PathBuf, String> {
    parzi_core::paths::ensure_dirs()
        .map(|root| root.join("devices.json"))
        .map_err(|e| e.to_string())
}

fn load() -> Vec<Device> {
    path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(list: &[Device]) -> Result<(), String> {
    let body = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    crate::osserve::write_private(&path()?, &body)
}

fn hash(token: &str) -> String {
    use std::fmt::Write;
    Sha256::digest(token.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Every paired device, oldest first.
#[must_use]
pub fn list() -> Vec<Device> {
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    load()
}

/// Pairs a device and returns it with its token, which is not kept and
/// cannot be shown again.
pub fn issue(name: &str, scope: Scope) -> Result<(Device, String), String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err("give the device a name of 1 to 60 characters".into());
    }
    // Two v4 UUIDs: 244 random bits from the OS.
    let token = format!(
        "{PREFIX}{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let device = Device {
        id: uuid::Uuid::new_v4().simple().to_string()[..12].to_string(),
        name: name.to_string(),
        scope,
        hash: hash(&token),
        created: chrono::Utc::now().to_rfc3339(),
        last_seen: None,
    };
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut all = load();
    all.push(device.clone());
    save(&all)?;
    Ok((device, token))
}

/// Unpairs a device by id or exact name; its token stops working at once.
pub fn revoke(which: &str) -> Result<Device, String> {
    let which = which.trim();
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut all = load();
    let hits: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, d)| d.id == which || d.name.eq_ignore_ascii_case(which))
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [] => Err(format!("no paired device `{which}`")),
        [i] => {
            let gone = all.remove(*i);
            save(&all)?;
            Ok(gone)
        }
        _ => Err(format!(
            "more than one device is called `{which}`; use its id"
        )),
    }
}

/// The device with this id, if it is still paired.
#[must_use]
pub fn verify_id(id: &str) -> Option<Device> {
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    load().into_iter().find(|d| d.id == id)
}

/// The device a token belongs to, if any, noting that it was seen now.
#[must_use]
pub fn verify(token: &str) -> Option<Device> {
    let token = token.trim();
    if !token.starts_with(PREFIX) {
        return None;
    }
    let want = hash(token);
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut all = load();
    let i = all
        .iter()
        .position(|d| crate::rpc::tokens_equal(&d.hash, &want))?;
    let now = chrono::Utc::now();
    let stale = all[i]
        .last_seen
        .as_deref()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .is_none_or(|t| now.signed_duration_since(t) > SEEN_EVERY);
    if stale {
        all[i].last_seen = Some(now.to_rfc3339());
        let _ = save(&all);
    }
    Some(all[i].clone())
}
