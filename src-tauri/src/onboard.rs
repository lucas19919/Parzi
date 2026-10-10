use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf, MAIN_SEPARATOR};
use std::process::Command;

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::AppState;

const NOTE_CAP: u64 = 256 * 1024;
const JSON_CAP: u64 = 8 * 1024 * 1024;
const MAX_FOLDERS: usize = 200;
const CODEX_FILES: usize = 300;
const CHROME_EPOCH_MS: i64 = 11_644_473_600_000;
const HISTORY_SQL: &str = "SELECT url, title, visit_count, last_visit_time FROM urls WHERE hidden = 0 ORDER BY last_visit_time DESC LIMIT 2000";

const TOOLS: [(&str, &str); 6] = [
    ("claude", "Claude Code"),
    ("codex", "Codex"),
    ("grok", "Grok"),
    ("cursor", "Cursor"),
    ("opencode", "OpenCode"),
    ("gemini", "Gemini"),
];

const BROWSERS: [(&str, &str); 3] = [
    ("brave", "Brave"),
    ("edge", "Microsoft Edge"),
    ("chrome", "Google Chrome"),
];

#[derive(Debug, Clone, Serialize)]
pub struct Scan {
    pub browsers: Vec<BrowserSource>,
    pub tools: Vec<ToolSource>,
    pub folders: Vec<FolderCandidate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowserSource {
    pub id: String,
    pub name: String,
    pub profile: String,
    pub bookmarks: usize,
    pub history: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSource {
    pub id: String,
    pub name: String,
    pub found: bool,
    pub notes: Vec<NoteCandidate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteCandidate {
    pub source: String,
    pub title: String,
    pub kind: String,
    pub project_folder: Option<String>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderCandidate {
    pub path: String,
    pub name: String,
    pub sources: Vec<String>,
    pub last_used: Option<i64>,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowserData {
    pub bookmarks: Vec<Bookmark>,
    pub history: Vec<Visit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    pub folder: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Visit {
    pub url: String,
    pub title: String,
    pub visits: i64,
    pub last_visit: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportReport {
    pub notes: usize,
    pub projects: usize,
    pub skipped: Vec<String>,
}

#[tauri::command]
pub async fn onboard_scan() -> Result<Scan, String> {
    blocking(|| {
        let env = Env::system()?;
        let (tools, folders) = local_scan(&env);
        Ok(Scan {
            browsers: scan_browsers(),
            tools,
            folders,
        })
    })
    .await
}

#[tauri::command]
pub async fn onboard_browser(id: String, profile: String) -> Result<BrowserData, String> {
    blocking(move || browser_data(&id, &profile)).await
}

#[tauri::command]
pub async fn onboard_import(
    notes: Vec<String>,
    folders: Vec<String>,
) -> Result<ImportReport, String> {
    blocking(move || {
        let env = Env::system()?;
        Ok(import_with(&env, &notes, &folders))
    })
    .await
}

#[tauri::command]
pub async fn agent_install(provider: String) -> Result<(), String> {
    let script = parzi_providers::install_script(&provider)
        .ok_or_else(|| format!("no install is known for {provider}"))?;
    let name = parzi_providers::display_name(&provider);
    open_terminal(&format!("Install {name}"), &script)
}

#[tauri::command]
pub async fn agent_login(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: String,
) -> Result<bool, String> {
    let cfg = state.orch.config();
    match parzi_providers::login_script(&provider, &cfg) {
        Some(script) => {
            let name = parzi_providers::display_name(&provider);
            open_terminal(&format!("Sign in to {name}"), &script).map(|()| false)
        }
        None => {
            let step = |step: &str| {
                let _ = app.emit(
                    "parzi://sign-in",
                    serde_json::json!({ "provider": provider, "step": step }),
                );
            };
            step("starting");
            parzi_providers::sign_in(&provider, &cfg, || step("browser"))
                .await
                .map(|()| true)
                .map_err(|e| e.message)
        }
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(windows)]
pub(crate) fn open_terminal(title: &str, script: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let title = title.replace('\'', "''");
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-NoExit",
            "-Command",
        ])
        .arg(format!("$Host.UI.RawUI.WindowTitle = '{title}'; {script}"))
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not open a terminal: {e}"))
}

#[cfg(target_os = "macos")]
pub(crate) fn open_terminal(_title: &str, script: &str) -> Result<(), String> {
    let script = script.replace('\\', "\\\\").replace('"', "\\\"");
    let tell = format!("tell application \"Terminal\" to do script \"{script}\"");
    let mut child = Command::new("osascript")
        .args([
            "-e",
            tell.as_str(),
            "-e",
            "tell application \"Terminal\" to activate",
        ])
        .spawn()
        .map_err(|e| format!("could not open Terminal: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn open_terminal(_title: &str, script: &str) -> Result<(), String> {
    Err(format!("Open a terminal and run: {script}"))
}

struct Env {
    home: PathBuf,
    parzi: PathBuf,
    temp: PathBuf,
}

impl Env {
    fn system() -> Result<Self, String> {
        Ok(Self {
            home: std::env::home_dir().ok_or("cannot locate the home folder")?,
            parzi: parzi_core::paths::parzi_dir().map_err(|e| e.to_string())?,
            temp: std::env::temp_dir(),
        })
    }
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

enum Place {
    Flat(String),
    Skill(String),
    Memory(String, String),
}

struct Cand {
    tool: &'static str,
    note: NoteCandidate,
    place: Place,
}

struct Hit {
    path: String,
    tool: &'static str,
    at: Option<i64>,
}

fn hit(path: &str, tool: &'static str, at: Option<i64>) -> Hit {
    Hit {
        path: path.to_string(),
        tool,
        at,
    }
}

fn sub(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, s| p.join(s))
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

fn dirs(dir: &Path) -> Vec<PathBuf> {
    entries(dir)
        .into_iter()
        .filter(|p| p.is_dir() && !file_name(p).starts_with('.'))
        .collect()
}

fn files_with(dir: &Path, ext: &str) -> Vec<PathBuf> {
    entries(dir)
        .into_iter()
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(ext))
        })
        .collect()
}

fn mtime_ms(p: &Path) -> Option<i64> {
    let t = fs::metadata(p).and_then(|m| m.modified()).ok()?;
    let d = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(d.as_millis()).ok()
}

fn is_secret(rel: &Path) -> bool {
    rel.components().any(|c| {
        let Component::Normal(n) = c else {
            return false;
        };
        let n = n.to_string_lossy().to_lowercase();
        ["token", "secret", "oauth", "credential", "cookie"]
            .iter()
            .any(|w| n.contains(w))
            || [".key", ".sqlite", ".db", ".pem"]
                .iter()
                .any(|e| n.ends_with(e))
            || ["auth.json", "cap_sid", "login data", "web data"].contains(&n.as_str())
    })
}

fn named_secret(p: &Path) -> bool {
    p.file_name().is_none_or(|n| is_secret(Path::new(n)))
}

fn read_capped(p: &Path, cap: u64) -> Result<String, String> {
    if named_secret(p) {
        return Err("refused to read a credential file".into());
    }
    let meta = fs::metadata(p).map_err(|e| e.to_string())?;
    if meta.len() > cap {
        return Err(format!("larger than {} KB", cap / 1024));
    }
    let mut s = String::new();
    File::open(p)
        .and_then(|f| f.take(cap).read_to_string(&mut s))
        .map_err(|e| e.to_string())?;
    Ok(s)
}

fn read_json<T: serde::de::DeserializeOwned>(p: &Path, cap: u64) -> Option<T> {
    if named_secret(p) {
        return None;
    }
    let f = File::open(p).ok()?;
    serde_json::from_reader(BufReader::new(f.take(cap))).ok()
}

fn head_lines(p: &Path, n: usize) -> Vec<String> {
    if named_secret(p) {
        return Vec::new();
    }
    File::open(p)
        .map(|f| {
            BufReader::new(f.take(NOTE_CAP))
                .lines()
                .take(n)
                .map_while(Result::ok)
                .collect()
        })
        .unwrap_or_default()
}

fn norm_with(raw: &str, windows: bool) -> Option<String> {
    let raw = raw.trim();
    if !windows {
        if !raw.starts_with('/') {
            return None;
        }
        let mut out = String::with_capacity(raw.len());
        for c in raw.chars() {
            if !(c == '/' && out.ends_with('/')) {
                out.push(c);
            }
        }
        while out.len() > 1 && out.ends_with('/') {
            out.pop();
        }
        return Some(out);
    }
    let s = raw.replace('/', "\\");
    let s = s
        .strip_prefix(r"\\?\UNC\")
        .map(|r| format!(r"\\{r}"))
        .or_else(|| s.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or(s);
    let unc = s.starts_with(r"\\");
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if !(c == '\\' && out.ends_with('\\')) {
            out.push(c);
        }
    }
    if unc {
        out.insert(0, '\\');
    }
    while out.len() > 3 && out.ends_with('\\') {
        out.pop();
    }
    let b = out.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    if drive {
        out[..1].make_ascii_uppercase();
    }
    (drive || (unc && out.len() > 2)).then_some(out)
}

fn norm_path(raw: &str) -> Option<String> {
    norm_with(raw, cfg!(windows))
}

fn key(path: &str) -> String {
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_string()
    }
}

fn last_segment(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .find(|s| !s.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "project".into()
    } else {
        out.to_string()
    }
}

fn unique_slug(base: &str, used: &HashSet<String>) -> String {
    if !used.contains(base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let s = format!("{base}-{n}");
        if !used.contains(&s) {
            return s;
        }
        n += 1;
    }
}

fn safe_name(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let out = out.trim_start_matches('.');
    if out.is_empty() {
        "note".into()
    } else {
        out.to_string()
    }
}

fn one_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
}

fn yaml(v: &str) -> String {
    let v = one_line(v);
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

fn unyaml(v: &str) -> String {
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
    v.split(" #").next().unwrap_or_default().trim().to_string()
}

fn claude_encode(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(s.get(i + 1..i + 3)?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn strip_hash(id: &str) -> &str {
    match id.rsplit_once('-') {
        Some((head, tail))
            if !head.is_empty()
                && tail.len() == 8
                && tail.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            head
        }
        _ => id,
    }
}

fn strip_frontmatter(s: &str) -> &str {
    let s = s.trim_start_matches('\u{feff}');
    let Some(rest) = s.strip_prefix("---") else {
        return s;
    };
    let mut off = 0;
    for (i, line) in rest.split_inclusive('\n').enumerate() {
        off += line.len();
        if i == 0 {
            if !line.trim().is_empty() {
                return s;
            }
        } else if line.trim_end() == "---" {
            return rest[off..].trim_start_matches(['\r', '\n']);
        }
    }
    s
}

fn front_field(s: &str, field: &str) -> Option<String> {
    let mut lines = s.trim_start_matches('\u{feff}').lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    for line in lines {
        let line = line.trim_end();
        if line == "---" {
            break;
        }
        if let Some(v) = line.strip_prefix(field).and_then(|r| r.strip_prefix(':')) {
            return Some(unyaml(v));
        }
    }
    None
}

fn tilde(p: &Path, home: &Path) -> String {
    p.strip_prefix(home).map_or_else(
        |_| p.to_string_lossy().into_owned(),
        |rel| {
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            format!("~/{}", parts.join("/"))
        },
    )
}

fn tool_name(id: &str) -> &'static str {
    TOOLS.iter().find(|(t, _)| *t == id).map_or("", |(_, n)| *n)
}

fn tool_dirs(home: &Path, id: &str) -> Vec<PathBuf> {
    if id == "opencode" {
        vec![
            sub(home, ".config/opencode"),
            sub(home, ".local/share/opencode"),
        ]
    } else {
        vec![home.join(format!(".{id}"))]
    }
}

fn note(
    env: &Env,
    tool: &'static str,
    path: &Path,
    title: String,
    kind: &str,
    folder: Option<String>,
    place: Place,
) -> Option<Cand> {
    let ext_md = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md"));
    if !ext_md || is_secret(path.strip_prefix(&env.home).unwrap_or(path)) {
        return None;
    }
    let meta = fs::metadata(path).ok().filter(fs::Metadata::is_file)?;
    Some(Cand {
        tool,
        note: NoteCandidate {
            source: path.to_string_lossy().into_owned(),
            title,
            kind: kind.into(),
            project_folder: folder,
            bytes: meta.len(),
        },
        place,
    })
}

fn flat(env: &Env, tool: &'static str, dir: &Path, file: &str) -> Option<Cand> {
    note(
        env,
        tool,
        &dir.join(file),
        format!("{} {file}", tool_name(tool)),
        "instructions",
        None,
        Place::Flat(file.into()),
    )
}

fn skills(env: &Env, tool: &'static str, dir: &Path) -> Vec<Cand> {
    dirs(dir)
        .into_iter()
        .filter_map(|d| {
            let name = file_name(&d);
            note(
                env,
                tool,
                &d.join("SKILL.md"),
                name.clone(),
                "skill",
                None,
                Place::Skill(name),
            )
        })
        .collect()
}

fn memories(
    env: &Env,
    tool: &'static str,
    dir: &Path,
    folder: Option<&str>,
    fallback: &str,
) -> Vec<Cand> {
    let label = folder.map_or_else(|| fallback.to_string(), last_segment);
    files_with(dir, "md")
        .into_iter()
        .filter_map(|p| {
            let s = stem(&p);
            let title = if s.eq_ignore_ascii_case("memory") {
                format!("{label} memory")
            } else {
                s
            };
            let file = file_name(&p);
            note(
                env,
                tool,
                &p,
                title,
                "memory",
                folder.map(str::to_string),
                Place::Memory(fallback.to_string(), file),
            )
        })
        .collect()
}

fn claude_keys(home: &Path) -> Vec<String> {
    #[derive(Deserialize)]
    struct ClaudeJson {
        #[serde(default)]
        projects: HashMap<String, serde::de::IgnoredAny>,
    }
    let mut out: Vec<String> = [home.join(".claude.json"), sub(home, ".claude/.claude.json")]
        .iter()
        .filter_map(|p| read_json::<ClaudeJson>(p, JSON_CAP))
        .flat_map(|j| j.projects.into_keys())
        .collect();
    out.sort();
    out.dedup();
    out
}

fn claude_projects(home: &Path, keys: &[String]) -> Vec<(String, Option<String>, Option<i64>)> {
    dirs(&sub(home, ".claude/projects"))
        .into_iter()
        .map(|d| {
            let dir = file_name(&d);
            let newest = files_with(&d, "jsonl")
                .into_iter()
                .filter_map(|f| mtime_ms(&f).map(|t| (t, f)))
                .max_by_key(|(t, _)| *t);
            let cwd = newest.as_ref().and_then(|(_, f)| {
                head_lines(f, 12).iter().find_map(|l| {
                    let v: Value = serde_json::from_str(l).ok()?;
                    v.get("cwd")?.as_str().map(str::to_string)
                })
            });
            let folder = cwd
                .or_else(|| keys.iter().find(|k| claude_encode(k) == dir).cloned())
                .and_then(|f| norm_path(&f));
            (dir, folder, newest.map(|(t, _)| t))
        })
        .collect()
}

fn collect_rollouts(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    for p in entries(dir) {
        if p.is_dir() && depth > 0 {
            collect_rollouts(&p, depth - 1, out);
        } else if file_name(&p).starts_with("rollout-")
            && p.extension().is_some_and(|e| e == "jsonl")
        {
            out.push(p);
        }
    }
}

fn codex_hits(home: &Path) -> Vec<Hit> {
    let mut files = Vec::new();
    for root in ["sessions", "archived_sessions"] {
        collect_rollouts(&home.join(".codex").join(root), 4, &mut files);
    }
    files.sort_by_key(|f| std::cmp::Reverse(file_name(f)));
    files.truncate(CODEX_FILES);
    files
        .iter()
        .filter_map(|f| {
            let line = head_lines(f, 1).into_iter().next()?;
            let v: Value = serde_json::from_str(&line).ok()?;
            if v.get("type")?.as_str()? != "session_meta" {
                return None;
            }
            let cwd = v.get("payload")?.get("cwd")?.as_str()?;
            Some(hit(cwd, "codex", mtime_ms(f)))
        })
        .collect()
}

fn grok_hits(home: &Path) -> Vec<Hit> {
    dirs(&sub(home, ".grok/sessions"))
        .iter()
        .filter_map(|d| Some(hit(&percent_decode(&file_name(d))?, "grok", mtime_ms(d))))
        .collect()
}

fn opencode_hits(home: &Path) -> Vec<Hit> {
    files_with(&sub(home, ".local/share/opencode/storage/project"), "json")
        .iter()
        .filter_map(|f| {
            let v: Value = read_json(f, NOTE_CAP)?;
            Some(hit(v.get("worktree")?.as_str()?, "opencode", mtime_ms(f)))
        })
        .collect()
}

fn unique_by_name(hits: &[Hit], label: &str) -> Option<String> {
    let label = label.to_lowercase();
    let mut found: Vec<String> = hits
        .iter()
        .filter_map(|h| norm_path(&h.path))
        .filter(|p| last_segment(p).to_lowercase() == label)
        .collect();
    found.sort_by_key(|p| key(p));
    found.dedup_by_key(|p| key(p));
    if found.len() == 1 {
        found.pop()
    } else {
        None
    }
}

fn gather(env: &Env) -> (Vec<Cand>, Vec<Hit>) {
    let h = &env.home;
    let mut cands = Vec::new();
    let keys = claude_keys(h);
    let mut hits: Vec<Hit> = keys.iter().map(|k| hit(k, "claude", None)).collect();

    let claude = h.join(".claude");
    cands.extend(flat(env, "claude", &claude, "CLAUDE.md"));
    cands.extend(skills(env, "claude", &claude.join("skills")));
    for (dir, folder, at) in claude_projects(h, &keys) {
        if let Some(f) = &folder {
            hits.push(hit(f, "claude", at));
        }
        let mem = claude.join("projects").join(&dir).join("memory");
        cands.extend(memories(
            env,
            "claude",
            &mem,
            folder.as_deref(),
            &slugify(&dir),
        ));
    }

    let codex = h.join(".codex");
    for f in ["AGENTS.md", "instructions.md"] {
        cands.extend(flat(env, "codex", &codex, f));
    }
    cands.extend(skills(env, "codex", &codex.join("skills")));
    for p in files_with(&codex.join("skills"), "md") {
        let s = stem(&p);
        cands.extend(note(
            env,
            "codex",
            &p,
            s.clone(),
            "skill",
            None,
            Place::Skill(s),
        ));
    }

    let grok = h.join(".grok");
    let grok_found = grok_hits(h);
    for f in ["GROK.md", "AGENTS.md"] {
        cands.extend(flat(env, "grok", &grok, f));
    }
    cands.extend(skills(env, "grok", &grok.join("skills")));
    let mem = grok.join("memory-v2");
    cands.extend(memories(env, "grok", &mem.join("global"), None, "global"));
    for ws in dirs(&mem.join("workspaces")) {
        let id = file_name(&ws);
        let label = strip_hash(&id);
        let folder = unique_by_name(&grok_found, label);
        let mut found = memories(env, "grok", &ws, folder.as_deref(), &slugify(&id));
        for c in &mut found {
            if folder.is_none() && c.note.title.ends_with(" memory") {
                c.note.title = format!("{label} memory");
            }
        }
        cands.extend(found);
    }
    hits.extend(grok_found);

    let cursor = h.join(".cursor");
    cands.extend(skills(env, "cursor", &cursor.join("skills")));
    for p in files_with(&cursor.join("rules"), "md") {
        let file = file_name(&p);
        cands.extend(note(
            env,
            "cursor",
            &p,
            stem(&p),
            "instructions",
            None,
            Place::Flat(file),
        ));
    }

    let opencode = sub(h, ".config/opencode");
    cands.extend(flat(env, "opencode", &opencode, "AGENTS.md"));
    for d in ["skills", "skill"] {
        cands.extend(skills(env, "opencode", &opencode.join(d)));
    }

    cands.extend(flat(env, "gemini", &h.join(".gemini"), "GEMINI.md"));
    (cands, hits)
}

fn merge_folders(env: &Env, hits: Vec<Hit>) -> Vec<FolderCandidate> {
    let norm_key = |p: &Path| norm_path(&p.to_string_lossy()).map(|s| key(&s));
    let home = norm_key(&env.home).unwrap_or_default();
    let roots: Vec<String> = [&env.parzi, &env.temp]
        .into_iter()
        .filter_map(|p| norm_key(p))
        .collect();
    let skip = |path: &str| {
        let k = key(path);
        let p = Path::new(path);
        p.parent().is_none()
            || k == home
            || roots
                .iter()
                .any(|r| k == *r || k.starts_with(&format!("{r}{MAIN_SEPARATOR}")))
            || p.components().any(|c| {
                matches!(c, Component::Normal(n) if {
                    let n = n.to_string_lossy();
                    n.starts_with('.') || n.eq_ignore_ascii_case("AppData")
                })
            })
    };
    let mut by: HashMap<String, FolderCandidate> = HashMap::new();
    for h in hits {
        let Some(path) = norm_path(&h.path) else {
            continue;
        };
        if skip(&path) {
            continue;
        }
        let f = by.entry(key(&path)).or_insert_with(|| FolderCandidate {
            name: last_segment(&path),
            path,
            sources: Vec::new(),
            last_used: None,
            exists: false,
        });
        if !f.sources.iter().any(|s| s == h.tool) {
            f.sources.push(h.tool.to_string());
        }
        f.last_used = f.last_used.max(h.at);
    }
    let mut out: Vec<FolderCandidate> = by.into_values().collect();
    out.sort_by(|a, b| {
        b.last_used
            .cmp(&a.last_used)
            .then_with(|| key(&a.path).cmp(&key(&b.path)))
    });
    out.truncate(MAX_FOLDERS);
    for f in &mut out {
        f.exists = Path::new(&f.path).is_dir();
        f.sources
            .sort_by_key(|s| TOOLS.iter().position(|(t, _)| *t == s.as_str()));
    }
    out
}

fn local_scan(env: &Env) -> (Vec<ToolSource>, Vec<FolderCandidate>) {
    let (cands, mut hits) = gather(env);
    hits.extend(codex_hits(&env.home));
    hits.extend(opencode_hits(&env.home));
    let tools = TOOLS
        .iter()
        .map(|(id, name)| ToolSource {
            id: (*id).to_string(),
            name: (*name).to_string(),
            found: tool_dirs(&env.home, id).iter().any(|d| d.is_dir()),
            notes: cands
                .iter()
                .filter(|c| c.tool == *id)
                .map(|c| c.note.clone())
                .collect(),
        })
        .collect();
    (tools, merge_folders(env, hits))
}

fn project_index(vault: &Path, home: &Path) -> (HashMap<String, String>, HashSet<String>) {
    let mut index = HashMap::new();
    let mut used = HashSet::new();
    for p in files_with(&vault.join("projects"), "md") {
        let slug = stem(&p);
        used.insert(slug.to_lowercase());
        let folder = read_capped(&p, NOTE_CAP)
            .ok()
            .and_then(|t| front_field(&t, "folder"))
            .map(
                |f| match f.strip_prefix("~/").or_else(|| f.strip_prefix("~\\")) {
                    Some(rest) => sub(home, &rest.replace('\\', "/"))
                        .to_string_lossy()
                        .into_owned(),
                    None => f,
                },
            )
            .and_then(|f| norm_path(&f));
        if let Some(folder) = folder {
            index.entry(key(&folder)).or_insert(slug);
        }
    }
    (index, used)
}

fn write_file(p: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(p, text).map_err(|e| e.to_string())
}

fn ensure_project(
    vault: &Path,
    raw: &str,
    index: &mut HashMap<String, String>,
    used: &mut HashSet<String>,
) -> Result<String, String> {
    let path = norm_path(raw).ok_or("not an absolute folder path")?;
    let name = last_segment(&path);
    if Path::new(&path).parent().is_none() || name.is_empty() {
        return Err("a drive or root folder cannot be a project".into());
    }
    let k = key(&path);
    if index.contains_key(&k) {
        return Ok(k);
    }
    let slug = unique_slug(&slugify(&name), used);
    write_file(
        &vault.join("projects").join(format!("{slug}.md")),
        &format!(
            "---\ntitle: {}\nfolder: {}\n---\n\n# {}\n",
            yaml(&name),
            yaml(&path),
            one_line(&name)
        ),
    )?;
    used.insert(slug.clone());
    index.insert(k.clone(), slug);
    Ok(k)
}

fn write_note(
    env: &Env,
    vault: &Path,
    c: &Cand,
    index: &HashMap<String, String>,
) -> Result<(), String> {
    let src = Path::new(&c.note.source);
    let text = read_capped(src, NOTE_CAP)?;
    let folder = c.note.project_folder.as_deref();
    let linked = folder.and_then(|f| index.get(&key(f)));
    let dir = vault.join("imported").join(c.tool);
    let file = match &c.place {
        Place::Flat(name) => dir.join(safe_name(name)),
        Place::Skill(name) => dir.join("skills").join(format!("{}.md", safe_name(name))),
        Place::Memory(fallback, name) => {
            let group = linked
                .cloned()
                .or_else(|| folder.map(|f| slugify(&last_segment(f))))
                .unwrap_or_else(|| fallback.clone());
            dir.join("memory").join(group).join(safe_name(name))
        }
    };
    let projects = linked
        .map(|s| format!("projects: [{s}]\n"))
        .unwrap_or_default();
    let description = front_field(&text, "description")
        .map(|d| one_line(&d))
        .filter(|d| !d.is_empty())
        .map(|d| format!("description: {}\n", yaml(&d)))
        .unwrap_or_default();
    let mut out = format!(
        "---\ntitle: {}\nsource: {}\n{description}{projects}---\n\n{}",
        yaml(&c.note.title),
        yaml(&format!("{}:{}", c.tool, tilde(src, &env.home))),
        strip_frontmatter(&text)
    );
    if !out.ends_with('\n') {
        out.push('\n');
    }
    write_file(&file, &out)
}

fn import_with(env: &Env, notes: &[String], folders: &[String]) -> ImportReport {
    let vault = env.parzi.join("brain");
    let mut report = ImportReport::default();
    let (mut index, mut used) = project_index(&vault, &env.home);
    let mut done = HashSet::new();
    for raw in folders {
        match ensure_project(&vault, raw, &mut index, &mut used) {
            Ok(k) => {
                if done.insert(k) {
                    report.projects += 1;
                }
            }
            Err(e) => report.skipped.push(format!("{raw}: {e}")),
        }
    }
    let (cands, _) = gather(env);
    for src in notes {
        let Some(c) = cands.iter().find(|c| c.note.source == *src) else {
            report
                .skipped
                .push(format!("{src}: not one of the scanned sources"));
            continue;
        };
        match write_note(env, &vault, c, &index) {
            Ok(()) => report.notes += 1,
            Err(e) => report.skipped.push(format!("{src}: {e}")),
        }
    }
    report
}

fn browser_root(id: &str) -> Option<PathBuf> {
    let (base, rel): (PathBuf, &str) = if cfg!(windows) {
        (
            std::env::var_os("LOCALAPPDATA")?.into(),
            match id {
                "brave" => "BraveSoftware/Brave-Browser/User Data",
                "edge" => "Microsoft/Edge/User Data",
                "chrome" => "Google/Chrome/User Data",
                _ => return None,
            },
        )
    } else if cfg!(target_os = "macos") {
        (
            sub(&std::env::home_dir()?, "Library/Application Support"),
            match id {
                "brave" => "BraveSoftware/Brave-Browser",
                "edge" => "Microsoft Edge",
                "chrome" => "Google/Chrome",
                _ => return None,
            },
        )
    } else {
        (
            std::env::home_dir()?.join(".config"),
            match id {
                "brave" => "BraveSoftware/Brave-Browser",
                "edge" => "microsoft-edge",
                "chrome" => "google-chrome",
                _ => return None,
            },
        )
    };
    Some(sub(&base, rel))
}

fn browser_profiles(root: &Path) -> Vec<String> {
    dirs(root)
        .into_iter()
        .filter(|d| {
            let n = file_name(d);
            (n == "Default" || n.starts_with("Profile "))
                && (d.join("Bookmarks").is_file() || d.join("History").is_file())
        })
        .map(|d| file_name(&d))
        .collect()
}

fn scan_browsers() -> Vec<BrowserSource> {
    let mut out = Vec::new();
    for (id, name) in BROWSERS {
        let Some(root) = browser_root(id) else {
            continue;
        };
        for profile in browser_profiles(&root) {
            let dir = root.join(&profile);
            out.push(BrowserSource {
                id: id.into(),
                name: name.into(),
                bookmarks: load_bookmarks(&dir).len(),
                history: dir.join("History").is_file(),
                profile,
            });
        }
    }
    out
}

fn valid_profile(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 64
        && !p.contains(['/', '\\', ':'])
        && matches!(
            Path::new(p).components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        )
}

fn browser_data(id: &str, profile: &str) -> Result<BrowserData, String> {
    if !BROWSERS.iter().any(|(b, _)| *b == id) {
        return Err(format!("unknown browser: {id}"));
    }
    if !valid_profile(profile) {
        return Err("invalid browser profile".into());
    }
    let dir = browser_root(id)
        .map(|r| r.join(profile))
        .filter(|d| d.is_dir())
        .ok_or("that browser profile was not found")?;
    let history = load_history(&dir).unwrap_or_else(|e| {
        tracing::warn!("browser history not read: {e}");
        Vec::new()
    });
    Ok(BrowserData {
        bookmarks: load_bookmarks(&dir),
        history,
    })
}

fn is_web(url: &str) -> bool {
    let u = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://")
}

fn chrome_ms(t: i64) -> i64 {
    if t <= 0 {
        0
    } else {
        (t / 1000 - CHROME_EPOCH_MS).max(0)
    }
}

fn walk_bookmarks(node: &Value, folder: &str, out: &mut Vec<Bookmark>) {
    let name = node.get("name").and_then(Value::as_str).unwrap_or_default();
    if node.get("type").and_then(Value::as_str) == Some("url") {
        if let Some(url) = node
            .get("url")
            .and_then(Value::as_str)
            .filter(|u| is_web(u))
        {
            out.push(Bookmark {
                title: name.to_string(),
                url: url.to_string(),
                folder: folder.to_string(),
            });
        }
        return;
    }
    let path = match (folder.is_empty(), name.is_empty()) {
        (true, _) => name.to_string(),
        (false, true) => folder.to_string(),
        (false, false) => format!("{folder}/{name}"),
    };
    for child in node
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        walk_bookmarks(child, &path, out);
    }
}

fn bookmarks_from(v: &Value) -> Vec<Bookmark> {
    let mut out = Vec::new();
    if let Some(roots) = v.get("roots") {
        for k in ["bookmark_bar", "other", "synced"] {
            if let Some(node) = roots.get(k) {
                walk_bookmarks(node, "", &mut out);
            }
        }
    }
    out
}

fn load_bookmarks(dir: &Path) -> Vec<Bookmark> {
    read_json::<Value>(&dir.join("Bookmarks"), JSON_CAP)
        .map(|v| bookmarks_from(&v))
        .unwrap_or_default()
}

fn load_history(dir: &Path) -> Result<Vec<Visit>, String> {
    let src = dir.join("History");
    if !src.is_file() {
        return Ok(Vec::new());
    }
    let tmp = TempDir(std::env::temp_dir().join(format!("parzi-history-{}", uuid::Uuid::new_v4())));
    fs::create_dir_all(&tmp.0).map_err(|e| e.to_string())?;
    let db = tmp.0.join("History");
    fs::copy(&src, &db).map_err(|e| format!("could not copy the history: {e}"))?;
    let wal = dir.join("History-wal");
    if wal.is_file() {
        let _ = fs::copy(&wal, tmp.0.join("History-wal"));
    }
    let conn = Connection::open_with_flags(
        &db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(HISTORY_SQL).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Visit {
                url: r.get(0)?,
                title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                visits: r.get(2)?,
                last_visit: chrome_ms(r.get(3)?),
            })
        })
        .map_err(|e| e.to_string())?;
    let out = rows
        .filter_map(Result::ok)
        .filter(|v| is_web(&v.url))
        .collect();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> TempDir {
        let p = std::env::temp_dir().join(format!("parzi-onboard-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn put(p: &Path, s: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }

    fn abs(rel: &str) -> String {
        if cfg!(windows) {
            format!("C:\\{}", rel.replace('/', "\\"))
        } else {
            format!("/{rel}")
        }
    }

    fn env_in(t: &TempDir) -> Env {
        Env {
            home: t.0.join("home"),
            parzi: t.0.join("home").join(".parzi"),
            temp: t.0.join("scratch"),
        }
    }

    fn fixture(t: &TempDir) -> Env {
        let env = env_in(t);
        let h = &env.home;
        let alpha = abs("work/alpha");
        let projects = serde_json::json!({
            "projects": {
                alpha.clone(): {"secretish": 1},
                abs("work/beta"): {},
                h.to_string_lossy(): {},
                h.join(".parzi").join("x").to_string_lossy(): {}
            },
            "oauthAccount": {"token": "x"}
        });
        put(&h.join(".claude.json"), &projects.to_string());
        put(&sub(h, ".claude/.credentials.json"), "{}");
        put(
            &sub(h, ".claude/skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: Runs the demo\n---\n\n# Demo\n",
        );
        put(&sub(h, ".claude/skills/empty/notes.txt"), "x");
        let dir = claude_encode(&alpha);
        let line = serde_json::json!({"type": "user", "cwd": alpha}).to_string();
        put(
            &sub(h, &format!(".claude/projects/{dir}/s.jsonl")),
            &format!("{{\"type\":\"queue-operation\"}}\n{line}\n"),
        );
        put(
            &sub(h, &format!(".claude/projects/{dir}/memory/MEMORY.md")),
            "---\nname: m\n---\nremember alpha\n",
        );
        put(
            &sub(
                h,
                &format!(
                    ".claude/projects/{}/memory/MEMORY.md",
                    claude_encode(&abs("work/beta"))
                ),
            ),
            "beta notes",
        );
        put(&sub(h, ".claude/projects/C--lost/memory/MEMORY.md"), "lost");
        put(&sub(h, ".codex/auth.json"), "{}");
        put(&sub(h, ".codex/AGENTS.md"), "codex rules");
        put(&sub(h, ".codex/skills/.system/sys/SKILL.md"), "system");
        put(&sub(h, ".codex/skills/my-token-thing.md"), "x");
        let meta = serde_json::json!({"type": "session_meta", "payload": {"cwd": format!("{}/", abs("work/alpha").to_uppercase())}});
        put(
            &sub(
                h,
                ".codex/sessions/2026/01/02/rollout-2026-01-02T00-00-00-a.jsonl",
            ),
            &format!("{meta}\n{{}}\n"),
        );
        let gamma = abs("work/gamma");
        let enc: String = gamma
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() {
                    char::from(b).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        fs::create_dir_all(sub(h, &format!(".grok/sessions/{enc}/s1"))).unwrap();
        put(&sub(h, ".grok/memory-v2/global/MEMORY.md"), "grok global");
        put(
            &sub(h, ".grok/memory-v2/workspaces/gamma-1234abcd/MEMORY.md"),
            "g",
        );
        put(
            &sub(h, ".grok/memory-v2/workspaces/zeta-0000ffff/MEMORY.md"),
            "z",
        );
        fs::create_dir_all(h.join(".cursor")).unwrap();
        env
    }

    #[test]
    fn slugs_are_lowercase_dashed_and_unique() {
        assert_eq!(slugify("My Project_v2!"), "my-project-v2");
        assert_eq!(slugify("--__--"), "project");
        assert_eq!(slugify("isarwebsites.de"), "isarwebsites-de");
        let used: HashSet<String> = ["alpha", "alpha-2"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(unique_slug("alpha", &used), "alpha-3");
        assert_eq!(unique_slug("beta", &used), "beta");
        assert_eq!(safe_name("../evil name.md"), "-evil-name.md");
        assert_eq!(safe_name("..."), "note");
    }

    #[test]
    fn chromium_time_converts_to_unix_ms() {
        assert_eq!(chrome_ms(13_300_000_000_000_000), 1_655_526_400_000);
        assert_eq!(chrome_ms(0), 0);
        assert_eq!(chrome_ms(5), 0);
    }

    #[test]
    fn bookmarks_walk_keeps_folder_paths_and_web_urls() {
        let v = serde_json::json!({"roots": {
            "bookmark_bar": {"type": "folder", "name": "Bookmarks bar", "children": [
                {"type": "url", "name": "Rust", "url": "https://rust-lang.org"},
                {"type": "folder", "name": "Dev", "children": [
                    {"type": "url", "name": "Docs", "url": "http://docs.rs"},
                    {"type": "url", "name": "Let", "url": "javascript:alert(1)"}
                ]}
            ]},
            "other": {"type": "folder", "name": "Other bookmarks", "children": [
                {"type": "url", "name": "X", "url": "HTTPS://example.com"}
            ]},
            "synced": {"type": "folder", "name": "Mobile bookmarks", "children": []}
        }});
        let b = bookmarks_from(&v);
        let got: Vec<(&str, &str)> = b
            .iter()
            .map(|b| (b.title.as_str(), b.folder.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("Rust", "Bookmarks bar"),
                ("Docs", "Bookmarks bar/Dev"),
                ("X", "Other bookmarks")
            ]
        );
    }

    #[test]
    fn paths_normalize_for_both_platforms() {
        assert_eq!(
            norm_with("c:/Users/x/Proj/", true).as_deref(),
            Some(r"C:\Users\x\Proj")
        );
        assert_eq!(norm_with(r"\\?\C:\a\\b\", true).as_deref(), Some(r"C:\a\b"));
        assert_eq!(
            norm_with(r"\\?\UNC\srv\share\p", true).as_deref(),
            Some(r"\\srv\share\p")
        );
        assert_eq!(norm_with("C:/", true).as_deref(), Some("C:\\"));
        assert_eq!(norm_with("relative/x", true), None);
        assert_eq!(norm_with("C:", true), None);
        assert_eq!(
            norm_with("/home//me/p/", false).as_deref(),
            Some("/home/me/p")
        );
        assert_eq!(norm_with("/", false).as_deref(), Some("/"));
        assert_eq!(norm_with("me/p", false), None);
    }

    #[test]
    fn folders_dedupe_and_drop_home_temp_hidden() {
        let t = tmp();
        let env = env_in(&t);
        let parzi = env.parzi.join("scratch").to_string_lossy().into_owned();
        let tmpd = env.temp.join("x").to_string_lossy().into_owned();
        let home = env.home.to_string_lossy().into_owned();
        let hidden = abs("work/.lanes/x");
        let a = abs("work/alpha");
        let a2 = format!("{}/", a.to_uppercase());
        let hits = vec![
            hit(&a, "codex", Some(5)),
            hit(&a2, "claude", Some(9)),
            hit(&a, "claude", None),
            hit(&abs("work/beta"), "grok", None),
            hit(&home, "claude", Some(99)),
            hit(&parzi, "claude", Some(99)),
            hit(&tmpd, "claude", Some(99)),
            hit(&hidden, "grok", Some(99)),
            hit(&abs(""), "opencode", Some(99)),
            hit("relative", "claude", Some(99)),
        ];
        let out = merge_folders(&env, hits);
        let paths: Vec<&str> = out.iter().map(|f| f.path.as_str()).collect();
        if cfg!(windows) {
            assert_eq!(paths, [a.as_str(), abs("work/beta").as_str()]);
            assert_eq!(out[0].sources, ["claude", "codex"]);
            assert_eq!(out[0].last_used, Some(9));
        } else {
            assert_eq!(paths.len(), 3);
        }
        assert_eq!(out.last().unwrap().last_used, None);
        assert!(!out[0].exists);
        assert_eq!(out[0].name, last_segment(&out[0].path));
    }

    #[test]
    fn credential_files_are_denied() {
        for p in [
            ".claude/.credentials.json",
            ".codex/auth.json",
            ".codex/cap_sid",
            ".codex/.sandbox-secrets/a.md",
            ".gemini/oauth_creds.json",
            ".gemini/jetski-standalone-oauth-token",
            "Default/Login Data",
            "Default/Web Data",
            "Default/Cookies",
            ".codex/state_5.sqlite",
            "x/id.key",
        ] {
            assert!(is_secret(Path::new(p)), "{p}");
        }
        for p in [
            ".claude/skills/demo/SKILL.md",
            ".claude/CLAUDE.md",
            ".claude.json",
        ] {
            assert!(!is_secret(Path::new(p)), "{p}");
        }
        assert!(read_capped(Path::new("auth.json"), NOTE_CAP).is_err());
    }

    #[test]
    fn frontmatter_is_stripped_and_read() {
        assert_eq!(
            strip_frontmatter("---\nname: x\n---\n\n# Body\n"),
            "# Body\n"
        );
        assert_eq!(
            strip_frontmatter("\u{feff}---\r\na: b\r\n---\r\nBody"),
            "Body"
        );
        assert_eq!(strip_frontmatter("---- rule\ntext"), "---- rule\ntext");
        assert_eq!(strip_frontmatter("---\nunclosed"), "---\nunclosed");
        assert_eq!(strip_frontmatter("# plain"), "# plain");
        let doc = "---\ntitle: A\nfolder: \"C:\\a\"\n---\n";
        assert_eq!(front_field(doc, "folder").as_deref(), Some("C:\\a"));
        assert_eq!(front_field(doc, "missing"), None);
        assert_eq!(yaml("alpha memory"), "alpha memory");
        assert_eq!(yaml(r"C:\work\alpha"), r"C:\work\alpha");
        let odd = r#"a #1, "b" \c"#;
        assert_eq!(unyaml(&yaml(odd)), odd);
        assert_eq!(unyaml("plain # note"), "plain");
    }

    #[test]
    fn misc_parsers() {
        assert_eq!(
            percent_decode("C%3A%5CStrohmann").as_deref(),
            Some("C:\\Strohmann")
        );
        assert_eq!(percent_decode("bad%zz"), None);
        assert_eq!(strip_hash("parzi-508be940"), "parzi");
        assert_eq!(strip_hash("website-redesign-aa6be69d"), "website-redesign");
        assert_eq!(strip_hash("plain"), "plain");
        assert_eq!(
            claude_encode(r"C:\Users\lucas\Desktop\Parzi"),
            "C--Users-lucas-Desktop-Parzi"
        );
        assert!(valid_profile("Default") && valid_profile("Profile 3"));
        for bad in ["", "..", ".", "a/b", "a\\b", "C:x", "/abs"] {
            assert!(!valid_profile(bad), "{bad}");
        }
        assert!(browser_data("firefox", "Default").is_err());
        assert!(browser_data("chrome", "../x").is_err());
    }

    #[test]
    fn scan_finds_notes_and_folders() {
        let t = tmp();
        let env = fixture(&t);
        let (tools, folders) = local_scan(&env);
        let ids: Vec<&str> = tools.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            ["claude", "codex", "grok", "cursor", "opencode", "gemini"]
        );
        let claude = &tools[0];
        assert!(claude.found);
        let srcs: Vec<String> = tools
            .iter()
            .flat_map(|t| {
                t.notes
                    .iter()
                    .map(|n| tilde(Path::new(&n.source), &env.home))
            })
            .collect();
        assert!(srcs.iter().all(|s| !s.contains("credentials")
            && !s.contains("auth.json")
            && !s.contains("token")));
        assert!(srcs.iter().all(|s| !s.contains(".system")));
        assert!(srcs.contains(&"~/.claude/skills/demo/SKILL.md".to_string()));
        let mem = claude
            .notes
            .iter()
            .find(|n| {
                n.kind == "memory"
                    && n.project_folder.as_deref() == Some(abs("work/alpha").as_str())
            })
            .expect("alpha memory");
        assert_eq!(mem.title, "alpha memory");
        assert!(claude
            .notes
            .iter()
            .any(|n| n.project_folder.as_deref() == Some(abs("work/beta").as_str())));
        let grok = &tools[2];
        let g = grok
            .notes
            .iter()
            .find(|n| n.source.contains("gamma-1234abcd"))
            .unwrap();
        assert_eq!(
            g.project_folder.as_deref(),
            Some(abs("work/gamma").as_str())
        );
        assert!(tools[3].found && tools[3].notes.is_empty());
        assert!(!tools[5].found);
        let paths: Vec<&str> = folders.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&abs("work/alpha").as_str()));
        assert!(paths.contains(&abs("work/gamma").as_str()));
        assert!(!paths.iter().any(|p| p.contains(".parzi")));
        if cfg!(windows) {
            let a = folders
                .iter()
                .find(|f| f.path == abs("work/alpha"))
                .unwrap();
            assert_eq!(a.sources, ["claude", "codex"]);
            assert!(a.last_used.is_some());
        }
    }

    #[test]
    fn import_writes_vault_layout_idempotently() {
        let t = tmp();
        let env = fixture(&t);
        let (tools, _) = local_scan(&env);
        let find = |part: &str| {
            tools
                .iter()
                .flat_map(|t| &t.notes)
                .find(|n| n.source.replace('\\', "/").contains(part))
                .unwrap()
                .source
                .clone()
        };
        let notes = vec![
            find(".claude/skills/demo/SKILL.md"),
            find(&format!(
                "{}/memory/MEMORY.md",
                claude_encode(&abs("work/alpha"))
            )),
            find(&format!(
                "{}/memory/MEMORY.md",
                claude_encode(&abs("work/beta"))
            )),
            find("C--lost/memory"),
            find(".codex/AGENTS.md"),
            find(".grok/memory-v2/global/MEMORY.md"),
            find("zeta-0000ffff"),
            env.home
                .join(".codex")
                .join("auth.json")
                .to_string_lossy()
                .into_owned(),
        ];
        let vault = env.parzi.join("brain");
        put(
            &vault.join("projects").join("alpha.md"),
            "---\ntitle: alpha\nfolder: /elsewhere/alpha\n---\n",
        );
        put(
            &vault.join("projects").join("mine.md"),
            "---\ntitle: mine\nfolder: ~/proj\n---\n",
        );
        let folders = vec![
            abs("work/alpha"),
            format!("{}/", abs("work/alpha")),
            env.home.join("proj").to_string_lossy().into_owned(),
            "relative".into(),
        ];
        for _ in 0..2 {
            let r = import_with(&env, &notes, &folders);
            assert_eq!(r.notes, 7);
            assert_eq!(r.projects, 2);
            assert_eq!(r.skipped.len(), 2, "{:?}", r.skipped);
        }
        let projects = files_with(&vault.join("projects"), "md");
        assert_eq!(projects.len(), 3);
        let p = fs::read_to_string(vault.join("projects").join("alpha-2.md")).unwrap();
        assert_eq!(
            p,
            format!(
                "---\ntitle: alpha\nfolder: {}\n---\n\n# alpha\n",
                abs("work/alpha")
            )
        );
        let skill = fs::read_to_string(sub(&vault, "imported/claude/skills/demo.md")).unwrap();
        assert_eq!(
            skill,
            "---\ntitle: demo\nsource: claude:~/.claude/skills/demo/SKILL.md\ndescription: Runs the demo\n---\n\n# Demo\n"
        );
        let mem =
            fs::read_to_string(sub(&vault, "imported/claude/memory/alpha-2/MEMORY.md")).unwrap();
        assert!(mem.starts_with("---\ntitle: alpha memory\nsource: claude:~/.claude/projects/"));
        assert!(mem.contains("projects: [alpha-2]\n---\n\nremember alpha\n"));
        let beta =
            fs::read_to_string(sub(&vault, "imported/claude/memory/beta/MEMORY.md")).unwrap();
        assert!(!beta.contains("projects:"));
        assert!(sub(&vault, "imported/claude/memory/c-lost/MEMORY.md").is_file());
        assert!(sub(&vault, "imported/codex/AGENTS.md").is_file());
        assert!(sub(&vault, "imported/grok/memory/global/MEMORY.md").is_file());
        assert!(sub(&vault, "imported/grok/memory/zeta-0000ffff/MEMORY.md").is_file());
    }
}
