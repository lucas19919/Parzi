//! ParziOS sync. The server is home for brain notes, settings and session
//! history; every device keeps a replica in step with it over its link.
//!
//! Notes sync three ways: what is here, what is on the server, and what both
//! agreed on last time (`~/.parzi/sync/brain.json`). A note changed on one
//! side moves to the other; a note deleted on one side and untouched on the
//! other is deleted; a note changed on both sides keeps the server's text
//! and saves this device's as `<name>.conflict-<device>.md` next to it, on
//! both sides. Nothing is ever dropped silently.
//!
//! Settings: the portable config (connectors and agent paths stay per
//! machine), newest change wins. Sessions: each device mirrors its own
//! history to the server, which any device can read and continue there.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use parzi_core::brain::Vault;
use parzi_core::config::ParziConfig;
use parzi_core::store::SessionStore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::provision::{merge_config, portable};
use crate::remote::Link;

const MAX_FILES: usize = 5_000;
const MAX_NOTE: u64 = 1024 * 1024;
const BATCH_PATHS: usize = 200;
const BATCH_BYTES: usize = 4 * 1024 * 1024;
const CALL: Duration = Duration::from_secs(60);

/// FNV-1a 64: tiny, fixed forever, plenty for telling note versions apart.
#[must_use]
pub fn fnv(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Every `.md` note under `root` (forward-slash paths) with its hash.
/// Hidden folders, links and notes over 1 MB are left out.
#[must_use]
pub fn manifest(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![(root.to_path_buf(), String::new(), 0u8)];
    while let Some((dir, rel, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = e.file_type() else { continue };
            let path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if kind.is_dir() && depth < 12 {
                stack.push((e.path(), path, depth + 1));
            } else if kind.is_file()
                && name.to_lowercase().ends_with(".md")
                && e.metadata().is_ok_and(|m| m.len() <= MAX_NOTE)
            {
                if let Ok(bytes) = std::fs::read(e.path()) {
                    out.insert(path, fnv(&bytes));
                }
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Same on both sides.
    Keep,
    Pull,
    Push,
    DeleteHere,
    DeleteThere,
    /// Changed on both sides since they last agreed.
    Conflict,
}

/// What to do with each note, from both sides and their last agreement.
#[must_use]
pub fn plan(
    here: &BTreeMap<String, String>,
    there: &BTreeMap<String, String>,
    base: &BTreeMap<String, String>,
) -> Vec<(String, Act)> {
    let paths: BTreeSet<&String> = here.keys().chain(there.keys()).chain(base.keys()).collect();
    paths
        .into_iter()
        .filter_map(|p| {
            let (h, t, b) = (here.get(p), there.get(p), base.get(p));
            let act = match (h, t) {
                (Some(h), Some(t)) if h == t => Act::Keep,
                (Some(_), Some(t)) if b == Some(t) => Act::Push,
                (Some(h), Some(_)) if b == Some(h) => Act::Pull,
                (Some(_), Some(_)) => Act::Conflict,
                // Deleted there and untouched here: delete. Changed here
                // (or new here): it wins and goes back.
                (Some(h), None) if b == Some(h) => Act::DeleteHere,
                (Some(_), None) => Act::Push,
                (None, Some(t)) if b == Some(t) => Act::DeleteThere,
                (None, Some(_)) => Act::Pull,
                (None, None) => return None,
            };
            Some((p.clone(), act))
        })
        .collect()
}

/// `notes/a.md` -> `notes/a.conflict-<device>.md`, numbered when taken.
#[must_use]
pub fn conflict_path(path: &str, device: &str, taken: &dyn Fn(&str) -> bool) -> String {
    let stem = path.strip_suffix(".md").unwrap_or(path);
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            format!("{stem}.conflict-{device}.md")
        } else {
            format!("{stem}.conflict-{device}-{n}.md")
        };
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

fn sync_dir() -> Result<PathBuf, String> {
    let dir = parzi_core::paths::parzi_dir()
        .map_err(|e| e.to_string())?
        .join("sync");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn load_state<T: for<'a> Deserialize<'a> + Default>(name: &str, server: &str) -> T {
    #[derive(Deserialize)]
    struct Wrapped<T> {
        server: String,
        state: T,
    }
    sync_dir()
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join(name)).ok())
        .and_then(|raw| serde_json::from_str::<Wrapped<T>>(&raw).ok())
        .filter(|w| w.server == server)
        .map(|w| w.state)
        .unwrap_or_default()
}

fn save_state<T: Serialize>(name: &str, server: &str, state: &T) -> Result<(), String> {
    let body = serde_json::to_vec(&json!({ "server": server, "state": state }))
        .map_err(|e| e.to_string())?;
    parzi_core::atomic_write(&sync_dir()?.join(name), &body).map_err(|e| e.to_string())
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct BrainReport {
    pub pulled: usize,
    pub pushed: usize,
    pub deleted_here: usize,
    pub deleted_there: usize,
    pub conflicts: usize,
    /// Notes this sync changed on this device (pulled, deleted, or set to
    /// the server's side of a conflict), for whoever shows them.
    #[serde(skip)]
    pub changed: Vec<String>,
}

impl BrainReport {
    #[must_use]
    pub fn changed(&self) -> bool {
        self.pulled + self.pushed + self.deleted_here + self.deleted_there + self.conflicts > 0
    }
}

/// The server end of a brain sync: `parzi serve`'s socket over a link, or
/// anything else that answers `brain.manifest`, `brain.read` and
/// `brain.apply` the same way.
#[async_trait::async_trait]
pub trait NoteServer: Send + Sync {
    async fn call(&self, op: &str, body: Value) -> Result<Value, String>;
}

#[async_trait::async_trait]
impl NoteServer for Link {
    async fn call(&self, op: &str, body: Value) -> Result<Value, String> {
        Link::call(self, op, body, CALL).await
    }
}

async fn read_there(
    server: &dyn NoteServer,
    paths: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for chunk in paths.chunks(BATCH_PATHS) {
        let v = server.call("brain.read", json!({ "paths": chunk })).await?;
        for n in v
            .get("notes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(p), Some(t)) = (
                n.get("path").and_then(Value::as_str),
                n.get("text").and_then(Value::as_str),
            ) {
                out.insert(p.to_string(), t.to_string());
            }
        }
    }
    Ok(out)
}

/// A note going to the server, with the hash the server held for it when
/// this sync listed it ("" for none): a server whose copy changed since
/// refuses it.
struct Put {
    path: String,
    text: String,
    expect: String,
}

/// What the server refused as stale (an old server refuses nothing).
fn stale_of(v: &Value) -> Vec<String> {
    v.get("stale")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

async fn push_there(server: &dyn NoteServer, notes: &[Put]) -> Result<Vec<String>, String> {
    let mut stale = vec![];
    let mut batch: Vec<Value> = vec![];
    let mut size = 0;
    for n in notes {
        if size + n.text.len() > BATCH_BYTES && !batch.is_empty() {
            let v = server.call("brain.apply", json!({ "put": batch })).await?;
            stale.extend(stale_of(&v));
            batch = vec![];
            size = 0;
        }
        size += n.text.len();
        batch.push(json!({ "path": n.path, "text": n.text, "expect": n.expect }));
    }
    if !batch.is_empty() {
        let v = server.call("brain.apply", json!({ "put": batch })).await?;
        stale.extend(stale_of(&v));
    }
    Ok(stale)
}

/// The hash of a note on disk now; None when it is gone.
fn hash_now(full: &Path) -> Option<String> {
    std::fs::read(full).ok().map(|b| fnv(&b))
}

/// Bring this device's brain and the server's into step. `server` names the
/// link (user@host) so a different server starts from no agreement.
pub async fn sync_brain(link: &Link, server: &str) -> Result<BrainReport, String> {
    let vault = Vault::open().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(vault.root()).map_err(|e| e.to_string())?;
    vault.claim_plain_folders();
    let base: BTreeMap<String, String> = load_state("brain.json", server);
    let (report, agreed) = sync_notes(&vault, link, &base).await?;
    save_state("brain.json", server, &agreed)?;
    Ok(report)
}

/// One sync of `vault` with `server` from the last agreement `base`: the
/// report, and the new agreement. The agreement is built path by path from
/// what both sides really held, never from a fresh listing: an edit made
/// here while the sync ran is not in it, so the next sync sends it on. A
/// path the sync could not settle (the server refused it, a note changed
/// under it) keeps its old agreement and is planned again next time.
pub async fn sync_notes(
    vault: &Vault,
    server: &dyn NoteServer,
    base: &BTreeMap<String, String>,
) -> Result<(BrainReport, BTreeMap<String, String>), String> {
    let device = parzi_core::device::id();
    let here = manifest(vault.root());
    let there: BTreeMap<String, String> = server
        .call("brain.manifest", json!({}))
        .await?
        .get("notes")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let steps = plan(&here, &there, base);
    let mut agreed: BTreeMap<String, String> = base
        .iter()
        .filter(|(p, _)| here.contains_key(*p) || there.contains_key(*p))
        .map(|(p, h)| (p.clone(), h.clone()))
        .collect();

    let mut report = BrainReport::default();
    let wanted: Vec<String> = steps
        .iter()
        .filter(|(_, a)| matches!(a, Act::Pull | Act::Conflict))
        .map(|(p, _)| p.clone())
        .collect();
    let fetched = read_there(server, &wanted).await?;
    let mut puts: Vec<Put> = vec![];
    let mut deletes: Vec<String> = vec![];
    let local =
        |p: &str| -> Result<PathBuf, String> { vault.resolve(p).map_err(|e| e.to_string()) };
    // A note edited here since the listing is left for the next sync.
    let untouched = |p: &str, full: &Path| hash_now(full).as_ref() == here.get(p);
    for (path, act) in &steps {
        match act {
            Act::Keep => {
                if let Some(h) = here.get(path) {
                    agreed.insert(path.clone(), h.clone());
                }
            }
            Act::Pull => {
                let Some(text) = fetched.get(path) else {
                    continue;
                };
                let full = local(path)?;
                if !untouched(path, &full) {
                    continue;
                }
                parzi_core::atomic_write(&full, text.as_bytes())
                    .map_err(|e| format!("{path}: {e}"))?;
                agreed.insert(path.clone(), fnv(text.as_bytes()));
                report.pulled += 1;
                report.changed.push(path.clone());
            }
            Act::Push => {
                let text = std::fs::read(local(path)?).map_err(|e| format!("{path}: {e}"))?;
                puts.push(Put {
                    path: path.clone(),
                    text: String::from_utf8_lossy(&text).into_owned(),
                    expect: there.get(path).cloned().unwrap_or_default(),
                });
                report.pushed += 1;
            }
            Act::DeleteHere => {
                let full = local(path)?;
                if !untouched(path, &full) {
                    continue;
                }
                let _ = std::fs::remove_file(&full);
                agreed.remove(path);
                report.deleted_here += 1;
                report.changed.push(path.clone());
            }
            Act::DeleteThere => {
                deletes.push(path.clone());
                report.deleted_there += 1;
            }
            Act::Conflict => {
                let Some(theirs) = fetched.get(path) else {
                    continue;
                };
                let full = local(path)?;
                let mine = std::fs::read(&full).map_err(|e| format!("{path}: {e}"))?;
                let taken = |p: &str| here.contains_key(p) || there.contains_key(p);
                let aside = conflict_path(path, &device, &taken);
                parzi_core::atomic_write(&local(&aside)?, &mine)
                    .map_err(|e| format!("{aside}: {e}"))?;
                parzi_core::atomic_write(&full, theirs.as_bytes())
                    .map_err(|e| format!("{path}: {e}"))?;
                agreed.insert(path.clone(), fnv(theirs.as_bytes()));
                puts.push(Put {
                    path: aside.clone(),
                    text: String::from_utf8_lossy(&mine).into_owned(),
                    expect: String::new(),
                });
                report.conflicts += 1;
                report.changed.push(path.clone());
                report.changed.push(aside);
            }
        }
    }
    let stale = push_there(server, &puts).await?;
    for p in &puts {
        if stale.contains(&p.path) {
            report.pushed = report.pushed.saturating_sub(1);
        } else {
            agreed.insert(p.path.clone(), fnv(p.text.as_bytes()));
        }
    }
    if !deletes.is_empty() {
        let expect: serde_json::Map<String, Value> = deletes
            .iter()
            .filter_map(|p| Some((p.clone(), json!(there.get(p)?))))
            .collect();
        let v = server
            .call(
                "brain.apply",
                json!({ "delete": deletes, "delete_expect": expect }),
            )
            .await?;
        let refused = stale_of(&v);
        for p in &deletes {
            if refused.contains(p) {
                report.deleted_there = report.deleted_there.saturating_sub(1);
            } else {
                agreed.remove(p);
            }
        }
    }
    Ok((report, agreed))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SettingsState {
    server_rev: u64,
    local_mtime: u64,
}

fn config_mtime() -> u64 {
    parzi_core::paths::config_path()
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[derive(Debug)]
pub enum SettingsOutcome {
    Same,
    Pushed,
    /// The merged config now on disk; the caller applies it to its engine.
    Pulled(Box<ParziConfig>),
}

/// Which way settings move. `None` state means never synced with this server.
#[must_use]
pub fn settings_direction(
    local_mtime: u64,
    server_rev: u64,
    last: Option<(u64, u64)>,
) -> Option<bool> {
    // Some(true) = push, Some(false) = pull, None = nothing to do.
    let Some((last_rev, last_mtime)) = last else {
        // First contact: the server is home once anyone has set it.
        return Some(server_rev == 0);
    };
    let local_changed = local_mtime != last_mtime;
    let remote_changed = server_rev != last_rev;
    match (local_changed, remote_changed) {
        (false, false) => None,
        (true, false) => Some(true),
        (false, true) => Some(false),
        (true, true) => Some(local_mtime > server_rev),
    }
}

pub async fn sync_settings(link: &Link, server: &str) -> Result<SettingsOutcome, String> {
    let v = link.call("settings.get", json!({}), CALL).await?;
    let rev = v.get("rev").and_then(Value::as_u64).unwrap_or(0);
    let raw_state = sync_dir()
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok());
    let known = raw_state
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(|w| w.get("server").and_then(Value::as_str) == Some(server))
        .and_then(|w| serde_json::from_value::<SettingsState>(w.get("state")?.clone()).ok())
        .map(|s| (s.server_rev, s.local_mtime));
    let mtime = config_mtime();
    match settings_direction(mtime, rev, known) {
        None => Ok(SettingsOutcome::Same),
        Some(true) => {
            let local = ParziConfig::load().map_err(|e| e.to_string())?;
            let put = link
                .call("settings.put", json!({ "config": portable(local) }), CALL)
                .await?;
            let rev = put.get("rev").and_then(Value::as_u64).unwrap_or(rev);
            save_state(
                "settings.json",
                server,
                &SettingsState {
                    server_rev: rev,
                    local_mtime: mtime,
                },
            )?;
            Ok(SettingsOutcome::Pushed)
        }
        Some(false) => {
            let incoming: ParziConfig = v
                .get("config")
                .cloned()
                .and_then(|c| serde_json::from_value(c).ok())
                .ok_or("the server sent no settings")?;
            let local = ParziConfig::load().unwrap_or_default();
            let merged = merge_config(local, incoming);
            merged.save().map_err(|e| e.to_string())?;
            save_state(
                "settings.json",
                server,
                &SettingsState {
                    server_rev: rev,
                    local_mtime: config_mtime(),
                },
            )?;
            Ok(SettingsOutcome::Pulled(Box::new(merged)))
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Mirrored {
    events: usize,
    updated: String,
}

/// Send this device's sessions to the server's mirror: new events only,
/// and a delete for sessions gone here. Returns how many sessions moved.
pub async fn mirror_sessions(
    link: &Link,
    store: &SessionStore,
    server: &str,
) -> Result<usize, String> {
    let me = parzi_core::device::this();
    let mut state: BTreeMap<String, Mirrored> = load_state("mirror.json", server);
    let all = store.list().map_err(|e| e.to_string())?;
    let mut moved = 0;
    for meta in &all {
        let count = store.event_count(&meta.id).unwrap_or(0);
        let updated = meta.updated.to_rfc3339();
        let prev = state.get(&meta.id).cloned().unwrap_or_default();
        if prev.events == count && prev.updated == updated {
            continue;
        }
        let from = if prev.events <= count { prev.events } else { 0 };
        let events = store
            .events_from(&meta.id, from)
            .map_err(|e| e.to_string())?;
        let mut start = from;
        let mut chunk: Vec<Value> = vec![];
        let mut size = 0;
        let flush = |start: usize, chunk: Vec<Value>| {
            json!({
                "device": me.id, "name": me.name, "session": meta,
                "from": start, "events": chunk,
            })
        };
        // The server answers how many events it holds; when that is not
        // where this chunk ended, stop: the next sync resends from there.
        let mut held = None;
        for ev in events {
            let v = serde_json::to_value(&ev).unwrap_or(Value::Null);
            let n = v.to_string().len();
            if size + n > BATCH_BYTES && !chunk.is_empty() {
                let sent = std::mem::take(&mut chunk);
                let end = start + sent.len();
                let have = put_mirror(link, flush(start, sent)).await?;
                size = 0;
                if have != end {
                    held = Some(have);
                    break;
                }
                start = end;
            }
            size += n;
            chunk.push(v);
        }
        let have = match held {
            Some(h) => h,
            None => put_mirror(link, flush(start, chunk)).await?,
        };
        state.insert(
            meta.id.clone(),
            Mirrored {
                events: have.min(count),
                updated,
            },
        );
        moved += 1;
    }
    let live: BTreeSet<&str> = all.iter().map(|m| m.id.as_str()).collect();
    let gone: Vec<String> = state
        .keys()
        .filter(|id| !live.contains(id.as_str()))
        .cloned()
        .collect();
    for id in gone {
        link.call("mirror.delete", json!({ "device": me.id, "id": id }), CALL)
            .await?;
        state.remove(&id);
        moved += 1;
    }
    save_state("mirror.json", server, &state)?;
    Ok(moved)
}

#[derive(Debug)]
pub struct Synced {
    pub settings: SettingsOutcome,
    pub brain: BrainReport,
    pub sessions: usize,
    /// A part that failed does not stop the others; each says why here.
    pub errors: Vec<String>,
}

impl Synced {
    /// "3 notes in, 1 out, settings from the server" or "" when nothing moved.
    #[must_use]
    pub fn summary(&self) -> String {
        let b = &self.brain;
        let mut parts = vec![];
        if b.pulled + b.deleted_here > 0 {
            parts.push(format!("{} notes in", b.pulled + b.deleted_here));
        }
        if b.pushed + b.deleted_there > 0 {
            parts.push(format!("{} notes out", b.pushed + b.deleted_there));
        }
        if b.conflicts > 0 {
            parts.push(format!(
                "{} notes changed on both sides, both kept",
                b.conflicts
            ));
        }
        match self.settings {
            SettingsOutcome::Pulled(_) => parts.push("settings from the server".into()),
            SettingsOutcome::Pushed => parts.push("settings to the server".into()),
            SettingsOutcome::Same => {}
        }
        if self.sessions > 0 {
            parts.push(format!("{} sessions mirrored", self.sessions));
        }
        parts.join(", ")
    }
}

/// Settings, then notes, then this device's sessions.
pub async fn everything(link: &Link, store: &SessionStore, server: &str) -> Synced {
    let mut errors = vec![];
    let settings = sync_settings(link, server).await.unwrap_or_else(|e| {
        errors.push(format!("settings: {e}"));
        SettingsOutcome::Same
    });
    let brain = sync_brain(link, server).await.unwrap_or_else(|e| {
        errors.push(format!("brain: {e}"));
        BrainReport::default()
    });
    let sessions = mirror_sessions(link, store, server)
        .await
        .unwrap_or_else(|e| {
            errors.push(format!("sessions: {e}"));
            0
        });
    Synced {
        settings,
        brain,
        sessions,
        errors,
    }
}

/// One `mirror.put`; the server answers how many events it now holds.
async fn put_mirror(link: &Link, body: Value) -> Result<usize, String> {
    let v = link.call("mirror.put", body, CALL).await?;
    Ok(v.get("have")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| ((*a).into(), (*b).into()))
            .collect()
    }

    fn act(steps: &[(String, Act)], path: &str) -> Act {
        steps
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, a)| *a)
            .unwrap()
    }

    #[test]
    fn three_way_plan_moves_each_change_the_right_way() {
        let base = map(&[
            ("same", "1"),
            ("mine", "1"),
            ("theirs", "1"),
            ("both", "1"),
            ("gone-there", "1"),
            ("gone-here", "1"),
            ("edit-vs-gone", "1"),
        ]);
        let here = map(&[
            ("same", "1"),
            ("mine", "2"),
            ("theirs", "1"),
            ("both", "2"),
            ("gone-there", "1"),
            ("edit-vs-gone", "2"),
            ("new-here", "9"),
        ]);
        let there = map(&[
            ("same", "1"),
            ("mine", "1"),
            ("theirs", "3"),
            ("both", "3"),
            ("gone-here", "1"),
            ("new-there", "8"),
        ]);
        let s = plan(&here, &there, &base);
        assert_eq!(act(&s, "same"), Act::Keep);
        assert_eq!(act(&s, "mine"), Act::Push);
        assert_eq!(act(&s, "theirs"), Act::Pull);
        assert_eq!(act(&s, "both"), Act::Conflict);
        assert_eq!(act(&s, "gone-there"), Act::DeleteHere);
        assert_eq!(act(&s, "gone-here"), Act::DeleteThere);
        assert_eq!(act(&s, "edit-vs-gone"), Act::Push, "an edit beats a delete");
        assert_eq!(act(&s, "new-here"), Act::Push);
        assert_eq!(act(&s, "new-there"), Act::Pull);
    }

    #[test]
    fn first_sync_merges_and_marks_differences_as_conflicts() {
        let s = plan(
            &map(&[("a", "1"), ("b", "2")]),
            &map(&[("a", "1"), ("b", "3"), ("c", "4")]),
            &BTreeMap::new(),
        );
        assert_eq!(act(&s, "a"), Act::Keep);
        assert_eq!(act(&s, "b"), Act::Conflict);
        assert_eq!(act(&s, "c"), Act::Pull);
    }

    #[test]
    fn conflict_copies_never_overwrite() {
        let taken = |p: &str| p == "n/a.conflict-pc.md";
        assert_eq!(
            conflict_path("n/a.md", "pc", &taken),
            "n/a.conflict-pc-2.md"
        );
        assert_eq!(conflict_path("b.md", "pc", &|_| false), "b.conflict-pc.md");
    }

    #[test]
    fn settings_go_to_the_newer_side_and_home_wins_at_first_contact() {
        assert_eq!(
            settings_direction(5, 0, None),
            Some(true),
            "empty server: push"
        );
        assert_eq!(
            settings_direction(5, 7, None),
            Some(false),
            "server set: pull"
        );
        assert_eq!(settings_direction(5, 7, Some((7, 5))), None);
        assert_eq!(settings_direction(6, 7, Some((7, 5))), Some(true));
        assert_eq!(settings_direction(5, 8, Some((7, 5))), Some(false));
        assert_eq!(
            settings_direction(9, 8, Some((7, 5))),
            Some(true),
            "both: newer wins"
        );
        assert_eq!(settings_direction(6, 8, Some((7, 5))), Some(false));
    }

    type Hook = Box<dyn FnOnce() + Send>;

    /// A server over a second vault, answering as `parzi serve` does. A
    /// hook runs once just before the server answers its op: something
    /// that happens while a sync is under way.
    struct Server {
        vault: Vault,
        hooks: std::sync::Mutex<Vec<(&'static str, Hook)>>,
    }

    impl Server {
        fn new(root: &Path) -> Self {
            Self {
                vault: Vault::at(root),
                hooks: std::sync::Mutex::default(),
            }
        }

        fn on(&self, op: &'static str, f: impl FnOnce() + Send + 'static) {
            self.hooks.lock().unwrap().push((op, Box::new(f)));
        }
    }

    #[async_trait::async_trait]
    impl NoteServer for Server {
        async fn call(&self, op: &str, body: Value) -> Result<Value, String> {
            let hook = {
                let mut hooks = self.hooks.lock().unwrap();
                hooks
                    .iter()
                    .position(|(o, _)| *o == op)
                    .map(|at| hooks.remove(at).1)
            };
            if let Some(h) = hook {
                h();
            }
            Ok(match op {
                "brain.manifest" => crate::hub::brain_manifest_at(&self.vault),
                "brain.read" => crate::hub::brain_read_at(&self.vault, &body),
                "brain.apply" => crate::hub::brain_apply_at(&self.vault, &body),
                _ => return Err(format!("unknown op `{op}`")),
            })
        }
    }

    fn put(root: &Path, path: &str, text: &str) {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }

    fn get(root: &Path, path: &str) -> Option<String> {
        std::fs::read_to_string(root.join(path)).ok()
    }

    #[tokio::test]
    async fn an_edit_made_during_a_sync_survives_the_next_one() {
        let (h, t) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (here, there) = (h.path().to_path_buf(), t.path().to_path_buf());
        put(&here, "a.md", "v1");
        put(&here, "c.md", "same");
        put(&there, "c.md", "same");
        put(&there, "projects/p/plan.md", "theirs");
        let server = Server::new(&there);
        // While the notes go out, the person (or an agent) edits two here.
        let (a, c) = (here.join("a.md"), here.join("c.md"));
        server.on("brain.apply", move || {
            std::fs::write(&a, "v2").unwrap();
            std::fs::write(&c, "edited").unwrap();
        });
        let vault = Vault::at(&here);
        let (report, agreed) = sync_notes(&vault, &server, &BTreeMap::new()).await.unwrap();
        assert_eq!((report.pushed, report.pulled), (1, 1));
        assert_eq!(report.changed, ["projects/p/plan.md"]);
        assert_eq!(
            agreed["a.md"],
            fnv(b"v1"),
            "what was sent, not what is there now"
        );
        assert_eq!(agreed["c.md"], fnv(b"same"));
        assert_eq!(get(&there, "a.md").as_deref(), Some("v1"));

        let (report, agreed) = sync_notes(&vault, &server, &agreed).await.unwrap();
        assert_eq!(report.pushed, 2, "both edits go out");
        assert_eq!(report.pulled, 0, "nothing comes back over them");
        assert_eq!(get(&there, "a.md").as_deref(), Some("v2"));
        assert_eq!(get(&there, "c.md").as_deref(), Some("edited"));
        assert_eq!(get(&here, "c.md").as_deref(), Some("edited"));
        assert_eq!(agreed["c.md"], fnv(b"edited"));
    }

    #[tokio::test]
    async fn a_pull_never_lands_on_an_edit_made_meanwhile() {
        let (h, t) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (here, there) = (h.path().to_path_buf(), t.path().to_path_buf());
        put(&here, "n.md", "orig");
        put(&there, "n.md", "server edit");
        let base = BTreeMap::from([("n.md".to_string(), fnv(b"orig"))]);
        let server = Server::new(&there);
        let n = here.join("n.md");
        server.on("brain.read", move || {
            std::fs::write(&n, "local edit").unwrap()
        });
        let vault = Vault::at(&here);
        let (report, agreed) = sync_notes(&vault, &server, &base).await.unwrap();
        assert_eq!(report.pulled, 0);
        assert_eq!(get(&here, "n.md").as_deref(), Some("local edit"));
        assert_eq!(agreed["n.md"], fnv(b"orig"), "planned again next time");
        let (report, _) = sync_notes(&vault, &server, &agreed).await.unwrap();
        assert_eq!(report.conflicts, 1, "both changed: both kept");
        assert_eq!(get(&here, "n.md").as_deref(), Some("server edit"));
        let device = parzi_core::device::id();
        let aside = format!("n.conflict-{device}.md");
        assert_eq!(get(&here, &aside).as_deref(), Some("local edit"));
        assert_eq!(get(&there, &aside).as_deref(), Some("local edit"));
    }

    #[tokio::test]
    async fn the_server_refuses_a_note_that_changed_since_it_was_listed() {
        let t = tempfile::tempdir().unwrap();
        let there = t.path().to_path_buf();
        put(&there, "n.md", "orig");
        let v = Vault::at(&there);
        let apply = |body: Value| crate::hub::brain_apply_at(&v, &body);
        let r = apply(json!({"put": [{"path": "n.md", "text": "x", "expect": fnv(b"old")}]}));
        assert_eq!(r["stale"], json!(["n.md"]));
        assert_eq!(r["put"], 0);
        assert_eq!(get(&there, "n.md").as_deref(), Some("orig"));
        let r = apply(json!({"put": [{"path": "n.md", "text": "x", "expect": ""}]}));
        assert_eq!(r["stale"], json!(["n.md"]), "expected new, but it is there");
        let r = apply(json!({"put": [{"path": "n.md", "text": "y", "expect": fnv(b"orig")}]}));
        assert_eq!(
            (r["put"].as_u64(), r["stale"].clone()),
            (Some(1), json!([]))
        );
        let r = apply(json!({"put": [{"path": "n.md", "text": "z"}]}));
        assert_eq!(r["put"], 1, "a device from before is not checked");
        assert_eq!(get(&there, "n.md").as_deref(), Some("z"));
        let r = apply(json!({"delete": ["n.md"], "delete_expect": {"n.md": fnv(b"y")}}));
        assert_eq!(r["stale"], json!(["n.md"]));
        assert!(
            get(&there, "n.md").is_some(),
            "a changed note is not deleted"
        );

        // In a sync: another device's edit lands between the listing and
        // the push. The push is refused; the next sync keeps both sides.
        let h = tempfile::tempdir().unwrap();
        let here = h.path().to_path_buf();
        put(&there, "m.md", "orig");
        put(&here, "m.md", "mine");
        let base = BTreeMap::from([("m.md".to_string(), fnv(b"orig"))]);
        let server = Server::new(&there);
        let m = there.join("m.md");
        server.on("brain.apply", move || std::fs::write(&m, "theirs").unwrap());
        let vault = Vault::at(&here);
        let (report, agreed) = sync_notes(&vault, &server, &base).await.unwrap();
        assert_eq!(report.pushed, 0, "refused");
        assert_eq!(get(&there, "m.md").as_deref(), Some("theirs"));
        assert_eq!(agreed.get("m.md"), base.get("m.md"));
        let (report, _) = sync_notes(&vault, &server, &agreed).await.unwrap();
        assert_eq!(report.conflicts, 1);
        assert_eq!(get(&here, "m.md").as_deref(), Some("theirs"));
        let aside = format!("m.conflict-{}.md", parzi_core::device::id());
        assert_eq!(get(&there, &aside).as_deref(), Some("mine"), "nothing lost");
    }

    #[test]
    fn manifests_hash_notes_and_skip_the_rest() {
        let d = std::env::temp_dir().join(format!("parzi-sync-manifest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("projects")).unwrap();
        std::fs::create_dir_all(d.join(".obsidian")).unwrap();
        std::fs::write(d.join("a.md"), "x").unwrap();
        std::fs::write(d.join("projects").join("p.md"), "y").unwrap();
        std::fs::write(d.join(".obsidian").join("w.md"), "z").unwrap();
        std::fs::write(d.join("pic.png"), "p").unwrap();
        let m = manifest(&d);
        assert_eq!(
            m.keys().cloned().collect::<Vec<_>>(),
            ["a.md", "projects/p.md"]
        );
        assert_eq!(m["a.md"], fnv(b"x"));
        assert_ne!(fnv(b"x"), fnv(b"y"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
