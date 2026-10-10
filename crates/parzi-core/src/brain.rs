use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::error::{atomic_write, ParziError, Result};
use crate::paths;

const CONTEXT_CAP: usize = 12_000;
pub const EVERYWHERE: &str = "all";
const MAX_NOTES: usize = 5_000;
const MAX_BYTES: u64 = 1024 * 1024;
const MAX_HITS: usize = 20;
const MAX_LISTED: usize = 50;
const SUMMARY_MAX: usize = 140;
const TITLE_MAX: usize = 120;
const CONTEXT_HEAD: &str = "# The user's notes (Parzi brain)\nPinned notes are included in full. \
On-demand notes are listed with a one-line summary; read one with brain.read when the task needs it.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteMeta {
    pub path: String,
    pub title: String,
    pub projects: Vec<String>,
    pub tags: Vec<String>,
    pub folder: Option<String>,
    pub source: Option<String>,
    pub pinned: bool,
    #[serde(default)]
    pub archived: bool,
    pub links: Vec<String>,
    pub modified: i64,
    pub bytes: u64,
    pub summary: String,
}

impl NoteMeta {
    #[must_use]
    pub fn everywhere(&self) -> bool {
        self.projects
            .iter()
            .any(|p| p.eq_ignore_ascii_case(EVERYWHERE))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub slug: String,
    pub title: String,
    pub folder: String,
    pub note: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrainContext {
    pub project: Option<Project>,
    pub text: String,
    pub attached: Vec<String>,
    pub listed: Vec<String>,
    pub chars: usize,
    pub tokens: usize,
}

pub fn list() -> Result<Vec<NoteMeta>> {
    Ok(Vault::open()?.list())
}

pub fn read(path: &str) -> Result<String> {
    Vault::open()?.read(path)
}

pub fn write(path: &str, content: &str) -> Result<NoteMeta> {
    Vault::open()?.write(path, content)
}

pub fn delete(path: &str) -> Result<()> {
    Vault::open()?.delete(path)
}

pub fn projects() -> Result<Vec<Project>> {
    Ok(Vault::open()?.projects())
}

/// Deepest project whose folder contains `cwd`, or None. New threads use
/// this instead of a shared fallback so same-project reads stay meaningful.
#[must_use]
pub fn project_for_folder(cwd: &str) -> Option<String> {
    Vault::open().ok()?.project_at(cwd)
}

pub fn project_upsert(slug: Option<&str>, title: &str, folder: &str) -> Result<Project> {
    Vault::open()?.project_upsert(slug, title, folder)
}

pub fn map(note_path: &str, slug: &str, on: bool) -> Result<NoteMeta> {
    Vault::open()?.map(note_path, slug, on)
}

pub fn pin(path: &str, on: bool) -> Result<NoteMeta> {
    Vault::open()?.set_pinned(path, on)
}

pub fn archive_project(slug: &str, on: bool) -> Result<Project> {
    Vault::open()?.project_archive(slug, on)
}

pub fn delete_project(slug: &str) -> Result<String> {
    Vault::open()?.project_delete(slug)
}

#[must_use]
pub fn context_for(cwd: &Path) -> Option<BrainContext> {
    Vault::open().ok()?.context(cwd)
}

#[must_use]
pub fn inside(folder: &Path, path: &Path) -> bool {
    within(&normalize(path), &normalize(folder))
}

pub struct Vault {
    root: PathBuf,
}

impl Vault {
    pub fn open() -> Result<Self> {
        Ok(Self::at(paths::brain_dir()?))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    fn list(&self) -> Vec<NoteMeta> {
        self.scan().into_iter().map(|n| n.meta).collect()
    }

    #[must_use]
    fn projects(&self) -> Vec<Project> {
        projects_of(&self.scan())
    }

    #[must_use]
    pub fn catalog(&self) -> (Vec<NoteMeta>, Vec<Project>) {
        let notes = self.scan();
        let projects = projects_of(&notes);
        (notes.into_iter().map(|n| n.meta).collect(), projects)
    }

    pub fn read(&self, path: &str) -> Result<String> {
        let full = self.locate(path)?.1;
        Ok(String::from_utf8_lossy(&read_whole(&full, path)?).into_owned())
    }

    pub fn resolve(&self, path: &str) -> Result<PathBuf> {
        if path.trim().is_empty() {
            return Ok(self.root.clone());
        }
        let (rel, _) = self.locate(path)?;
        Ok(rel.split('/').fold(self.root.clone(), |p, s| p.join(s)))
    }

    pub fn write(&self, path: &str, content: &str) -> Result<NoteMeta> {
        let (rel, full) = self.locate(path)?;
        if content.len() as u64 > MAX_BYTES {
            return Err(too_big(path));
        }
        atomic_write(&full, content.as_bytes())?;
        self.meta(rel, &full)
    }

    fn delete(&self, path: &str) -> Result<()> {
        Ok(std::fs::remove_file(self.locate(path)?.1)?)
    }

    #[must_use]
    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return vec![];
        }
        let (mut titled, mut bodied) = (vec![], vec![]);
        for n in self.scan() {
            let at = find_folded(&n.body, &q);
            let in_title = n.meta.title.to_lowercase().contains(&q);
            if !in_title && at.is_none() {
                continue;
            }
            let hit = SearchHit {
                snippet: snippet(&n.body, at.unwrap_or(0)),
                path: n.meta.path,
                title: n.meta.title,
            };
            if in_title {
                titled.push(hit);
            } else {
                bodied.push(hit);
            }
        }
        titled.extend(bodied);
        titled.truncate(MAX_HITS);
        titled
    }

    fn project_upsert(&self, slug: Option<&str>, title: &str, folder: &str) -> Result<Project> {
        let folder = folder.trim();
        if folder.is_empty() {
            return Err(ParziError::Validation("a project needs a folder".into()));
        }
        let slug = match slug.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => check_slug(s)?.to_string(),
            None => self.fresh_slug(title),
        };
        let title = match title.trim() {
            "" => slug.clone(),
            t => t.to_string(),
        };
        let (rel, full) = self.locate(&format!("projects/{slug}.md"))?;
        let raw = match read_whole(&full, &rel) {
            Ok(raw) => raw,
            Err(ParziError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(e),
        };
        let text = rewrite_raw(
            &raw,
            &[
                ("title", Some(format!("title: {}", quote(&title)))),
                ("folder", Some(format!("folder: {}", quote(folder)))),
            ],
        )?;
        atomic_write(&full, &text)?;
        self.projects()
            .into_iter()
            .find(|p| p.note.eq_ignore_ascii_case(&rel))
            .ok_or_else(|| ParziError::Store(format!("project note `{rel}` did not read back")))
    }

    fn map(&self, note_path: &str, slug: &str, on: bool) -> Result<NoteMeta> {
        let slug = slug.trim();
        if slug.is_empty() {
            return Err(ParziError::Validation("project slug is empty".into()));
        }
        let (rel, full) = self.locate(note_path)?;
        let raw = read_whole(&full, &rel)?;
        let found = front_blocks(&raw);
        let current = found.iter().find(|b| b.key.as_deref() == Some("projects"));
        let block_style = current.is_some_and(Block::is_block_list);
        let mut items = current.map(Block::list).unwrap_or_default();
        if items.iter().any(|p| p.eq_ignore_ascii_case(slug)) != on {
            if on {
                items.push(slug.to_string());
            } else {
                items.retain(|p| !p.eq_ignore_ascii_case(slug));
            }
            let text = rewrite_raw(
                &raw,
                &[("projects", list_line("projects", &items, block_style))],
            )?;
            atomic_write(&full, &text)?;
        }
        self.meta(rel, &full)
    }

    fn set_pinned(&self, path: &str, on: bool) -> Result<NoteMeta> {
        self.set_flag(path, "pinned", on)
    }

    fn set_archived(&self, path: &str, on: bool) -> Result<NoteMeta> {
        self.set_flag(path, "archived", on)
    }

    /// Sets or clears a `key: true` flag; a cleared flag drops its line.
    fn set_flag(&self, path: &str, key: &str, on: bool) -> Result<NoteMeta> {
        let (rel, full) = self.locate(path)?;
        let raw = read_whole(&full, &rel)?;
        let found = front_blocks(&raw);
        let block = found.iter().rfind(|b| b.key.as_deref() == Some(key));
        let set = block
            .and_then(Block::scalar)
            .is_some_and(|v| v.eq_ignore_ascii_case("true"));
        if on != set || (!on && block.is_some()) {
            let text = rewrite_raw(&raw, &[(key, on.then(|| format!("{key}: true")))])?;
            atomic_write(&full, &text)?;
        }
        self.meta(rel, &full)
    }

    /// The note of project `slug`. Plain notes (no `folder`) are refused so
    /// archive and delete can never reach them.
    fn project_note(&self, slug: &str) -> Result<(String, PathBuf)> {
        let slug = check_slug(slug.trim())?;
        let (rel, full) = self.locate(&format!("projects/{slug}.md"))?;
        let folder = std::fs::symlink_metadata(&full)
            .is_ok_and(|m| m.is_file())
            .then(|| front_fields(&full))
            .flatten()
            .and_then(|f| f.folder);
        if folder.is_none_or(|f| f.trim().is_empty()) {
            return Err(ParziError::Validation(format!("no project `{slug}`")));
        }
        Ok((rel, full))
    }

    /// Archive/unarchive a project. Archived projects vanish from routing and catalogs.
    fn project_archive(&self, slug: &str, on: bool) -> Result<Project> {
        let (rel, _) = self.project_note(slug)?;
        let slug = slug.trim();
        let meta = self.set_archived(&rel, on)?;
        if !on {
            return self
                .projects()
                .into_iter()
                .find(|p| p.note.eq_ignore_ascii_case(&meta.path))
                .ok_or_else(|| {
                    ParziError::Validation(format!(
                        "unarchived `{rel}` but it is not a project (missing folder?)"
                    ))
                });
        }
        Ok(Project {
            slug: slug.to_string(),
            title: meta.title.clone(),
            folder: meta.folder.clone().unwrap_or_default(),
            note: meta.path.clone(),
            notes: vec![],
        })
    }

    /// Delete a project's note. Mappings to a missing slug are ignored.
    fn project_delete(&self, slug: &str) -> Result<String> {
        let (rel, full) = self.project_note(slug)?;
        std::fs::remove_file(&full)?;
        Ok(rel)
    }

    /// Slug of the project holding `cwd`. Reads only the project notes, not
    /// every note body, since new threads call this on the hot path.
    fn project_at(&self, cwd: &str) -> Option<String> {
        let homes = self.project_folders();
        let at = deepest(cwd, homes.iter().map(|(s, f)| (s.as_str(), f.as_str())))?;
        homes.into_iter().nth(at).map(|(slug, _)| slug)
    }

    /// (slug, folder) of every live project, from `projects/*.md` alone.
    fn project_folders(&self) -> Vec<(String, String)> {
        let mut out = vec![];
        let Ok(top) = std::fs::read_dir(&self.root) else {
            return out;
        };
        for dir in top.flatten() {
            let name = dir.file_name().to_string_lossy().into_owned();
            if !name.eq_ignore_ascii_case("projects") || !dir.file_type().is_ok_and(|t| t.is_dir())
            {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(dir.path()) else {
                continue;
            };
            for e in entries.flatten() {
                let file = e.file_name().to_string_lossy().into_owned();
                if file.starts_with('.')
                    || !is_md(&file)
                    || !e.file_type().is_ok_and(|t| t.is_file())
                {
                    continue;
                }
                let rel = format!("{name}/{file}");
                let Some(slug) = project_slug(&rel) else {
                    continue;
                };
                let Some(f) = front_fields(&e.path()).filter(|f| !f.archived) else {
                    continue;
                };
                if let Some(folder) = f.folder.filter(|d| !d.trim().is_empty()) {
                    out.push((slug.to_string(), folder.trim().to_string()));
                }
            }
        }
        out
    }

    #[must_use]
    fn context(&self, cwd: &Path) -> Option<BrainContext> {
        if !self.root.is_dir() {
            return None;
        }
        let notes = self.scan();
        let all = projects_of(&notes);
        let project = deepest(
            &cwd.to_string_lossy(),
            all.iter().map(|p| (p.slug.as_str(), p.folder.as_str())),
        )
        .and_then(|at| all.into_iter().nth(at));
        let by_path: HashMap<&str, &Note> =
            notes.iter().map(|n| (n.meta.path.as_str(), n)).collect();
        let home = project
            .as_ref()
            .and_then(|p| by_path.get(p.note.as_str()).copied());
        let mut everywhere: Vec<&Note> = notes
            .iter()
            .filter(|n| n.meta.everywhere())
            .filter(|n| home.is_none_or(|h| h.meta.path != n.meta.path))
            .collect();
        if project.is_none() && everywhere.is_empty() {
            return None;
        }
        let mut mapped: Vec<&Note> = project
            .iter()
            .flat_map(|p| &p.notes)
            .filter_map(|p| by_path.get(p.as_str()).copied())
            .filter(|n| !n.meta.everywhere())
            .collect();
        everywhere.sort_by_cached_key(|n| (n.meta.title.to_lowercase(), n.meta.path.clone()));
        mapped.sort_by_cached_key(|n| (n.meta.title.to_lowercase(), n.meta.path.clone()));
        let mut text = CONTEXT_HEAD.to_string();
        if let Some(p) = &project {
            text.push_str(&format!(
                "\n\n## Project: {}\nFolder: {}",
                p.title, p.folder
            ));
            if let Some(h) = home.filter(|h| !h.body.is_empty()) {
                text.push_str("\n\n");
                text.push_str(&clip(&h.body, CONTEXT_CAP / 2));
            }
        }
        let mut chars = text.chars().count();
        let (mut attached, mut listed, mut rest) = (vec![], vec![], vec![]);
        for n in everywhere.into_iter().chain(mapped) {
            if !n.meta.pinned {
                rest.push(n);
                continue;
            }
            let mut section = format!("\n\n## {} ({})", n.meta.title, n.meta.path);
            if !n.body.is_empty() {
                section.push_str("\n\n");
                section.push_str(&n.body);
            }
            let len = section.chars().count();
            if chars + len <= CONTEXT_CAP {
                text.push_str(&section);
                chars += len;
                attached.push(n.meta.path.clone());
            } else {
                listed.push(n);
            }
        }
        listed.extend(rest);
        if !listed.is_empty() {
            const ON_DEMAND: &str = "\n\n## On demand";
            text.push_str(ON_DEMAND);
            chars += ON_DEMAND.len();
            // The list counts against the same cap as the pinned notes.
            let mut shown = 0;
            for n in listed.iter().take(MAX_LISTED) {
                let mut line = format!("\n- {} — {}", n.meta.path, n.meta.title);
                if !n.meta.summary.is_empty() {
                    line.push_str(": ");
                    line.push_str(&n.meta.summary);
                }
                let len = line.chars().count();
                if chars + len > CONTEXT_CAP {
                    break;
                }
                text.push_str(&line);
                chars += len;
                shown += 1;
            }
            if listed.len() > shown {
                text.push_str(&format!(
                    "\n- …and {} more (brain_list)",
                    listed.len() - shown
                ));
            }
        }
        let chars = text.chars().count();
        Some(BrainContext {
            project,
            text,
            attached,
            listed: listed.into_iter().map(|n| n.meta.path.clone()).collect(),
            chars,
            tokens: chars.div_ceil(4),
        })
    }

    fn locate(&self, path: &str) -> Result<(String, PathBuf)> {
        let rel = note_path(path)?;
        let full = self.root.join(&rel);
        if !confined(&self.root, &full) {
            return Err(ParziError::Validation(format!(
                "note path `{path}` leaves the vault"
            )));
        }
        Ok((rel, full))
    }

    fn meta(&self, rel: String, full: &Path) -> Result<NoteMeta> {
        let resolver = Resolver::new(&walk(&self.root));
        Ok(load(rel, full, &resolver)?.meta)
    }

    fn scan(&self) -> Vec<Note> {
        let files = walk(&self.root);
        let resolver = Resolver::new(&files);
        files
            .into_iter()
            .filter_map(|(rel, full)| load(rel, &full, &resolver).ok())
            .collect()
    }

    fn fresh_slug(&self, title: &str) -> String {
        let mut base = String::new();
        for c in title.to_lowercase().chars() {
            if c.is_ascii_alphanumeric() {
                base.push(c);
            } else if !base.is_empty() && !base.ends_with('-') {
                base.push('-');
            }
        }
        let base = match base.trim_end_matches('-') {
            "" => "project".to_string(),
            b => b.to_string(),
        };
        let taken = |s: &str| {
            s.eq_ignore_ascii_case(EVERYWHERE)
                || self.root.join("projects").join(format!("{s}.md")).exists()
        };
        let mut slug = base.clone();
        let mut n = 1;
        while taken(&slug) {
            n += 1;
            slug = format!("{base}-{n}");
        }
        slug
    }
}

struct Note {
    meta: NoteMeta,
    body: String,
}

struct Resolver {
    paths: HashMap<String, String>,
    stems: HashMap<String, Option<String>>,
}

impl Resolver {
    fn new(files: &[(String, PathBuf)]) -> Self {
        let mut paths = HashMap::new();
        let mut stems: HashMap<String, Option<String>> = HashMap::new();
        for (rel, _) in files {
            paths.insert(strip_md(rel).to_lowercase(), rel.clone());
            stems
                .entry(stem(rel).to_lowercase())
                .and_modify(|v| *v = None)
                .or_insert_with(|| Some(rel.clone()));
        }
        Self { paths, stems }
    }

    fn resolve(&self, target: &str) -> Option<&str> {
        let key = strip_md(target.replace('\\', "/").trim_start_matches('/')).to_lowercase();
        self.paths
            .get(&key)
            .or_else(|| self.stems.get(&key)?.as_ref())
            .map(String::as_str)
    }
}

#[derive(Default)]
struct Fields {
    title: Option<String>,
    projects: Vec<String>,
    tags: Vec<String>,
    folder: Option<String>,
    source: Option<String>,
    description: Option<String>,
    pinned: bool,
    archived: bool,
}

struct Block {
    key: Option<String>,
    lines: Vec<String>,
}

impl Block {
    fn value(&self) -> &str {
        self.lines
            .first()
            .and_then(|l| l.split_once(':'))
            .map_or("", |(_, v)| v.trim())
    }

    fn scalar(&self) -> Option<String> {
        Some(unquote(self.value())).filter(|v| !v.is_empty())
    }

    fn text(&self) -> Option<String> {
        let head = self.value();
        let mut out = if head.starts_with(['>', '|']) {
            String::new()
        } else {
            unquote(head)
        };
        for line in self.lines[1..]
            .iter()
            .filter(|l| l.starts_with(char::is_whitespace))
        {
            out.push(' ');
            out.push_str(line.trim());
        }
        Some(out).filter(|t| !t.trim().is_empty())
    }

    fn is_block_list(&self) -> bool {
        self.value().is_empty() && self.lines.len() > 1
    }

    fn list(&self) -> Vec<String> {
        let v = self.value();
        let items = if let Some(inner) = v.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            split_inline(inner)
        } else if v.is_empty() {
            self.lines[1..]
                .iter()
                .filter_map(|l| l.trim().strip_prefix('-'))
                .map(unquote)
                .collect()
        } else {
            vec![unquote(v)]
        };
        items.into_iter().filter(|i| !i.is_empty()).collect()
    }
}

fn walk(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    'walk: while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(kind) = e.file_type() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            if kind.is_dir() {
                stack.push(e.path());
            } else if kind.is_file() && is_md(&name) {
                if out.len() >= MAX_NOTES {
                    static WARNED: AtomicBool = AtomicBool::new(false);
                    if !WARNED.swap(true, Relaxed) {
                        tracing::warn!(
                            "brain vault {} holds over {MAX_NOTES} notes; the rest are skipped",
                            root.display()
                        );
                    }
                    break 'walk;
                }
                let full = e.path();
                let Ok(rel) = full.strip_prefix(root) else {
                    continue;
                };
                let rel: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push((rel.join("/"), full));
            }
        }
    }
    out.sort();
    out
}

fn load(path: String, full: &Path, resolver: &Resolver) -> std::io::Result<Note> {
    let md = std::fs::metadata(full)?;
    // An oversized note still lists with its title and summary.
    let raw = String::from_utf8_lossy(&read_head(full, MAX_BYTES)?).into_owned();
    let modified = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    Ok(parse(path, &raw, modified, md.len(), resolver))
}

fn parse(path: String, raw: &str, modified: i64, bytes: u64, resolver: &Resolver) -> Note {
    let (front, body) = split(raw);
    let f = front.map(|t| fields(&blocks(t))).unwrap_or_default();
    let title = f
        .title
        .or_else(|| heading(body))
        .unwrap_or_else(|| stem(&path).to_string());
    let title = ellipsize(&title, TITLE_MAX);
    let mut links: Vec<String> = vec![];
    for target in link_targets(raw) {
        if let Some(p) = resolver.resolve(&target) {
            if !links.iter().any(|l| l == p) {
                links.push(p.to_string());
            }
        }
    }
    Note {
        meta: NoteMeta {
            path,
            title,
            projects: f.projects,
            tags: f.tags,
            folder: f.folder,
            source: f.source,
            pinned: f.pinned,
            archived: f.archived,
            links,
            modified,
            bytes,
            summary: summary(f.description.as_deref(), body),
        },
        body: body.trim().to_string(),
    }
}

fn split(raw: &str) -> (Option<&str>, &str) {
    let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let Some(first) = text.find('\n') else {
        return (None, text);
    };
    if text[..first].trim_end() != "---" {
        return (None, text);
    }
    let start = first + 1;
    let mut pos = start;
    while pos < text.len() {
        let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
        if text[pos..end].trim_end() == "---" {
            return (Some(&text[start..pos]), text.get(end + 1..).unwrap_or(""));
        }
        pos = end + 1;
    }
    (None, text)
}

fn summary(description: Option<&str>, body: &str) -> String {
    let text = description
        .map(str::to_string)
        .or_else(|| first_prose(body))
        .unwrap_or_default();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    ellipsize(&text, SUMMARY_MAX)
}

/// At most `max` chars, ending in `…` when cut.
fn ellipsize(text: &str, max: usize) -> String {
    if text.char_indices().nth(max).is_none() {
        return text.to_string();
    }
    let cut: String = text.chars().take(max - 1).collect();
    format!("{}…", cut.trim_end())
}

fn first_prose(body: &str) -> Option<String> {
    let mut fence: Option<&str> = None;
    for line in body.lines() {
        let t = line.trim();
        if let Some(f) = fence {
            if t.starts_with(f) {
                fence = None;
            }
            continue;
        }
        if let Some(f) = ["```", "~~~"].into_iter().find(|f| t.starts_with(f)) {
            fence = Some(f);
            continue;
        }
        if t.starts_with('#') {
            continue;
        }
        let text = plain(unmark(t));
        if text.chars().any(char::is_alphanumeric) {
            return Some(text);
        }
    }
    None
}

fn unmark(line: &str) -> &str {
    let mut t = line.trim();
    loop {
        let rest = ["- ", "* ", "+ ", "[ ] ", "[x] ", "[X] "]
            .into_iter()
            .find_map(|m| t.strip_prefix(m))
            .or_else(|| t.strip_prefix('>'))
            .or_else(|| {
                let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
                (1..10)
                    .contains(&digits)
                    .then(|| {
                        t[digits..]
                            .strip_prefix(". ")
                            .or_else(|| t[digits..].strip_prefix(") "))
                    })
                    .flatten()
            });
        match rest {
            Some(r) => t = r.trim_start(),
            None => return t,
        }
    }
}

fn plain(text: &str) -> String {
    let text = delink(text);
    let mut out = String::with_capacity(text.len());
    for (i, part) in text.split('`').enumerate() {
        if i % 2 == 1 {
            out.push_str(part);
            continue;
        }
        let part = part.replace("**", "").replace("__", "").replace("~~", "");
        let chars: Vec<char> = part.chars().collect();
        let spaced = |at: Option<usize>| {
            at.and_then(|k| chars.get(k))
                .is_none_or(|c| c.is_whitespace())
        };
        for (j, c) in chars.iter().enumerate() {
            if *c != '*' || (spaced(j.checked_sub(1)) && spaced(Some(j + 1))) {
                out.push(*c);
            }
        }
    }
    out
}

fn delink(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('[') {
        let (before, from) = rest.split_at(i);
        let wiki = from
            .strip_prefix("[[")
            .and_then(|r| r.split_once("]]"))
            .map(|(inner, after)| {
                let label = match inner.rsplit_once('|') {
                    Some((_, alias)) => alias,
                    None => inner.split_once('#').map_or(inner, |(target, _)| target),
                };
                (label.trim(), after)
            });
        let link = wiki.or_else(|| {
            let (label, tail) = from[1..].split_once("](")?;
            let (_, after) = tail.split_once(')')?;
            (!label.contains(['[', ']'])).then_some((label, after))
        });
        if let Some((label, after)) = link {
            out.push_str(before.strip_suffix('!').unwrap_or(before));
            out.push_str(label);
            rest = after;
        } else {
            out.push_str(before);
            out.push('[');
            rest = &from[1..];
        }
    }
    out.push_str(rest);
    out
}

fn blocks(front: &str) -> Vec<Block> {
    let mut out: Vec<Block> = vec![];
    for line in front.lines() {
        match (key_of(line), out.last_mut()) {
            (None, Some(last)) => last.lines.push(line.to_string()),
            (key, _) => out.push(Block {
                key,
                lines: vec![line.to_string()],
            }),
        }
    }
    out
}

fn key_of(line: &str) -> Option<String> {
    if line.starts_with(|c: char| c.is_whitespace() || c == '-' || c == '#') {
        return None;
    }
    let key = line.split_once(':')?.0.trim();
    (!key.is_empty()).then(|| key.to_string())
}

fn fields(blocks: &[Block]) -> Fields {
    let mut f = Fields::default();
    for b in blocks {
        match b.key.as_deref() {
            Some("title") => f.title = b.scalar(),
            Some("folder") => f.folder = b.scalar(),
            Some("source") => f.source = b.scalar(),
            Some("description") => f.description = b.text(),
            Some("projects") => f.projects = b.list(),
            Some("tags") => f.tags = b.list(),
            Some("pinned") => f.pinned = b.scalar().is_some_and(|v| v.eq_ignore_ascii_case("true")),
            Some("archived") => {
                f.archived = b.scalar().is_some_and(|v| v.eq_ignore_ascii_case("true"))
            }
            _ => {}
        }
    }
    f
}

fn rewrite(raw: &str, edits: &[(&str, Option<String>)]) -> String {
    let nl = if raw.contains("\r\n") { "\r\n" } else { "\n" };
    let (front, body) = split(raw);
    let mut found = front.map(blocks).unwrap_or_default();
    for (key, text) in edits {
        let at = found.iter().position(|b| b.key.as_deref() == Some(*key));
        let lines = text
            .as_deref()
            .map(|t| t.lines().map(str::to_string).collect::<Vec<_>>());
        match (at, lines) {
            (Some(i), Some(lines)) => found[i].lines = lines,
            (Some(i), None) => {
                found.remove(i);
            }
            (None, Some(lines)) => found.push(Block {
                key: Some((*key).to_string()),
                lines,
            }),
            (None, None) => {}
        }
    }
    let mut out = format!("---{nl}");
    for line in found.iter().flat_map(|b| &b.lines) {
        out.push_str(line);
        out.push_str(nl);
    }
    out.push_str("---");
    out.push_str(nl);
    out.push_str(body);
    out
}

/// `rewrite` over raw bytes: the frontmatter is edited as text and the body
/// keeps its exact bytes, so a stray non-UTF-8 byte survives a pin.
fn rewrite_raw(raw: &[u8], edits: &[(&str, Option<String>)]) -> Result<Vec<u8>> {
    let text = String::from_utf8_lossy(raw);
    let body = split(&text).1.len();
    let head = text.len() - body;
    if raw.get(..head) != Some(&text.as_bytes()[..head]) {
        return Err(ParziError::Validation(
            "the note's frontmatter is not UTF-8 text".into(),
        ));
    }
    let mut out = rewrite(&text, edits).into_bytes();
    out.truncate(out.len() - body);
    out.extend_from_slice(&raw[head..]);
    Ok(out)
}

fn too_big(path: &str) -> ParziError {
    ParziError::Validation(format!(
        "note `{path}` is over the 1 MB note limit; edit it outside Parzi"
    ))
}

/// The whole note, refused past the size cap so an edit never truncates it.
fn read_whole(full: &Path, path: &str) -> Result<Vec<u8>> {
    let raw = read_head(full, MAX_BYTES + 1)?;
    if raw.len() as u64 > MAX_BYTES {
        return Err(too_big(path));
    }
    Ok(raw)
}

/// At most `cap` bytes from the start of the file.
fn read_head(full: &Path, cap: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut raw = vec![];
    std::fs::File::open(full)?.take(cap).read_to_end(&mut raw)?;
    Ok(raw)
}

fn front_blocks(raw: &[u8]) -> Vec<Block> {
    split(&String::from_utf8_lossy(raw))
        .0
        .map(blocks)
        .unwrap_or_default()
}

/// Frontmatter fields only, for callers that never need the body.
fn front_fields(full: &Path) -> Option<Fields> {
    Some(fields(&front_blocks(&read_head(full, MAX_BYTES).ok()?)))
}

fn list_line(key: &str, items: &[String], block: bool) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let quoted = items.iter().map(|i| quote(i));
    Some(if block {
        std::iter::once(format!("{key}:"))
            .chain(quoted.map(|i| format!("  - {i}")))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        format!("{key}: [{}]", quoted.collect::<Vec<_>>().join(", "))
    })
}

fn quote(v: &str) -> String {
    let v = v.replace(['\r', '\n'], " ");
    let v = v.trim();
    let plain = !v.is_empty()
        && !v.starts_with(|c: char| "[]{}&*!|>'\"%@`#,?:-".contains(c))
        && !v.contains(": ")
        && !v.contains(" #")
        && !v.ends_with(':')
        && !v.contains([',', '[', ']', '{', '}'])
        && !matches!(
            v.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "yes" | "no" | "~"
        );
    if plain {
        v.to_string()
    } else {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if let Some(inner) = v.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, chars.peek()) {
                ('\\', Some(&n @ ('\\' | '"'))) => {
                    out.push(n);
                    chars.next();
                }
                _ => out.push(c),
            }
        }
        return out;
    }
    if let Some(inner) = v.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    v.split(" #").next().unwrap_or("").trim().to_string()
}

fn split_inline(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut open: Option<char> = None;
    for c in s.chars() {
        match (open, c) {
            (None, '"' | '\'') => {
                open = Some(c);
                cur.push(c);
            }
            (Some(q), _) if q == c => {
                open = None;
                cur.push(c);
            }
            (None, ',') => out.push(unquote(&std::mem::take(&mut cur))),
            _ => cur.push(c),
        }
    }
    out.push(unquote(&cur));
    out
}

fn heading(body: &str) -> Option<String> {
    body.lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

fn link_targets(raw: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = raw;
    while let Some(open) = rest.find("[[") {
        rest = &rest[open + 2..];
        let Some(close) = rest.find("]]") else {
            break;
        };
        let mut inner = &rest[..close];
        rest = &rest[close + 2..];
        if let Some(again) = inner.rfind("[[") {
            inner = &inner[again + 2..];
        }
        if inner.contains('\n') {
            continue;
        }
        let target = inner
            .split(['|', '#'])
            .next()
            .unwrap_or("")
            .trim_end_matches('\\')
            .trim();
        if !target.is_empty() {
            out.push(target.to_string());
        }
    }
    out
}

fn projects_of(notes: &[Note]) -> Vec<Project> {
    notes
        .iter()
        .filter_map(|home| {
            let slug = project_slug(&home.meta.path)?;
            // Archived notes stay on disk but leave every project list.
            if home.meta.archived {
                return None;
            }
            let folder = home
                .meta
                .folder
                .as_deref()
                .map(str::trim)
                .filter(|f| !f.is_empty())?;
            let filed = format!("projects/{slug}/");
            let notes = notes
                .iter()
                .filter(|n| {
                    n.meta.path != home.meta.path
                        && (n.meta.projects.iter().any(|p| p.eq_ignore_ascii_case(slug))
                            || n.meta.links.contains(&home.meta.path)
                            || note_in_project_folder(&n.meta.path, &filed))
                })
                .map(|n| n.meta.path.clone())
                .collect();
            Some(Project {
                slug: slug.to_string(),
                title: home.meta.title.clone(),
                folder: folder.to_string(),
                note: home.meta.path.clone(),
                notes,
            })
        })
        .collect()
}

/// A note filed directly under `prefix` (`projects/<slug>/`) belongs to that
/// project even without frontmatter: placement implies membership.
fn note_in_project_folder(path: &str, prefix: &str) -> bool {
    // get() instead of slicing: a non-ASCII name may not split at prefix.len().
    path.get(..prefix.len())
        .is_some_and(|h| h.eq_ignore_ascii_case(prefix))
        && path.len() > prefix.len()
        && !path[prefix.len()..].contains('/')
}

/// A project slug names `projects/<slug>.md`: one plain segment, never `all`.
fn check_slug(slug: &str) -> Result<&str> {
    if slug.is_empty() {
        return Err(ParziError::Validation("project slug is empty".into()));
    }
    if slug.contains(['/', '\\']) {
        return Err(ParziError::Validation(format!(
            "project slug `{slug}` must be a plain name"
        )));
    }
    if slug.eq_ignore_ascii_case(EVERYWHERE) {
        return Err(ParziError::Validation(format!(
            "project slug `{slug}` is reserved for notes that apply to every session"
        )));
    }
    Ok(slug)
}

fn project_slug(path: &str) -> Option<&str> {
    let (dir, name) = path.split_once('/')?;
    let slug = strip_md(name);
    (dir.eq_ignore_ascii_case("projects")
        && !name.contains('/')
        && !slug.eq_ignore_ascii_case(EVERYWHERE))
    .then_some(slug)
}

fn note_path(path: &str) -> Result<String> {
    let bad = |why: &str| -> Result<String> {
        Err(ParziError::Validation(format!("note path `{path}` {why}")))
    };
    let p = path.trim().replace('\\', "/");
    if p.starts_with('/') || p.contains(':') || Path::new(path.trim()).is_absolute() {
        return bad("must be relative to the vault");
    }
    let mut parts = vec![];
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => return bad("must stay inside the vault"),
            s if s.starts_with('.') => return bad("must not touch hidden folders"),
            s => parts.push(s),
        }
    }
    match parts.last() {
        Some(name) if is_md(name) => Ok(parts.join("/")),
        _ => bad("must name a .md note"),
    }
}

fn confined(root: &Path, full: &Path) -> bool {
    let Ok(base) = std::fs::canonicalize(root) else {
        return true;
    };
    full.ancestors()
        .find_map(|a| std::fs::canonicalize(a).ok())
        .is_some_and(|real| real.starts_with(&base))
}

fn expand(folder: &str) -> PathBuf {
    match folder
        .strip_prefix("~/")
        .or_else(|| folder.strip_prefix("~\\"))
        .or_else(|| (folder == "~").then_some(""))
        .zip(dirs::home_dir())
    {
        Some(("", home)) => home,
        Some((rest, home)) => home.join(rest),
        None => PathBuf::from(folder),
    }
}

/// Index of the project whose folder holds `cwd`: the deepest folder wins,
/// equal folders go to the first slug. Both sides expand `~`, resolve when
/// they exist and ignore case on Windows, so thread routing and the brain
/// context always pick the same project.
fn deepest<'a>(cwd: &str, projects: impl IntoIterator<Item = (&'a str, &'a str)>) -> Option<usize> {
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return None;
    }
    let here = normalize(&expand(cwd));
    projects
        .into_iter()
        .enumerate()
        .filter_map(|(i, (slug, folder))| {
            let folder = folder.trim();
            if folder.is_empty() {
                return None;
            }
            let folder = normalize(&expand(folder));
            within(&here, &folder).then_some((i, folder.len(), slug))
        })
        .min_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(b.2)))
        .map(|(i, _, _)| i)
}

fn normalize(p: &Path) -> String {
    // A project folder from a note is agent-writable: never resolve a share.
    let real = if crate::paths::is_network_path(&p.to_string_lossy()) {
        p.to_path_buf()
    } else {
        std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
    };
    let s = real.to_string_lossy().replace('\\', "/");
    let s = match s.strip_prefix("//?/") {
        Some(r) => r
            .strip_prefix("UNC/")
            .map_or_else(|| r.to_string(), |u| format!("//{u}")),
        None => s.clone(),
    };
    let s = s.trim_end_matches('/');
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.to_string()
    }
}

fn within(path: &str, folder: &str) -> bool {
    !folder.is_empty()
        && (path == folder
            || path
                .strip_prefix(folder)
                .is_some_and(|r| r.starts_with('/')))
}

fn strip_md(s: &str) -> &str {
    s.len()
        .checked_sub(3)
        .filter(|&i| s.get(i..).is_some_and(|t| t.eq_ignore_ascii_case(".md")))
        .map_or(s, |i| &s[..i])
}

fn is_md(name: &str) -> bool {
    name.len() > 3 && strip_md(name).len() < name.len()
}

fn stem(path: &str) -> &str {
    strip_md(path.rsplit('/').next().unwrap_or(path))
}

fn find_folded(hay: &str, needle: &str) -> Option<usize> {
    let pos = hay.to_lowercase().find(needle)?;
    let mut seen = 0;
    for (i, c) in hay.char_indices() {
        seen += c.to_lowercase().map(char::len_utf8).sum::<usize>();
        if seen > pos {
            return Some(i);
        }
    }
    Some(hay.len())
}

fn snippet(body: &str, at: usize) -> String {
    let start = body[..at]
        .char_indices()
        .rev()
        .nth(59)
        .map_or(0, |(i, _)| i);
    let window: String = body[start..].chars().take(160).collect();
    let mut out = window.split_whitespace().collect::<Vec<_>>().join(" ");
    if start > 0 {
        out.insert(0, '…');
    }
    if start + window.len() < body.len() {
        out.push('…');
    }
    out
}

fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}\n…(truncated)", &text[..i]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::at(dir.path().join("brain"));
        (dir, v)
    }

    fn summary_of(raw: &str) -> String {
        parse("n.md".into(), raw, 0, 0, &Resolver::new(&[]))
            .meta
            .summary
    }

    #[test]
    fn frontmatter_parses_and_rewrites_only_owned_keys() {
        let raw = "---\ntitle: \"Auth: notes\"\nprojects:\n  - parzi\n  - 'web'\ntags: [rust, \"a, b\"]\nfolder: C:\\code\\parzi\nsource: obsidian # imported\npinned: true\nextra:\n  nested: 1\n---\n# Heading\n\nBody [[x]]\n";
        let (front, body) = split(raw);
        let f = fields(&blocks(front.unwrap()));
        assert_eq!(f.title.as_deref(), Some("Auth: notes"));
        assert_eq!(f.projects, ["parzi", "web"]);
        assert_eq!(f.tags, ["rust", "a, b"]);
        assert_eq!(f.folder.as_deref(), Some("C:\\code\\parzi"));
        assert_eq!(f.source.as_deref(), Some("obsidian"));
        assert!(f.pinned);
        assert_eq!(body, "# Heading\n\nBody [[x]]\n");
        assert_eq!(rewrite(raw, &[]), raw);
        let one = vec!["parzi".to_string()];
        let moved = rewrite(
            raw,
            &[
                ("projects", list_line("projects", &one, true)),
                ("title", None),
            ],
        );
        assert!(moved.contains("projects:\n  - parzi\ntags: [rust"));
        assert!(moved.contains("extra:\n  nested: 1\n---\n"));
        assert!(!moved.contains("title:"));
        assert!(moved.ends_with("---\n# Heading\n\nBody [[x]]\n"));
        let fresh = rewrite(
            "plain body\n",
            &[("projects", list_line("projects", &one, false))],
        );
        assert_eq!(fresh, "---\nprojects: [parzi]\n---\nplain body\n");
        let crlf = "---\r\nkeep: me\r\n---\r\nbody\r\n";
        assert_eq!(
            rewrite(crlf, &[("pinned", Some("pinned: true".into()))]),
            "---\r\nkeep: me\r\npinned: true\r\n---\r\nbody\r\n"
        );
        let tricky = "say \"hi\": C:\\x, [y]";
        assert_eq!(unquote(&quote(tricky)), tricky);
        assert_eq!(quote("C:\\code\\parzi"), "C:\\code\\parzi");
        assert_eq!(split("no front\n---\nx"), (None, "no front\n---\nx"));
        assert_eq!(split("\u{feff}bom body"), (None, "bom body"));
        assert_eq!(
            rewrite(
                "\u{feff}bom body",
                &[("pinned", Some("pinned: true".into()))]
            ),
            "---\npinned: true\n---\nbom body"
        );
        assert_eq!(
            split("---\nunclosed: yes\n"),
            (None, "---\nunclosed: yes\n")
        );
    }

    #[test]
    fn wikilinks_resolve_by_path_then_unique_stem() {
        let (_d, v) = vault();
        for p in ["a.md", "sub/B.md", "x/dup.md", "y/dup.md"] {
            v.write(p, "").unwrap();
        }
        let meta = v
            .write(
                "n.md",
                "[[sub/b]] [[b|alias]] [[A#part]] [[dup]] [[nope]] [[a.md]] ![[sub/B.md]] [[x/dup]]",
            )
            .unwrap();
        assert_eq!(meta.links, ["sub/B.md", "a.md", "x/dup.md"]);
        assert_eq!(meta.title, "n");
    }

    #[test]
    fn projects_collect_notes_by_frontmatter_and_by_link() {
        let (d, v) = vault();
        let folder = d.path().join("code");
        let folder = folder.to_str().unwrap();
        let p = v.project_upsert(None, "Parzi App!", folder).unwrap();
        assert_eq!(
            (p.slug.as_str(), p.note.as_str()),
            ("parzi-app", "projects/parzi-app.md")
        );
        assert_eq!(
            (p.title.as_str(), p.folder.as_str()),
            ("Parzi App!", folder)
        );
        v.write("one.md", "---\nprojects: [parzi-app]\n---\nfirst")
            .unwrap();
        v.write("two.md", "see [[parzi-app]]").unwrap();
        v.write("three.md", "unrelated").unwrap();
        v.write("projects/loose.md", "no folder, so not a project")
            .unwrap();
        let all = v.projects();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].notes, ["one.md", "two.md"]);
        assert_eq!(
            v.map("three.md", "parzi-app", true).unwrap().projects,
            ["parzi-app"]
        );
        assert_eq!(
            v.read("three.md").unwrap(),
            "---\nprojects: [parzi-app]\n---\nunrelated"
        );
        assert_eq!(v.projects()[0].notes, ["one.md", "three.md", "two.md"]);
        v.write("projects/parzi-app/probe.md", "filed, not tagged")
            .unwrap();
        v.write("projects/parzi-app/nested/deep.md", "too deep to claim")
            .unwrap();
        assert_eq!(
            v.projects()[0].notes,
            [
                "one.md",
                "projects/parzi-app/probe.md",
                "three.md",
                "two.md"
            ]
        );
        v.map("one.md", "parzi-app", false).unwrap();
        assert_eq!(v.read("one.md").unwrap(), "---\n---\nfirst");
        assert_eq!(
            v.project_upsert(None, "Parzi App", "elsewhere")
                .unwrap()
                .slug,
            "parzi-app-2"
        );
        v.write(
            "projects/parzi-app.md",
            "---\ntitle: Old\nowner: lucas\nfolder: x\n---\nbody stays\n",
        )
        .unwrap();
        let updated = v
            .project_upsert(Some("parzi-app"), "Parzi", folder)
            .unwrap();
        assert_eq!(updated.title, "Parzi");
        let raw = v.read("projects/parzi-app.md").unwrap();
        assert!(
            raw.starts_with("---\ntitle: Parzi\nowner: lucas\nfolder: "),
            "{raw}"
        );
        assert!(raw.ends_with("---\nbody stays\n"), "{raw}");
        assert!(v.project_upsert(Some("a/b"), "x", folder).is_err());
        assert!(v.project_upsert(None, "x", " ").is_err());
    }

    #[test]
    fn non_ascii_note_names_do_not_split_inside_a_char() {
        let prefix = "projects/a/";
        let path = "Notizen/Größe.md";
        assert!(
            !path.is_char_boundary(prefix.len()),
            "fixture must straddle"
        );
        assert!(!note_in_project_folder(path, prefix));
        assert!(note_in_project_folder("projects/A/Größe.md", prefix));
        assert!(!note_in_project_folder("projects/a/", prefix));
        assert!(!note_in_project_folder("projects/a/sub/x.md", prefix));
        let (_d, v) = vault();
        v.project_upsert(Some("a"), "A", "C:\\code\\a").unwrap();
        v.write("Notizen/Größe.md", "x").unwrap();
        v.write("projects/a/Überblick.md", "y").unwrap();
        assert_eq!(v.projects()[0].notes, ["projects/a/Überblick.md"]);
    }

    #[test]
    fn routing_and_context_pick_the_same_project() {
        let (d, v) = vault();
        let code = d.path().join("code");
        std::fs::create_dir_all(code.join("app").join("src")).unwrap();
        let shown = |p: &Path| p.to_str().unwrap().to_string();
        v.project_upsert(Some("beta"), "Beta", &shown(&code))
            .unwrap();
        v.project_upsert(Some("alpha"), "Alpha", &format!("{}/", shown(&code)))
            .unwrap();
        let app = code.join("app");
        v.project_upsert(Some("app"), "App", &shown(&app)).unwrap();
        v.write("n.md", "---\nprojects: [all]\n---\nx").unwrap();
        let both = |cwd: &Path| {
            let routed = v.project_at(&shown(cwd));
            let ctx = v.context(cwd).and_then(|c| c.project).map(|p| p.slug);
            assert_eq!(routed, ctx, "{}", cwd.display());
            routed
        };
        assert_eq!(
            both(&code).as_deref(),
            Some("alpha"),
            "equal folders: first slug"
        );
        assert_eq!(
            both(&app.join("src")).as_deref(),
            Some("app"),
            "deepest wins"
        );
        assert_eq!(both(d.path()), None);
        assert_eq!(v.project_at("  "), None);
        if cfg!(windows) {
            let upper = PathBuf::from(shown(&app.join("src")).to_uppercase());
            assert_eq!(both(&upper).as_deref(), Some("app"));
        }
        v.project_archive("app", true).unwrap();
        assert_eq!(
            both(&app).as_deref(),
            Some("alpha"),
            "archived projects stop routing"
        );
    }

    #[test]
    fn notes_over_the_cap_are_refused_not_truncated() {
        let (_d, v) = vault();
        let big = "x".repeat(usize::try_from(MAX_BYTES).unwrap() + 1);
        let err = v.write("big.md", &big).unwrap_err();
        assert!(matches!(err, ParziError::Validation(_)), "{err}");
        let full = v.root().join("huge.md");
        std::fs::create_dir_all(v.root()).unwrap();
        std::fs::write(&full, format!("# Huge\n\n{big}")).unwrap();
        let meta = v.list().into_iter().find(|m| m.path == "huge.md").unwrap();
        assert_eq!(meta.title, "Huge", "an oversized note still lists");
        assert!(matches!(v.read("huge.md"), Err(ParziError::Validation(_))));
        assert!(v.set_pinned("huge.md", true).is_err());
        assert_eq!(
            std::fs::metadata(&full).unwrap().len(),
            big.len() as u64 + 8
        );
    }

    #[test]
    fn non_utf8_notes_read_leniently_and_keep_their_bytes_on_edit() {
        let (_d, v) = vault();
        let full = v.root().join("latin.md");
        std::fs::create_dir_all(v.root()).unwrap();
        std::fs::write(&full, b"---\ntitle: Old\n---\nK\xe4se\n").unwrap();
        assert_eq!(
            v.read("latin.md").unwrap(),
            "---\ntitle: Old\n---\nK\u{fffd}se\n"
        );
        assert!(v.set_pinned("latin.md", true).unwrap().pinned);
        assert_eq!(
            std::fs::read(&full).unwrap(),
            b"---\ntitle: Old\npinned: true\n---\nK\xe4se\n"
        );
        std::fs::write(&full, b"---\ntitle: K\xe4se\n---\nbody\n").unwrap();
        assert!(matches!(
            v.set_pinned("latin.md", true),
            Err(ParziError::Validation(_))
        ));
        assert_eq!(
            v.read("latin.md").unwrap(),
            "---\ntitle: K\u{fffd}se\n---\nbody\n"
        );
    }

    #[test]
    fn project_ops_never_reach_plain_notes() {
        let (_d, v) = vault();
        v.write("projects/sub/note.md", "plain").unwrap();
        v.write("projects/plain.md", "no folder").unwrap();
        for slug in ["sub/note", "sub\\note", "all", " ", "../x"] {
            assert!(v.project_delete(slug).is_err(), "{slug:?}");
            assert!(v.project_archive(slug, true).is_err(), "{slug:?}");
        }
        assert!(matches!(
            v.project_delete("plain"),
            Err(ParziError::Validation(_))
        ));
        assert!(v.project_archive("plain", true).is_err());
        assert_eq!(v.read("projects/sub/note.md").unwrap(), "plain");
        assert_eq!(v.read("projects/plain.md").unwrap(), "no folder");
    }

    #[test]
    fn long_titles_clip_and_the_on_demand_list_fits_the_cap() {
        let (d, v) = vault();
        let folder = d.path().join("code");
        std::fs::create_dir_all(&folder).unwrap();
        v.project_upsert(Some("demo"), "Demo", folder.to_str().unwrap())
            .unwrap();
        let title = "T".repeat(500);
        let body = "word ".repeat(100);
        for i in 0..50 {
            v.write(
                &format!("n{i:02}.md"),
                &format!("---\ntitle: {title}\nprojects: [demo]\n---\n{body}"),
            )
            .unwrap();
        }
        let ctx = v.context(&folder).unwrap();
        let first = v.list().into_iter().find(|m| m.path == "n00.md").unwrap();
        assert_eq!(first.title.chars().count(), TITLE_MAX);
        assert!(first.title.ends_with('…'));
        assert_eq!(ctx.listed.len(), 50);
        assert!(ctx.chars <= CONTEXT_CAP + 40, "{}", ctx.chars);
        let shown = ctx.text.matches("\n- n").count();
        assert!(shown < 50, "{shown}");
        assert!(ctx
            .text
            .ends_with(&format!("\n- …and {} more (brain_list)", 50 - shown)));
    }

    #[test]
    fn projects_archive_and_delete() {
        let (_d, v) = vault();
        let folder = "C:\\code\\demo";
        v.project_upsert(Some("demo"), "Demo", folder).unwrap();
        assert_eq!(v.projects().len(), 1);
        v.project_archive("demo", true).unwrap();
        assert!(v.projects().is_empty(), "archived projects leave the list");
        assert!(v.read("projects/demo.md").is_ok(), "the note stays on disk");
        v.project_archive("demo", false).unwrap();
        assert_eq!(v.projects().len(), 1, "unarchive brings it back");
        assert!(v.project_archive("missing", true).is_err());
        let rel = v.project_delete("demo").unwrap();
        assert_eq!(rel, "projects/demo.md");
        assert!(v.projects().is_empty());
        assert!(v.project_delete("demo").is_err(), "deleting twice fails");
    }

    #[test]
    fn context_pins_in_full_and_lists_the_rest_with_a_summary() {
        let (d, v) = vault();
        let folder = d.path().join("code");
        std::fs::create_dir_all(folder.join("src")).unwrap();
        v.write(
            "projects/demo.md",
            &format!(
                "---\ntitle: Demo\nfolder: {}\n---\nRust CLI. Run `cargo xtask` before a commit.\n",
                folder.display()
            ),
        )
        .unwrap();
        v.write(
            "notes/style.md",
            "---\nprojects: [demo]\npinned: true\n---\n# Code style\n\nNo comments; match the file around you.\n",
        )
        .unwrap();
        v.write(
            "notes/release.md",
            "---\nprojects: [demo]\n---\n# Release\n\n- Bump the **version**, then tag it.\n\n1. Build\n2. Upload\n",
        )
        .unwrap();
        v.write(
            "notes/me.md",
            "---\ntitle: About me\nprojects: [all]\npinned: true\n---\nI like short answers.\n",
        )
        .unwrap();
        v.write(
            "notes/machines.md",
            "---\nprojects: [all]\ndescription: Which machines and shells I use\n---\nLong inventory…\n",
        )
        .unwrap();
        v.write("notes/other.md", "Unrelated.").unwrap();
        let ctx = v.context(&folder.join("src")).unwrap();
        assert_eq!(ctx.project.as_ref().map(|p| p.slug.as_str()), Some("demo"));
        assert_eq!(ctx.attached, ["notes/me.md", "notes/style.md"]);
        assert_eq!(ctx.listed, ["notes/machines.md", "notes/release.md"]);
        assert_eq!(
            ctx.text,
            format!(
                "# The user's notes (Parzi brain)\n\
                 Pinned notes are included in full. On-demand notes are listed with a one-line summary; read one with brain.read when the task needs it.\n\
                 \n\
                 ## Project: Demo\n\
                 Folder: {}\n\
                 \n\
                 Rust CLI. Run `cargo xtask` before a commit.\n\
                 \n\
                 ## About me (notes/me.md)\n\
                 \n\
                 I like short answers.\n\
                 \n\
                 ## Code style (notes/style.md)\n\
                 \n\
                 # Code style\n\
                 \n\
                 No comments; match the file around you.\n\
                 \n\
                 ## On demand\n\
                 - notes/machines.md — machines: Which machines and shells I use\n\
                 - notes/release.md — Release: Bump the version, then tag it.",
                folder.display()
            )
        );
        assert_eq!(ctx.chars, ctx.text.chars().count());
        assert_eq!(ctx.tokens, ctx.chars.div_ceil(4));
    }

    #[test]
    fn context_without_a_project_carries_only_notes_for_every_session() {
        let (d, v) = vault();
        assert!(v.context(Path::new("")).is_none());
        v.write("notes/plain.md", "nothing global").unwrap();
        assert!(v.context(Path::new("")).is_none());
        assert!(v.context(d.path()).is_none());
        v.write(
            "notes/me.md",
            "---\nprojects: [all]\npinned: true\n---\nI prefer short answers.",
        )
        .unwrap();
        v.write(
            "notes/tools.md",
            "---\nprojects: [ALL]\ndescription: Which CLIs are installed\n---\nlong body",
        )
        .unwrap();
        let ctx = v.context(Path::new("")).unwrap();
        assert!(ctx.project.is_none());
        assert_eq!(ctx.attached, ["notes/me.md"]);
        assert_eq!(ctx.listed, ["notes/tools.md"]);
        assert_eq!(
            ctx.text,
            format!(
                "{CONTEXT_HEAD}\n\n## me (notes/me.md)\n\nI prefer short answers.\n\n## On demand\n- notes/tools.md — tools: Which CLIs are installed"
            )
        );
        assert_eq!(v.context(d.path()).unwrap(), ctx);
        assert_eq!(v.context(Path::new("  ")).unwrap(), ctx);
    }

    #[test]
    fn pinned_notes_that_do_not_fit_move_to_on_demand() {
        let (d, v) = vault();
        let folder = d.path().join("code");
        std::fs::create_dir_all(&folder).unwrap();
        v.project_upsert(Some("demo"), "Demo", folder.to_str().unwrap())
            .unwrap();
        let big = "x".repeat(5_000);
        for name in ["a", "b", "c"] {
            v.write(
                &format!("{name}.md"),
                &format!("---\nprojects: [demo]\npinned: true\n---\n{big}"),
            )
            .unwrap();
        }
        v.write("d.md", "---\nprojects: [demo]\npinned: true\n---\nsmall")
            .unwrap();
        v.write("e.md", "---\nprojects: [demo]\n---\nnot pinned")
            .unwrap();
        let ctx = v.context(&folder).unwrap();
        assert_eq!(ctx.attached, ["a.md", "b.md", "d.md"]);
        assert_eq!(ctx.listed, ["c.md", "e.md"]);
        assert!(ctx.chars <= CONTEXT_CAP + 400, "{}", ctx.chars);
        assert!(ctx.text.ends_with(&format!(
            "\n\n## On demand\n- c.md — c: {}…\n- e.md — e: not pinned",
            "x".repeat(SUMMARY_MAX - 1)
        )));
        for i in 0..50 {
            v.write(&format!("more/n{i:02}.md"), "[[demo]]").unwrap();
        }
        let ctx = v.context(&folder).unwrap();
        assert_eq!(ctx.listed.len(), 52);
        assert_eq!(ctx.text.matches("\n- more/").count(), 48);
        assert!(ctx.text.ends_with("\n- …and 2 more (brain_list)"));
    }

    #[test]
    fn everywhere_and_project_notes_merge_once_in_the_deepest_project() {
        let (d, v) = vault();
        let outer = d.path().join("code");
        let inner = outer.join("inner");
        std::fs::create_dir_all(inner.join("src")).unwrap();
        v.project_upsert(Some("outer"), "Outer", outer.to_str().unwrap())
            .unwrap();
        v.project_upsert(Some("inner"), "Inner", inner.to_str().unwrap())
            .unwrap();
        v.write(
            "both.md",
            "---\ntitle: Both\nprojects: [inner, all]\npinned: true\n---\nshared rule",
        )
        .unwrap();
        v.write(
            "global.md",
            "---\ntitle: Zeta global\nprojects: [all]\npinned: true\n---\nglobal rule",
        )
        .unwrap();
        v.write(
            "local.md",
            "---\ntitle: Alpha local\npinned: true\n---\nsee [[inner]]",
        )
        .unwrap();
        v.write("outer.md", "---\nprojects: [outer]\n---\nouter only")
            .unwrap();
        let ctx = v.context(&inner.join("src")).unwrap();
        assert_eq!(ctx.project.as_ref().unwrap().slug, "inner");
        assert_eq!(ctx.attached, ["both.md", "global.md", "local.md"]);
        assert!(ctx.listed.is_empty());
        assert_eq!(ctx.text.matches("(both.md)").count(), 1);
        assert!(!ctx.text.contains("## On demand"));
        let top = v.context(&outer).unwrap();
        assert_eq!(top.project.as_ref().unwrap().slug, "outer");
        assert_eq!(top.attached, ["both.md", "global.md"]);
        assert_eq!(top.listed, ["outer.md"]);
        assert!(Vault::at(d.path().join("missing"))
            .context(&inner)
            .is_none());
    }

    #[test]
    fn all_is_reserved_for_every_session() {
        let (d, v) = vault();
        let folder = d.path().join("code");
        let f = folder.to_str().unwrap();
        assert!(matches!(
            v.project_upsert(Some("all"), "x", f),
            Err(ParziError::Validation(_))
        ));
        assert!(v.project_upsert(Some(" ALL "), "x", f).is_err());
        assert_eq!(v.project_upsert(None, "All", f).unwrap().slug, "all-2");
        v.write("projects/all.md", &format!("---\nfolder: {f}\n---\n"))
            .unwrap();
        v.write("projects/ALL.md", &format!("---\nfolder: {f}\n---\n"))
            .unwrap();
        let slugs: Vec<String> = v.projects().into_iter().map(|p| p.slug).collect();
        assert_eq!(slugs, ["all-2"]);
        v.write("n.md", "x").unwrap();
        assert!(v.map("n.md", EVERYWHERE, true).unwrap().everywhere());
        assert!(!v.map("n.md", "All", false).unwrap().everywhere());
    }

    #[test]
    fn summaries_prefer_the_description_then_the_first_prose_line() {
        assert_eq!(
            summary_of("---\ndescription: Short and   sweet\n---\n# H\n\nBody line"),
            "Short and sweet"
        );
        assert_eq!(
            summary_of("---\ndescription: >-\n  Folded\n  text\nother: x\n---\nBody"),
            "Folded text"
        );
        assert_eq!(
            summary_of("---\ndescription: \"\"\n---\nBody wins"),
            "Body wins"
        );
        assert_eq!(
            summary_of(
                "# Heading\n\n```rust\nlet x = 1;\n```\n\n- **Bold** item with `__init__` and [[target|alias]], [[page#part]] and [a link](http://x)\n"
            ),
            "Bold item with __init__ and alias, page and a link"
        );
        assert_eq!(summary_of("> quoted *step*   here"), "quoted step here");
        assert_eq!(summary_of("## Steps\n\n1. First step\n"), "First step");
        assert_eq!(summary_of("- [ ] open task"), "open task");
        assert_eq!(summary_of("---\n\n***\n\n2 * 3 = 6"), "2 * 3 = 6");
        assert_eq!(summary_of("\u{feff}plain start"), "plain start");
        assert_eq!(summary_of("# Only a heading\n\n```\ncode\n```"), "");
        let long = summary_of(&"word ".repeat(60));
        assert_eq!(long.chars().count(), SUMMARY_MAX);
        assert!(long.ends_with("word…"), "{long}");
        let wide = summary_of(&"é".repeat(300));
        assert_eq!(wide, format!("{}…", "é".repeat(SUMMARY_MAX - 1)));
    }

    #[test]
    fn pinning_round_trips_and_keeps_other_keys_and_line_endings() {
        let (_d, v) = vault();
        let raw = "---\r\ntitle: Keep\r\ntags: [a, b]\r\n---\r\nBody\r\n";
        v.write("n.md", raw).unwrap();
        assert!(v.set_pinned("n.md", true).unwrap().pinned);
        let pinned = "---\r\ntitle: Keep\r\ntags: [a, b]\r\npinned: true\r\n---\r\nBody\r\n";
        assert_eq!(v.read("n.md").unwrap(), pinned);
        assert!(v.set_pinned("n.md", true).unwrap().pinned);
        assert_eq!(v.read("n.md").unwrap(), pinned);
        let off = v.set_pinned("n.md", false).unwrap();
        assert!(!off.pinned);
        assert_eq!((off.title.as_str(), off.tags.len()), ("Keep", 2));
        assert_eq!(v.read("n.md").unwrap(), raw);
        v.write("plain.md", "just text\n").unwrap();
        assert!(v.set_pinned("plain.md", true).unwrap().pinned);
        assert_eq!(
            v.read("plain.md").unwrap(),
            "---\npinned: true\n---\njust text\n"
        );
        v.write("f.md", "---\npinned: false\nx: 1\n---\nb").unwrap();
        v.set_pinned("f.md", false).unwrap();
        assert_eq!(v.read("f.md").unwrap(), "---\nx: 1\n---\nb");
        v.write("t.md", "---\npinned: no\n---\nb").unwrap();
        v.set_pinned("t.md", true).unwrap();
        assert_eq!(v.read("t.md").unwrap(), "---\npinned: true\n---\nb");
        assert!(v.set_pinned("../out.md", true).is_err());
        assert!(v.set_pinned("missing.md", true).is_err());
    }

    #[test]
    fn search_puts_title_hits_first() {
        let (_d, v) = vault();
        v.write("one.md", &format!("{}needle in the body", "x ".repeat(100)))
            .unwrap();
        v.write("two.md", "# The Needle\nnothing here").unwrap();
        v.write("three.md", "nothing").unwrap();
        let hits = v.search("NEEDLE");
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(paths, ["two.md", "one.md"]);
        assert_eq!(hits[0].title, "The Needle");
        assert!(
            hits[1].snippet.starts_with('…') && hits[1].snippet.ends_with("needle in the body")
        );
        assert!(v.search("  ").is_empty());
        assert!(Vault::at("/no/such/vault").search("x").is_empty());
    }

    #[test]
    fn note_paths_stay_inside_the_vault() {
        for bad in [
            "../x.md",
            "a/../../x.md",
            "/abs.md",
            "C:\\x.md",
            "C:x.md",
            "notes.txt",
            ".obsidian/app.md",
            "a/.hidden.md",
            "",
            "dir/",
            ".md",
        ] {
            assert!(note_path(bad).is_err(), "{bad:?}");
        }
        assert_eq!(note_path("a\\b.md").unwrap(), "a/b.md");
        assert_eq!(note_path("./a//b.MD").unwrap(), "a/b.MD");
        let (d, v) = vault();
        assert!(v.write("../escape.md", "x").is_err());
        assert!(!d.path().join("escape.md").exists());
        assert!(v.delete("../escape.md").is_err());
        let meta = v.write("deep/new/note.md", "# Hi").unwrap();
        assert_eq!(
            (meta.path.as_str(), meta.title.as_str()),
            ("deep/new/note.md", "Hi")
        );
        std::fs::create_dir_all(v.root().join(".obsidian")).unwrap();
        std::fs::write(v.root().join(".obsidian/hidden.md"), "x").unwrap();
        let listed: Vec<String> = v.list().into_iter().map(|m| m.path).collect();
        assert_eq!(listed, ["deep/new/note.md"]);
        v.delete("deep/new/note.md").unwrap();
        assert!(v.list().is_empty());
        assert_eq!(v.resolve(" ").unwrap(), v.root());
        assert!(v.resolve("deep/new/note").is_err());
        assert_eq!(
            v.resolve("deep\\new/note.md").unwrap(),
            v.root().join("deep").join("new").join("note.md")
        );
        assert!(v.resolve("../x.md").is_err());
        assert!(v.resolve("C:\\x.md").is_err());
        assert!(inside(d.path(), &d.path().join("brain")));
        assert!(inside(&d.path().join("brain"), &d.path().join("brain")));
        assert!(!inside(&d.path().join("brain"), d.path()));
        assert!(!inside(&d.path().join("bra"), &d.path().join("brain")));
        assert!(!inside(Path::new(""), d.path()));
    }
}
