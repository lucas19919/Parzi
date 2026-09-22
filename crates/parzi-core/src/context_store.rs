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
pub struct ManifestRef {
    pub label: String,
    pub path: String,
    #[serde(default)]
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Manifest {
    version: u8,
    #[serde(default)]
    files: Vec<ManifestFile>,
    /// Pinned references to canonical docs (plans, specs): read live, never
    /// copied, so plans cannot sprawl or go stale.
    #[serde(default)]
    refs: Vec<ManifestRef>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: 1,
            files: Vec::new(),
            refs: Vec::new(),
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

#[derive(Clone, Debug)]
pub struct ContextRef {
    pub label: String,
    pub path: String,
    pub kind: String,
}

fn ws_root(home: &Path, workspace: &str) -> PathBuf {
    home.join("workspaces")
        .join(crate::workspace::sanitize(workspace))
}

/// Resolve a ref path against the workspace root. Refuses escapes, missing
/// files, oversized files and non-markdown: a ref is a pointer, not storage.
fn resolve_ref(home: &Path, workspace: &str, rel: &str) -> Option<PathBuf> {
    let root = ws_root(home, workspace);
    let clean = rel.trim().replace('\\', "/");
    if clean.is_empty() || clean.len() > 128 {
        return None;
    }
    if clean.split('/').any(|c| {
        c.is_empty()
            || c == "."
            || c == ".."
            || c.len() > 64
            || !c
                .chars()
                .all(|x| x.is_ascii_alphanumeric() || "-_. ".contains(x))
    }) {
        return None;
    }
    if !clean.to_lowercase().ends_with(".md") {
        return None;
    }
    let abs = root.join(&clean);
    let meta = std::fs::symlink_metadata(&abs).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    Some(abs)
}

/// Live canonical docs pinned by reference. Missing or invalid targets are
/// skipped, never fatal.
#[must_use]
pub fn list_refs(home: &Path, workspace: &str, slug: Option<&str>) -> Vec<ContextRef> {
    if workspace.trim().is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut push_set = |dir: PathBuf| {
        let (m, _) = read_manifest(&dir);
        for r in m.refs {
            if !seen.insert(r.path.clone()) {
                continue;
            }
            if resolve_ref(home, workspace, &r.path).is_some() {
                out.push(ContextRef {
                    label: if r.label.trim().is_empty() {
                        r.path.clone()
                    } else {
                        r.label.clone()
                    },
                    path: ws_root(home, workspace)
                        .join(&r.path)
                        .to_string_lossy()
                        .to_string(),
                    kind: if r.kind.trim().is_empty() {
                        "doc".to_string()
                    } else {
                        r.kind.clone()
                    },
                });
            }
        }
    };
    push_set(ws_dir(home, workspace));
    if let Some(s) = slug {
        if !s.trim().is_empty() {
            push_set(proj_dir(home, workspace, s));
        }
    }
    out
}

/// Pin a live reference to a canonical doc.
///
/// # Errors
///
/// Rejects blank workspaces, missing or unsafe targets, and duplicates.
pub fn add_ref(
    home: &Path,
    workspace: &str,
    slug: Option<&str>,
    label: &str,
    path: &str,
    kind: &str,
) -> Result<()> {
    if workspace.trim().is_empty() {
        return Err(ParziError::Config("workspace is required".into()));
    }
    if resolve_ref(home, workspace, path).is_none() {
        return Err(ParziError::Config("ref target missing or unsafe".into()));
    }
    let dir = match slug {
        Some(s) if !s.trim().is_empty() => proj_dir(home, workspace, s),
        _ => ws_dir(home, workspace),
    };
    std::fs::create_dir_all(&dir)?;
    let (mut m, loaded_ok) = read_manifest(&dir);
    if !loaded_ok {
        repair(&dir, &mut m);
    }
    let rel = path.trim().replace('\\', "/");
    if m.refs.iter().any(|r| r.path == rel) {
        return Err(ParziError::Config("ref already pinned".into()));
    }
    m.refs.push(ManifestRef {
        label: sanitize_label(label, 48),
        path: rel,
        kind: sanitize_label(kind, 16),
    });
    write_manifest(&dir, &m)
}

/// Drop a pinned reference. The target file is untouched.
///
/// # Errors
///
/// Rejects blank workspaces and unknown refs.
pub fn remove_ref(home: &Path, workspace: &str, slug: Option<&str>, path: &str) -> Result<()> {
    if workspace.trim().is_empty() {
        return Err(ParziError::Config("workspace is required".into()));
    }
    let dir = match slug {
        Some(s) if !s.trim().is_empty() => proj_dir(home, workspace, s),
        _ => ws_dir(home, workspace),
    };
    let (mut m, loaded_ok) = read_manifest(&dir);
    if !loaded_ok {
        repair(&dir, &mut m);
    }
    let rel = path.trim().replace('\\', "/");
    let Some(pos) = m.refs.iter().position(|r| r.path == rel) else {
        return Err(ParziError::Config("unknown ref".into()));
    };
    m.refs.remove(pos);
    write_manifest(&dir, &m)
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

fn sanitize_label(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || "-_.,:;!?()[]'\" ".contains(*c))
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

fn read_manifest(dir: &Path) -> (Manifest, bool) {
    let raw = std::fs::read_to_string(dir.join("manifest.toml")).unwrap_or_default();
    if raw.is_empty() {
        return (Manifest::default(), !dir.join("manifest.toml").exists());
    }
    match toml::from_str(&raw) {
        Ok(m) => (m, true),
        Err(_) => (Manifest::default(), false),
    }
}

/// A corrupt or missing manifest must never orphan files: adopt any
/// well-named `.md` on disk as curated so nothing silently disappears.
fn repair(dir: &Path, m: &mut Manifest) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(clean) = sanitize_file(&name) else {
            continue;
        };
        if clean != name || m.files.iter().any(|f| f.name == clean) {
            continue;
        }
        let bytes = e.metadata().map_or(0, |x| x.len());
        m.files.push(ManifestFile {
            name: clean,
            tier: Tier::Curated,
            source: "recovered".into(),
            bytes,
            updated: now_unix(),
        });
    }
}

static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn write_manifest(dir: &Path, m: &Manifest) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let body = toml::to_string(m).map_err(ParziError::TomlSer)?;
    let unique = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(
        "manifest.{}.{}.{}.tmp",
        std::process::id(),
        now_unix(),
        unique
    ));
    std::fs::write(&tmp, &body)?;
    let f = std::fs::File::open(&tmp)?;
    let _ = f.sync_all();
    drop(f);
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
    // Oldest, least-pinned first. Pinned entries never drop: if only pinned
    // entries remain and the set is still over budget, the set stays over
    // and the next add fails loudly instead of eating pinned memory.
    m.files.sort_by_key(|f| (f.tier.rank(), f.updated));
    while m.files.len() > max_files || total_bytes(m) > max_bytes {
        let Some(i) = m.files.iter().position(|f| f.tier != Tier::Pinned) else {
            break;
        };
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
    let (mut m, loaded_ok) = read_manifest(dir);
    if !loaded_ok {
        repair(dir, &mut m);
    }
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
    if loaded_ok {
        let _ = write_manifest(dir, &m);
    }
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
    // Re-adding keeps the existing tier: tiers move only through set_pinned,
    // so `auto` cannot be promoted (or `pinned` demoted) by a rewrite.
    match m.files.iter_mut().find(|f| f.name == clean) {
        Some(f) => {
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
    let (mut m, loaded_ok) = read_manifest(&dir);
    if !loaded_ok {
        repair(&dir, &mut m);
    }
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
    // Anti-overlap: one fact, one home. A file identical to a pinned plan
    // is rejected — reference the plan instead of copying it.
    let digest = hash_bytes(content.as_bytes());
    for r in list_refs(home, workspace, slug) {
        if std::fs::read(&r.path).is_ok_and(|b| hash_bytes(&b) == digest) {
            return Err(ParziError::Config(
                "duplicates a pinned plan — reference it instead".into(),
            ));
        }
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
    if workspace.trim().is_empty() {
        return Err(ParziError::Config("workspace is required".into()));
    }
    let clean =
        sanitize_file(name).ok_or_else(|| ParziError::Config("unknown context file".into()))?;
    let dir = match slug {
        Some(s) if !s.trim().is_empty() => proj_dir(home, workspace, s),
        _ => ws_dir(home, workspace),
    };
    let (mut m, loaded_ok) = read_manifest(&dir);
    if !loaded_ok {
        repair(&dir, &mut m);
    }
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

/// # Errors
///
/// Rejects unknown files.
pub fn remove(home: &Path, workspace: &str, slug: Option<&str>, name: &str) -> Result<()> {
    if workspace.trim().is_empty() {
        return Err(ParziError::Config("workspace is required".into()));
    }
    let clean =
        sanitize_file(name).ok_or_else(|| ParziError::Config("unknown context file".into()))?;
    let dir = match slug {
        Some(s) if !s.trim().is_empty() => proj_dir(home, workspace, s),
        _ => ws_dir(home, workspace),
    };
    let (mut m, loaded_ok) = read_manifest(&dir);
    if !loaded_ok {
        repair(&dir, &mut m);
    }
    let Some(pos) = m.files.iter().position(|f| f.name == clean) else {
        return Err(ParziError::Config("unknown context file".into()));
    };
    m.files.remove(pos);
    let _ = std::fs::remove_file(dir.join(&clean));
    write_manifest(&dir, &m)
}

#[must_use]
/// Compose the injectable text: pinned plans first, then pinned files,
/// then curated. `budget` counts characters.
pub fn injection_text(home: &Path, workspace: &str, slug: Option<&str>, budget: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    // Pinned plans first: canonical docs, read live.
    for r in list_refs(home, workspace, slug) {
        let Ok(content) = std::fs::read_to_string(&r.path) else {
            continue;
        };
        if content.trim().is_empty() {
            continue;
        }
        let head = format!("# Plan ({}, {})\n\n", r.kind, r.label);
        let room = budget.saturating_sub(used);
        let take = content
            .chars()
            .take(room.saturating_sub(head.chars().count()))
            .collect::<String>();
        if take.trim().is_empty() {
            continue;
        }
        out.push_str(&head);
        out.push_str(&take);
        out.push_str("\n\n");
        used += head.chars().count() + take.chars().count();
        if used >= budget {
            return out;
        }
    }
    let Ok(docs) = list(home, workspace, slug) else {
        return out;
    };
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
            .take(room.saturating_sub(head.chars().count()))
            .collect::<String>();
        if take.trim().is_empty() {
            continue;
        }
        out.push_str(&head);
        out.push_str(&take);
        out.push_str("\n\n");
        used += head.chars().count() + take.chars().count();
        if used >= budget {
            break;
        }
    }
    out
}
