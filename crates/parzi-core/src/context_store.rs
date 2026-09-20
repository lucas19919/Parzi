use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};

pub const MAX_WS_FILES: usize = 32;
pub const MAX_WS_BYTES: u64 = 131_072;
pub const MAX_PROJ_FILES: usize = 16;
pub const MAX_PROJ_BYTES: u64 = 65_536;
pub const MAX_FILE_BYTES: u64 = 16_384;
pub const MAX_AUTO_FILES: usize = 8;
pub const AUTO_TTL_SECS: i64 = 14 * 86_400;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Pinned,
    Curated,
    Auto,
}

impl Tier {
    fn rank(self) -> u8 {
        match self {
            Tier::Pinned => 0,
            Tier::Curated => 1,
            Tier::Auto => 2,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Workspace,
    Project,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestFile {
    pub name: String,
    pub tier: Tier,
    #[serde(default)]
    pub source: String,
    pub bytes: u64,
    pub updated: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Manifest {
    version: u8,
    #[serde(default)]
    files: Vec<ManifestFile>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: 1,
            files: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ContextDoc {
    pub name: String,
    pub tier: Tier,
    pub scope: Scope,
    pub content: String,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().cast_signed())
}

fn hash_bytes(b: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

#[must_use]
pub fn ws_dir(home: &Path, workspace: &str) -> PathBuf {
    home.join("workspaces")
        .join(crate::workspace::sanitize(workspace))
        .join("context")
}

#[must_use]
pub fn proj_dir(home: &Path, workspace: &str, slug: &str) -> PathBuf {
    home.join("workspaces")
        .join(crate::workspace::sanitize(workspace))
        .join("projects")
        .join(crate::project::slug_of(slug))
        .join("context")
}

fn sanitize_file(name: &str) -> Option<String> {
    let base = name.trim().replace('\\', "/");
    let base = base.rsplit('/').next().unwrap_or("").trim();
    let stem = base.strip_suffix(".md").unwrap_or(base);
    if stem.is_empty() || stem.len() > 48 {
        return None;
    }
    if stem == "." || stem == ".." {
        return None;
    }
    if !stem
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(format!("{stem}.md"))
}

fn read_manifest(dir: &Path) -> Manifest {
    let raw = std::fs::read_to_string(dir.join("manifest.toml")).unwrap_or_default();
    if raw.is_empty() {
        return Manifest::default();
    }
    toml::from_str(&raw).unwrap_or_default()
}

fn write_manifest(dir: &Path, m: &Manifest) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let body = toml::to_string(m).map_err(ParziError::TomlSer)?;
    let tmp = dir.join(format!("manifest.{}.tmp", std::process::id()));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, dir.join("manifest.toml"))?;
    Ok(())
}

fn total_bytes(m: &Manifest) -> u64 {
    m.files.iter().map(|f| f.bytes).sum()
}

fn prune_expired(dir: &Path, m: &mut Manifest, now: i64) {
    let expired: Vec<String> = m
        .files
        .iter()
        .filter(|f| f.tier == Tier::Auto && now.saturating_sub(f.updated) > AUTO_TTL_SECS)
        .map(|f| f.name.clone())
        .collect();
    if expired.is_empty() {
        return;
    }
    for name in &expired {
        let _ = std::fs::remove_file(dir.join(name));
    }
    m.files.retain(|f| !expired.iter().any(|e| e == &f.name));
}

fn enforce_caps(dir: &Path, m: &mut Manifest, max_files: usize, max_bytes: u64) {
    m.files.sort_by_key(|f| (f.tier.rank(), f.updated));
    while m.files.len() > max_files || total_bytes(m) > max_bytes {
        let pos =
            m.files
                .iter()
                .rposition(|f| f.tier != Tier::Pinned)
                .or(if m.files.len() > max_files {
                    Some(0)
                } else {
                    None
                });
        let Some(i) = pos else { break };
        let gone = m.files.remove(i);
        let _ = std::fs::remove_file(dir.join(&gone.name));
    }
    let auto_over = m
        .files
        .iter()
        .filter(|f| f.tier == Tier::Auto)
        .count()
        .saturating_sub(MAX_AUTO_FILES);
    for _ in 0..auto_over {
        let oldest = m
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.tier == Tier::Auto)
            .min_by_key(|(_, f)| f.updated)
            .map(|(i, _)| i);
        let Some(i) = oldest else { break };
        let gone = m.files.remove(i);
        let _ = std::fs::remove_file(dir.join(&gone.name));
    }
}

fn read_set(dir: &Path, scope: Scope, max_files: usize, max_bytes: u64, out: &mut Vec<ContextDoc>) {
    if !dir.is_dir() {
        return;
    }
    let mut m = read_manifest(dir);
    prune_expired(dir, &mut m, now_unix());
    enforce_caps(dir, &mut m, max_files, max_bytes);
    let mut files = m.files.clone();
    files.sort_by_key(|f| (f.tier.rank(), f.name.clone()));
    for f in files {
        if out.iter().any(|d| d.name == f.name) {
            continue;
        }
        let content = std::fs::read_to_string(dir.join(&f.name)).unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }
        out.push(ContextDoc {
            name: f.name,
            tier: f.tier,
            scope,
            content,
        });
    }
    let _ = write_manifest(dir, &m);
}

fn seed_workspace(home: &Path, workspace: &str) {
    let dir = ws_dir(home, workspace);
    if dir.join("manifest.toml").is_file() {
        return;
    }
    let sys = home
        .join("workspaces")
        .join(crate::workspace::sanitize(workspace))
        .join("SYSTEM.md");
    let Ok(content) = std::fs::read_to_string(&sys) else {
        return;
    };
    if content.trim().is_empty() {
        return;
    }
    let _ = add(
        home,
        workspace,
        None,
        "system",
        &content,
        Tier::Pinned,
        "seed",
    );
}

/// Workspace set, then the deck project's set (project names never shadow
/// workspace names). Seeds from `SYSTEM.md` on first read.
///
/// # Errors
///
/// Rejects a blank workspace name; unreadable files are skipped, never fatal.
pub fn list(home: &Path, workspace: &str, slug: Option<&str>) -> Result<Vec<ContextDoc>> {
    if workspace.trim().is_empty() {
        return Err(ParziError::Config("workspace is required".into()));
    }
    seed_workspace(home, workspace);
    let mut out = Vec::new();
    read_set(
        &ws_dir(home, workspace),
        Scope::Workspace,
        MAX_WS_FILES,
        MAX_WS_BYTES,
        &mut out,
    );
    if let Some(slug) = slug {
        if !slug.trim().is_empty() {
            let dir = proj_dir(home, workspace, slug);
            if dir.is_dir() {
                let mut proj = Vec::new();
                read_set(
                    &dir,
                    Scope::Project,
                    MAX_PROJ_FILES,
                    MAX_PROJ_BYTES,
                    &mut proj,
                );
                for d in proj {
                    if out.iter().any(|e| e.name == d.name) {
                        continue;
                    }
                    out.push(d);
                }
            }
        }
    }
    Ok(out)
}

fn target_set(home: &Path, workspace: &str, slug: Option<&str>) -> (PathBuf, Scope, usize, u64) {
    match slug {
        Some(s) if !s.trim().is_empty() => (
            proj_dir(home, workspace, s),
            Scope::Project,
            MAX_PROJ_FILES,
            MAX_PROJ_BYTES,
        ),
        _ => (
            ws_dir(home, workspace),
            Scope::Workspace,
            MAX_WS_FILES,
            MAX_WS_BYTES,
        ),
    }
}

fn duplicate_of(dir: &Path, m: &Manifest, clean: &str, digest: u64) -> bool {
    m.files.iter().any(|f| {
        f.name == clean && std::fs::read(dir.join(&f.name)).is_ok_and(|b| hash_bytes(&b) == digest)
    })
}

fn upsert_entry(m: &mut Manifest, clean: &str, tier: Tier, source: &str, bytes: u64, now: i64) {
    match m.files.iter_mut().find(|f| f.name == clean) {
        Some(f) => {
            f.tier = tier;
            f.source = source.to_string();
            f.bytes = bytes;
            f.updated = now;
        }
        None => m.files.push(ManifestFile {
            name: clean.to_string(),
            tier,
            source: source.to_string(),
            bytes,
            updated: now,
        }),
    }
}

struct NewFile<'a> {
    workspace: &'a str,
    slug: Option<&'a str>,
    name: &'a str,
    title: &'a str,
    content: &'a str,
    tier: Tier,
    source: &'a str,
}

fn write_one(home: &Path, op: &NewFile) -> Result<ContextDoc> {
    let clean = sanitize_file(op.name)
        .or_else(|| sanitize_file(op.title))
        .ok_or_else(|| ParziError::Config("context name must be [A-Za-z0-9_-], .md".into()))?;
    let bytes = op.content.as_bytes();
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(ParziError::Config(format!(
            "context file over {MAX_FILE_BYTES} bytes"
        )));
    }
    if op.content.trim().is_empty() {
        return Err(ParziError::Config("context file is empty".into()));
    }
    let (dir, scope, max_files, max_bytes) = target_set(home, op.workspace, op.slug);
    std::fs::create_dir_all(&dir)?;
    let mut m = read_manifest(&dir);
    if duplicate_of(&dir, &m, &clean, hash_bytes(bytes)) {
        return Err(ParziError::Config("identical context file exists".into()));
    }
    let target = dir.join(&clean);
    let tmp = dir.join(format!(".{clean}.tmp"));
    std::fs::write(&tmp, op.content)?;
    std::fs::rename(&tmp, &target)?;
    upsert_entry(
        &mut m,
        &clean,
        op.tier,
        op.source,
        bytes.len() as u64,
        now_unix(),
    );
    enforce_caps(&dir, &mut m, max_files, max_bytes);
    if !m.files.iter().any(|f| f.name == clean) {
        let _ = std::fs::remove_file(&target);
        return Err(ParziError::Config("context budget full".into()));
    }
    write_manifest(&dir, &m)?;
    Ok(ContextDoc {
        name: clean,
        tier: op.tier,
        scope,
        content: op.content.to_string(),
    })
}

/// Add or replace one context file.
///
/// # Errors
///
/// Rejects blank workspaces, unsafe names, empty or oversized content,
/// exact duplicates, and writes that do not fit the tier budget.
pub fn add(
    home: &Path,
    workspace: &str,
    slug: Option<&str>,
    title: &str,
    content: &str,
    tier: Tier,
    source: &str,
) -> Result<ContextDoc> {
    if workspace.trim().is_empty() {
        return Err(ParziError::Config("workspace is required".into()));
    }
    write_one(
        home,
        &NewFile {
            workspace,
            slug,
            name: title,
            title,
            content,
            tier,
            source,
        },
    )
}

/// Flip a curated file to pinned (always loads) or back.
///
/// # Errors
///
/// Rejects unknown files; `auto` files cannot be pinned.
pub fn set_pinned(
    home: &Path,
    workspace: &str,
    slug: Option<&str>,
    name: &str,
    pinned: bool,
) -> Result<()> {
    let clean =
        sanitize_file(name).ok_or_else(|| ParziError::Config("unknown context file".into()))?;
    let dir = match slug {
        Some(s) if !s.trim().is_empty() => proj_dir(home, workspace, s),
        _ => ws_dir(home, workspace),
    };
    let mut m = read_manifest(&dir);
    let Some(f) = m.files.iter_mut().find(|f| f.name == clean) else {
        return Err(ParziError::Config("unknown context file".into()));
    };
    if f.tier == Tier::Auto {
        return Err(ParziError::Config("auto context cannot be pinned".into()));
    }
    f.tier = if pinned { Tier::Pinned } else { Tier::Curated };
    f.updated = now_unix();
    write_manifest(&dir, &m)
}

/// Drop one context file and its manifest row.
///
/// # Errors
///
/// Rejects unknown files.
pub fn remove(home: &Path, workspace: &str, slug: Option<&str>, name: &str) -> Result<()> {
    let clean =
        sanitize_file(name).ok_or_else(|| ParziError::Config("unknown context file".into()))?;
    let dir = match slug {
        Some(s) if !s.trim().is_empty() => proj_dir(home, workspace, s),
        _ => ws_dir(home, workspace),
    };
    let mut m = read_manifest(&dir);
    let Some(pos) = m.files.iter().position(|f| f.name == clean) else {
        return Err(ParziError::Config("unknown context file".into()));
    };
    m.files.remove(pos);
    let _ = std::fs::remove_file(dir.join(&clean));
    write_manifest(&dir, &m)
}

#[must_use]
pub fn injection_text(home: &Path, workspace: &str, slug: Option<&str>, budget: usize) -> String {
    let Ok(docs) = list(home, workspace, slug) else {
        return String::new();
    };
    let mut out = String::new();
    let mut used = 0usize;
    for d in docs {
        let scope = match d.scope {
            Scope::Workspace => "workspace",
            Scope::Project => "project",
        };
        let head = format!("# Context ({scope}, {name})\n\n", name = d.name);
        let room = budget.saturating_sub(used);
        let take = d
            .content
            .chars()
            .take(room.saturating_sub(head.len()))
            .collect::<String>();
        if take.trim().is_empty() {
            continue;
        }
        out.push_str(&head);
        out.push_str(&take);
        out.push_str("\n\n");
        used += head.len() + take.len();
        if used >= budget {
            break;
        }
    }
    out
}
