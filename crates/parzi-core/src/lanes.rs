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
                })
                .collect(),
        })
        .collect())
}

/// Delete a workspace dir (`projects/<name>`). Refuses blanks, `default`,
/// and path escapes. Missing dirs are a no-op success so UI deletes stay
/// idempotent when the folder was removed by hand.
pub fn delete_project(name: &str) -> Result<()> {
    let clean = name.trim();
    if clean.is_empty() || clean == "default" {
        return Err(crate::error::ParziError::Config(
            "the default workspace can't be deleted".into(),
        ));
    }
    if clean.contains('/') || clean.contains('\\') || clean.contains("..") {
        return Err(crate::error::ParziError::Config("bad project name".into()));
    }
    let dir = crate::paths::projects_dir()?.join(clean);
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
