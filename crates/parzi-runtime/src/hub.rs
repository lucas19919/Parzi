//! The server half of ParziOS sync, behind `parzi serve`'s socket: the
//! brain notes, the shared settings, and the mirror of every device's
//! sessions. See `sync.rs` for the device half.

use std::path::{Path, PathBuf};

use parzi_core::brain::Vault;
use parzi_core::config::ParziConfig;
use parzi_core::store::{Event, SessionMeta, SessionStatus};
use serde_json::{json, Value};

use crate::orchestrator::Orchestrator;
use crate::provision::{merge_config, portable};

const READ_CAP: usize = 8 * 1024 * 1024;
const MIRROR_PAGE: usize = 400;

fn fail(error: impl Into<String>) -> Value {
    json!({ "ok": false, "error": error.into() })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

pub fn brain_manifest() -> Value {
    match parzi_core::paths::brain_dir() {
        Ok(root) => json!({ "ok": true, "notes": crate::sync::manifest(&root) }),
        Err(e) => fail(e.to_string()),
    }
}

pub fn brain_read(req: &Value) -> Value {
    let vault = match Vault::open() {
        Ok(v) => v,
        Err(e) => return fail(e.to_string()),
    };
    let mut notes = vec![];
    let mut size = 0;
    for p in req
        .get("paths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(path) = p.as_str() else { continue };
        let Ok(full) = vault.resolve(path) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(&full) else {
            continue;
        };
        size += bytes.len();
        if size > READ_CAP {
            break;
        }
        notes.push(json!({ "path": path, "text": String::from_utf8_lossy(&bytes) }));
    }
    json!({ "ok": true, "notes": notes })
}

/// Put and delete notes. Paths go through the vault's own checks: `.md`,
/// inside the vault, no hidden folders.
pub fn brain_apply(req: &Value) -> Value {
    let vault = match Vault::open() {
        Ok(v) => v,
        Err(e) => return fail(e.to_string()),
    };
    let (mut put, mut deleted) = (0, 0);
    for n in req
        .get("put")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(path), Some(text)) = (
            n.get("path").and_then(Value::as_str),
            n.get("text").and_then(Value::as_str),
        ) else {
            continue;
        };
        let full = match vault.resolve(path) {
            Ok(f) if f != vault.root() => f,
            Ok(_) => continue,
            Err(e) => return fail(e.to_string()),
        };
        if let Err(e) = parzi_core::atomic_write(&full, text.as_bytes()) {
            return fail(format!("{path}: {e}"));
        }
        put += 1;
    }
    for p in req
        .get("delete")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(path) = p.as_str() else { continue };
        if let Ok(full) = vault.resolve(path) {
            if full != vault.root() && std::fs::remove_file(&full).is_ok() {
                deleted += 1;
            }
        }
    }
    json!({ "ok": true, "put": put, "deleted": deleted })
}

fn rev_file() -> Option<PathBuf> {
    parzi_core::paths::parzi_dir()
        .ok()
        .map(|d| d.join("sync").join("settings-rev"))
}

/// When a device last put the shared settings here (ms), 0 for never.
#[must_use]
pub fn settings_rev() -> u64 {
    rev_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

pub fn settings_get(orch: &Orchestrator) -> Value {
    json!({ "ok": true, "config": portable(orch.config()), "rev": settings_rev() })
}

pub async fn settings_put(orch: &Orchestrator, req: &Value) -> Value {
    let Some(incoming) = req
        .get("config")
        .cloned()
        .and_then(|c| serde_json::from_value::<ParziConfig>(c).ok())
    else {
        return fail("settings are missing or unreadable");
    };
    let merged = merge_config(ParziConfig::load().unwrap_or_default(), incoming);
    if let Err(e) = merged.save() {
        return fail(e.to_string());
    }
    orch.apply_config(merged).await;
    let rev = now_ms().max(settings_rev() + 1);
    if let Some(path) = rev_file() {
        let _ = parzi_core::atomic_write(&path, rev.to_string().as_bytes());
    }
    json!({ "ok": true, "rev": rev })
}

fn mirrors_dir() -> Result<PathBuf, String> {
    Ok(parzi_core::paths::parzi_dir()
        .map_err(|e| e.to_string())?
        .join("mirrors"))
}

fn valid_session_id(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// `mirrors/<device>/<session>`, refused unless both parts are plain ids.
fn mirror_dir(device: &str, id: &str) -> Result<PathBuf, String> {
    if !parzi_core::device::valid_id(device) || !valid_session_id(id) {
        return Err("not a device or session id".into());
    }
    Ok(mirrors_dir()?.join(device).join(id))
}

fn line_count(path: &Path) -> usize {
    std::fs::read(path).map_or(0, |b| b.iter().filter(|c| **c == b'\n').count())
}

/// Store a device's session: its meta, and events appended from `from`. A
/// `from` that does not match what is held is refused with the count, so
/// the device resends from there.
pub fn mirror_put(req: &Value) -> Value {
    let device = req.get("device").and_then(Value::as_str).unwrap_or("");
    let Some(meta) = req.get("session").cloned() else {
        return fail("no session");
    };
    let id = meta.get("id").and_then(Value::as_str).unwrap_or("");
    let dir = match mirror_dir(device, id) {
        Ok(d) => d,
        Err(e) => return fail(e),
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return fail(e.to_string());
    }
    let events_path = dir.join("events.jsonl");
    let have = line_count(&events_path);
    let from = req
        .get("from")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0);
    let events = req
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut lines = String::new();
    for ev in &events {
        lines.push_str(&ev.to_string());
        lines.push('\n');
    }
    let written = if from == 0 {
        parzi_core::atomic_write(&events_path, lines.as_bytes()).map(|()| events.len())
    } else if from == have {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)
            .and_then(|mut f| f.write_all(lines.as_bytes()))
            .map(|()| have + events.len())
            .map_err(Into::into)
    } else {
        return json!({ "ok": true, "have": have });
    };
    let name = req.get("name").and_then(Value::as_str).unwrap_or(device);
    let card = json!({ "device": device, "name": name, "session": meta });
    let saved = parzi_core::atomic_write(&dir.join("meta.json"), card.to_string().as_bytes());
    match (written, saved) {
        (Ok(n), Ok(())) => json!({ "ok": true, "have": n }),
        (Err(e), _) | (_, Err(e)) => fail(e.to_string()),
    }
}

/// Every mirrored session, newest first, with the device it ran on.
pub fn mirror_list() -> Value {
    let mut rows: Vec<Value> = vec![];
    let Ok(root) = mirrors_dir() else {
        return json!({ "ok": true, "sessions": rows });
    };
    for dev in std::fs::read_dir(root).into_iter().flatten().flatten() {
        for s in std::fs::read_dir(dev.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let Ok(raw) = std::fs::read_to_string(s.path().join("meta.json")) else {
                continue;
            };
            if let Ok(card) = serde_json::from_str::<Value>(&raw) {
                rows.push(card);
            }
        }
    }
    let updated = |v: &Value| {
        v.pointer("/session/updated")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    rows.sort_by_key(|v| std::cmp::Reverse(updated(v)));
    json!({ "ok": true, "sessions": rows })
}

pub fn mirror_events(req: &Value) -> Value {
    let device = req.get("device").and_then(Value::as_str).unwrap_or("");
    let id = req.get("id").and_then(Value::as_str).unwrap_or("");
    let dir = match mirror_dir(device, id) {
        Ok(d) => d,
        Err(e) => return fail(e),
    };
    let Ok(card) = std::fs::read_to_string(dir.join("meta.json"))
        .map_err(|e| e.to_string())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string()))
    else {
        return fail("no such mirrored session");
    };
    let raw = std::fs::read_to_string(dir.join("events.jsonl")).unwrap_or_default();
    let all: Vec<Value> = raw
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let from = all.len().saturating_sub(MIRROR_PAGE);
    json!({
        "ok": true,
        "session": card.get("session"),
        "name": card.get("name"),
        "events": &all[from..],
        "total": all.len(),
    })
}

pub fn mirror_delete(req: &Value) -> Value {
    let device = req.get("device").and_then(Value::as_str).unwrap_or("");
    let id = req.get("id").and_then(Value::as_str).unwrap_or("");
    match mirror_dir(device, id) {
        Ok(dir) => {
            let _ = std::fs::remove_dir_all(dir);
            json!({ "ok": true })
        }
        Err(e) => fail(e),
    }
}

/// Copy a mirrored session into this server's own store, so it can go on
/// here. Returns the new session.
pub fn mirror_adopt(orch: &Orchestrator, req: &Value) -> Value {
    let device = req.get("device").and_then(Value::as_str).unwrap_or("");
    let id = req.get("id").and_then(Value::as_str).unwrap_or("");
    let dir = match mirror_dir(device, id) {
        Ok(d) => d,
        Err(e) => return fail(e),
    };
    let card: Value = match std::fs::read_to_string(dir.join("meta.json"))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
    {
        Some(c) => c,
        None => return fail("no such mirrored session"),
    };
    let Some(meta) = card
        .get("session")
        .cloned()
        .and_then(|m| serde_json::from_value::<SessionMeta>(m).ok())
    else {
        return fail("the mirrored session is unreadable");
    };
    let store = orch.store();
    let fresh = match store.create(&meta.title, &meta.project, &meta.lane, &meta.model) {
        Ok(m) => m,
        Err(e) => return fail(e.to_string()),
    };
    let raw = std::fs::read_to_string(dir.join("events.jsonl")).unwrap_or_default();
    for line in raw.lines() {
        if let Ok(ev) = serde_json::from_str::<Event>(line) {
            let _ = store.append(&fresh.id, &ev);
        }
    }
    let _ = store.set_status(&fresh.id, SessionStatus::Idle);
    match store.get(&fresh.id) {
        Ok(m) => json!({ "ok": true, "id": m.id, "session": m }),
        Err(e) => fail(e.to_string()),
    }
}

/// Projects as this server sees them: `folder` is its own folder ("" when
/// it has none yet).
pub fn project_list() -> Value {
    match Vault::open() {
        Ok(v) => {
            let (_, projects) = v.catalog();
            json!({ "ok": true, "projects": projects })
        }
        Err(e) => fail(e.to_string()),
    }
}

/// Give a project a folder on this server (or start one).
pub fn project_folder(req: &Value) -> Value {
    let slug = req
        .get("slug")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty());
    let title = req.get("title").and_then(Value::as_str).unwrap_or("");
    let folder = req
        .get("folder")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !folder.starts_with('/') && !folder.starts_with('~') {
        return fail("give the folder as a path on the server, like /home/you/src/app");
    }
    match parzi_core::brain::project_upsert(slug, title, folder) {
        Ok(p) => json!({ "ok": true, "project": p }),
        Err(e) => fail(e.to_string()),
    }
}
