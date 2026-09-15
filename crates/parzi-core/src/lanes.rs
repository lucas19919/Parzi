use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::paths;

/// `projects/<name>/SYSTEM.md` + `parzi.toml`, lane overrides win.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LaneFile {
    #[serde(default)]
    pub model: Option<String>,
    /// auto | ask | deny
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    /// Working root this project/lane operates on. Lane wins over project.
    #[serde(default)]
    pub root: Option<String>,
    /// Execute worker tasks in an isolated git worktree rather than in-place.
    #[serde(default)]
    pub isolated_worktree: bool,
    /// 3-tier agent roster (`[roles.header]` / `[roles.orchestrator]` /
    /// `[roles.implementation]`). `roster` accepted as a legacy alias.
    #[serde(default, rename = "roles", alias = "roster")]
    pub roster: ProjectRoster,
}

/// One agent role in the 3-tier workspace model (Header / Orchestrator /
/// Implementation). All fields optional: unset = sensible auto defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRoleConfig {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
}

/// Per-project agent roster, stored inline in `parzi.toml` under `[roles.*]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectRoster {
    #[serde(default)]
    pub header: AgentRoleConfig,
    #[serde(default)]
    pub orchestrator: AgentRoleConfig,
    #[serde(default)]
    pub implementation: AgentRoleConfig,
}

/// Load the roster for a project (defaults when unset or file missing).
pub fn get_project_roster(project: &str) -> Result<ProjectRoster> {
    let clean = safe_name(project)?;
    let dir = paths::projects_dir()?.join(&clean);
    let raw = std::fs::read_to_string(dir.join("parzi.toml")).unwrap_or_default();
    if raw.trim().is_empty() {
        return Ok(ProjectRoster::default());
    }
    let file: LaneFile = toml::from_str(&raw).unwrap_or_default();
    Ok(file.roster)
}

/// Persist the roster, preserving every other `parzi.toml` key byte-for-byte
/// when possible (TOML round-trip via a value table merge).
pub fn save_project_roster(project: &str, roster: &ProjectRoster) -> Result<()> {
    let clean = safe_name(project)?;
    let dir = paths::projects_dir()?.join(&clean);
    std::fs::create_dir_all(&dir).map_err(crate::error::ParziError::Io)?;
    let path = dir.join("parzi.toml");
    let mut table: toml::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or(toml::Value::Table(toml::map::Map::new()));
    let roster_val = toml::Value::try_from(roster)
        .map_err(|e| crate::error::ParziError::Config(e.to_string()))?;
    if let toml::Value::Table(ref mut m) = table {
        m.insert("roles".to_string(), roster_val);
        m.remove("roster");
    }
    let text =
        toml::to_string(&table).map_err(|e| crate::error::ParziError::Config(e.to_string()))?;
    crate::atomic_write(&path, text.as_bytes())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectView {
    pub name: String,
    pub root: Option<String>,
    pub has_system: bool,
    pub lanes: Vec<LaneView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaneView {
    pub name: String,
    pub mode: String,
    pub model: Option<String>,
    pub root: Option<String>,
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub isolated_worktree: bool,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub name: String,
    pub system: Option<String>,
    pub defaults: LaneFile,
    pub root: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Lane {
    pub project: String,
    pub name: String,
    pub system: Option<String>,
    /// Resolved: lane file > project file > global default.
    pub mode: String,
    pub model: Option<String>,
    pub allowed_tools: Vec<String>,
    /// Resolved working root: lane > project > None.
    pub root: Option<String>,
    /// Resolved worktree flag: lane > project defaults.
    pub isolated_worktree: bool,
}

fn read_system(dir: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(dir.join("SYSTEM.md")).ok()
}

fn read_lane_file(dir: &std::path::Path) -> LaneFile {
    std::fs::read_to_string(dir.join("parzi.toml"))
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

/// Scan `~/.parzi/projects`. Missing dir = zero projects, not an error.
pub fn scan_projects() -> Result<Vec<(Project, Vec<Lane>)>> {
    let root = paths::projects_dir()?;
    let mut out = vec![];
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let file = read_lane_file(&p);
        let project = Project {
            name: name.clone(),
            system: read_system(&p),
            root: file.root.clone(),
            defaults: file.clone(),
        };
        let mut lanes = vec![];
        if let Ok(le) = std::fs::read_dir(p.join("lanes")) {
            for l in le.flatten() {
                if !l.path().is_dir() {
                    continue;
                }
                let lf = read_lane_file(&l.path());
                lanes.push(Lane {
                    project: name.clone(),
                    name: l.file_name().to_string_lossy().to_string(),
                    system: read_system(&l.path()).or_else(|| project.system.clone()),
                    mode: lf
                        .mode
                        .or(file.mode.clone())
                        .unwrap_or_else(|| "ask".into()),
                    model: lf.model.or(file.model.clone()),
                    allowed_tools: if lf.allowed_tools.is_empty() {
                        file.allowed_tools.clone()
                    } else {
                        lf.allowed_tools
                    },
                    root: lf.root.or(file.root.clone()),
                    isolated_worktree: lf.isolated_worktree || file.isolated_worktree,
                });
            }
        }
        out.push((project, lanes));
    }
    Ok(out)
}

/// Serializable project tree for the UI project menu.
pub fn project_views() -> Result<Vec<ProjectView>> {
    Ok(scan_projects()?
        .into_iter()
        .map(|(p, lanes)| ProjectView {
            name: p.name,
            root: p.root,
            has_system: p.system.is_some(),
            lanes: lanes
                .into_iter()
                .map(|l| LaneView {
                    name: l.name,
                    mode: l.mode,
                    model: l.model,
                    root: l.root,
                    allowed_tools: l.allowed_tools,
                    isolated_worktree: l.isolated_worktree,
                })
                .collect(),
        })
        .collect())
}

/// Shared project/lane name gate (H-11): `[A-Za-z0-9_-]{1,64}`, no path
/// separators, no `.`/`..`, no Windows device names. Every name-taking
/// command must go through here.
pub fn safe_name(name: &str) -> Result<String> {
    let clean = name.trim().to_string();
    if clean.is_empty() || clean.len() > 64 {
        return Err(crate::error::ParziError::Config("bad project name".into()));
    }
    if clean == "." || clean == ".." {
        return Err(crate::error::ParziError::Config("bad project name".into()));
    }
    let ok = clean
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(crate::error::ParziError::Config("bad project name".into()));
    }
    for reserved in [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ] {
        if clean.eq_ignore_ascii_case(reserved) {
            return Err(crate::error::ParziError::Config("bad project name".into()));
        }
    }
    Ok(clean)
}

/// Delete a workspace dir (`projects/<name>`). Refuses blanks, `default`,
/// and path escapes. Missing dirs are a no-op success so UI deletes stay
/// idempotent when the folder was removed by hand.
pub fn delete_project(name: &str) -> Result<()> {
    let clean = safe_name(name)?;
    if clean == "default" {
        return Err(crate::error::ParziError::Config(
            "the default workspace can't be deleted".into(),
        ));
    }
    let dir = crate::paths::projects_dir()?.join(&clean);
    // Belt-and-braces: the resolved dir must stay under projects/.
    let base = crate::paths::projects_dir()?;
    if !dir.starts_with(&base) {
        return Err(crate::error::ParziError::Config("bad project name".into()));
    }
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(crate::error::ParziError::Io)?;
    }
    Ok(())
}

/// Resolve the working root for a project/lane pair. Empty when unset.
pub fn lane_root(project: &str, lane: &str) -> Option<String> {
    let scan = scan_projects().ok()?;
    for (p, lanes) in scan {
        if p.name != project {
            continue;
        }
        if lane.is_empty() {
            return p.root;
        }
        for l in lanes {
            if l.name == lane {
                return l.root;
            }
        }
        return p.root;
    }
    None
}

/// Tasks were removed (replaced by lanes + plan artifacts).
/// One-time legacy import: `projects/<p>/tasks/<id>/task.json` -> real lanes
/// (`projects/<p>/lanes/<id>/SYSTEM.md`). Idempotent (skips lanes that
/// already exist). Returns lanes created.
pub fn migrate_tasks_to_lanes(project: &str) -> Result<usize> {
    let tasks_dir = match paths::projects_dir() {
        Ok(p) => p.join(project).join("tasks"),
        Err(_) => return Ok(0),
    };
    if !tasks_dir.exists() {
        return Ok(0);
    }
    let proj_root = lane_root(project, "");
    let mut made = 0usize;
    let entries = std::fs::read_dir(&tasks_dir).map_err(crate::error::ParziError::Io)?;
    for e in entries.flatten() {
        let task_file = e.path().join("task.json");
        if !task_file.is_file() {
            continue;
        }
        let content = match std::fs::read_to_string(&task_file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let v: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = v
            .get("id")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
            continue;
        }
        let title = v
            .get("title")
            .and_then(|s| s.as_str())
            .unwrap_or(id.as_str())
            .to_string();
        let objective = v.get("objective").and_then(|s| s.as_str()).unwrap_or("");
        let plan_md = v.get("plan_md").and_then(|s| s.as_str()).unwrap_or("");
        let subfolder = v
            .get("subfolder")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .trim()
            .trim_matches(['/', '\\'])
            .to_string();
        let lanes_dir = match paths::projects_dir() {
            Ok(p) => p.join(project).join("lanes").join(&id),
            Err(_) => continue,
        };
        if lanes_dir.join("SYSTEM.md").exists() {
            continue;
        }
        std::fs::create_dir_all(&lanes_dir).map_err(crate::error::ParziError::Io)?;
        let mut sys = format!("# {title}\n\n");
        if !objective.trim().is_empty() {
            sys.push_str(&format!("## Objective\n\n{objective}\n\n"));
        }
        if !plan_md.trim().is_empty() {
            sys.push_str(&format!("## Plan\n\n{plan_md}\n"));
        }
        crate::atomic_write(&lanes_dir.join("SYSTEM.md"), sys.as_bytes())?;
        if !subfolder.is_empty() {
            if let Some(root) = proj_root.clone() {
                let full = std::path::PathBuf::from(root).join(&subfolder);
                let toml_text = format!("root = {full:?}\n");
                let _ = crate::atomic_write(&lanes_dir.join("parzi.toml"), toml_text.as_bytes());
            }
        }
        made += 1;
    }
    Ok(made)
}

/// Read cumulative knowledge for a project (`projects/<p>/KNOWLEDGE.md`).
pub fn read_knowledge(project: &str) -> Option<String> {
    let clean = safe_name(project).ok()?;
    let path = paths::project_knowledge_path(&clean).ok()?;
    std::fs::read_to_string(path).ok().filter(|t| !t.trim().is_empty())
}

/// Append an architectural note, discovered pattern, or gotcha to cumulative knowledge.
pub fn append_knowledge(project: &str, note: &str) -> Result<()> {
    let clean = safe_name(project)?;
    let path = paths::project_knowledge_path(&clean)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(crate::error::ParziError::Io)?;
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(crate::error::ParziError::Io)?;
    let trimmed = note.trim();
    if !trimmed.is_empty() {
        let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        writeln!(f, "- [{ts}] {trimmed}").map_err(crate::error::ParziError::Io)?;
    }
    Ok(())
}

