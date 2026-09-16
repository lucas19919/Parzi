//! Workspaces (PLAN §1.1): the set of repos a team works with, one small git
//! repo per workspace at `~/.parzi/workspaces/<name>/`. `workspace.toml` is
//! the file; projects live under `projects/<slug>/`. No network here — the
//! hub only ever mirrors what these files already say.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::{lanes, paths};

pub mod sync;

/// Solo = one person, no hub. Team = members and roles (§7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Solo,
    Team,
}

/// Alias so call sites can spell out what the enum is about.
pub type WorkspaceKind = Kind;

/// What a member may do (§7). The hub enforces it; locally it is a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    #[default]
    Owner,
    Maintainer,
    Member,
    Viewer,
}

impl Role {
    /// Approving a `critical` lease transfer: any maintainer or owner (§14.2).
    #[must_use]
    pub fn may_approve_critical(self) -> bool {
        matches!(self, Role::Owner | Role::Maintainer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub user: String,
    #[serde(default)]
    pub role: Role,
}

/// A code repo the workspace works with. `remote` is shared; `local_path` is
/// this machine's own mapping and is never authoritative for anyone else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRef {
    pub name: String,
    #[serde(default)]
    pub remote: String,
    #[serde(default = "default_branch")]
    pub default_branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<PathBuf>,
}

fn default_branch() -> String {
    "main".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub repos: Vec<RepoRef>,
    #[serde(default)]
    pub members: Vec<Member>,
}

impl Workspace {
    #[must_use]
    pub fn solo(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: Kind::Solo,
            repos: vec![],
            members: vec![],
        }
    }

    #[must_use]
    pub fn repo(&self, name: &str) -> Option<&RepoRef> {
        self.repos.iter().find(|r| r.name == name)
    }

    #[must_use]
    pub fn role_of(&self, user: &str) -> Option<Role> {
        self.members.iter().find(|m| m.user == user).map(|m| m.role)
    }
}

/// `~/.parzi/workspaces/<name>`. Infallible on purpose (the UI wants a path,
/// not a `Result`); the name is sanitised so it can never leave the tree —
/// `load`/`save`/`create` still refuse invalid names outright.
#[must_use]
pub fn dir(name: &str) -> PathBuf {
    let root = paths::workspaces_dir().unwrap_or_else(|_| std::env::temp_dir().join("parzi"));
    root.join(sanitize(name))
}

#[must_use]
pub fn workspace_file(name: &str) -> PathBuf {
    dir(name).join("workspace.toml")
}

/// Directory-name sanitiser for the infallible `dir`: keeps `[A-Za-z0-9_-]`,
/// folds everything else to `_`, caps at 64 chars. No separators, no dots —
/// the result can never escape `workspaces/`.
fn sanitize(name: &str) -> String {
    let mut out: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if out.is_empty() {
        out.push('_');
    }
    out
}

pub fn load(name: &str) -> Result<Workspace> {
    let clean = lanes::safe_name(name)?;
    let path = workspace_file(&clean);
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ParziError::Store(format!("workspace not found: {clean}"))
        } else {
            ParziError::Io(e)
        }
    })?;
    let mut ws: Workspace = toml::from_str(&raw)?;
    // The directory is the identity; a hand-edited `name` never wins.
    ws.name = clean;
    Ok(ws)
}

pub fn save(ws: &Workspace) -> Result<()> {
    let clean = lanes::safe_name(&ws.name)?;
    let text = toml::to_string_pretty(ws)?;
    crate::atomic_write(&workspace_file(&clean), text.as_bytes())
}

/// Every workspace directory that holds a `workspace.toml`. A missing tree is
/// zero workspaces, not an error.
#[must_use]
pub fn list() -> Vec<String> {
    let Ok(root) = paths::workspaces_dir() else {
        return vec![];
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return vec![];
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().join("workspace.toml").is_file())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

/// Create the tree (`projects/`) and write `workspace.toml`. Refuses to
/// overwrite an existing workspace: renaming is a separate, human act.
pub fn create(ws: Workspace) -> Result<Workspace> {
    let clean = lanes::safe_name(&ws.name)?;
    let dir = dir(&clean);
    if dir.join("workspace.toml").exists() {
        return Err(ParziError::Config(format!(
            "workspace `{clean}` already exists"
        )));
    }
    std::fs::create_dir_all(dir.join("projects"))?;
    let ws = Workspace { name: clean, ..ws };
    save(&ws)?;
    Ok(ws)
}

/// Step 1 of the wizard (§8): an empty workspace of `kind`, owned by whoever
/// asked for it. The shell supplies the machine's user and nothing else — that
/// the creator is the owner is a rule of the model, so it lives here.
pub fn create_for(name: &str, kind: Kind, user: &str) -> Result<Workspace> {
    let user = user.trim();
    if user.is_empty() {
        return Err(ParziError::Validation(
            "a workspace needs an owner".to_string(),
        ));
    }
    create(Workspace {
        name: name.trim().to_string(),
        kind,
        repos: Vec::new(),
        members: vec![Member {
            user: user.to_string(),
            role: Role::Owner,
        }],
    })
}

/// Each machine maps `remote → local path` for itself (§1.1): record where
/// this machine's checkout of `repo` is. A repo the workspace never named is
/// an error, not a silent no-op — the wizard would otherwise report a clone
/// that mapped nothing.
pub fn map_repo(name: &str, repo: &str, local_path: &std::path::Path) -> Result<Workspace> {
    let mut ws = load(name)?;
    let Some(slot) = ws.repos.iter_mut().find(|r| r.name == repo) else {
        return Err(ParziError::Store(format!(
            "workspace `{}` has no repo called `{repo}`",
            ws.name
        )));
    };
    slot.local_path = Some(local_path.to_path_buf());
    save(&ws)?;
    Ok(ws)
}

/// Add repos, keyed by name: an existing entry is replaced, so re-running the
/// wizard re-maps a local path instead of duplicating the repo.
pub fn add_repos(name: &str, repos: Vec<RepoRef>) -> Result<Workspace> {
    let mut ws = load(name)?;
    for repo in repos {
        match ws.repos.iter_mut().find(|r| r.name == repo.name) {
            Some(slot) => *slot = repo,
            None => ws.repos.push(repo),
        }
    }
    save(&ws)?;
    Ok(ws)
}

/// Migration (§15.1): an existing `~/.parzi/projects/<name>` (root + lanes)
/// read as a Solo workspace with one repo. Reads only — nothing is written,
/// nothing is deleted, and the legacy project stays where it is until a
/// person asks for the move.
pub fn from_legacy_project(name: &str) -> Result<Workspace> {
    let clean = lanes::safe_name(name)?;
    let legacy = paths::projects_dir()?.join(&clean);
    if !legacy.is_dir() {
        return Err(ParziError::Store(format!("no legacy project: {clean}")));
    }
    let root = lanes::lane_root(&clean, "").filter(|r| !r.trim().is_empty());
    let repos = root.map_or_else(Vec::new, |root| {
        let path = PathBuf::from(&root);
        let repo_name = path
            .file_name()
            .map_or_else(|| clean.clone(), |n| sanitize(&n.to_string_lossy()));
        vec![RepoRef {
            // The remote is unknown from disk; the wizard fills it in.
            name: repo_name,
            remote: String::new(),
            default_branch: default_branch(),
            local_path: Some(path),
        }]
    });
    Ok(Workspace {
        name: clean,
        kind: Kind::Solo,
        repos,
        members: vec![],
    })
}

/// The lanes of a legacy project as lane-template names (§15.1). Kept apart
/// from `from_legacy_project` so the wizard can show them without the
/// workspace grammar growing a field for them.
#[must_use]
pub fn legacy_lane_names(name: &str) -> Vec<String> {
    let Ok(clean) = lanes::safe_name(name) else {
        return vec![];
    };
    let Ok(scan) = lanes::scan_projects() else {
        return vec![];
    };
    let mut names: BTreeSet<String> = BTreeSet::new();
    for (project, lanes) in scan {
        if project.name == clean {
            names.extend(lanes.into_iter().map(|l| l.name));
        }
    }
    names.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::{add_repos, create_for, dir, load, map_repo, Kind, RepoRef, Role};
    use std::path::{Path, PathBuf};

    /// Unique per test, so nothing collides in a shared `PARZI_HOME`.
    fn scratch(tag: &str) -> String {
        format!("t-{tag}-{}", std::process::id())
    }

    fn cleanup(name: &str) {
        let _ = std::fs::remove_dir_all(dir(name));
    }

    #[test]
    fn the_creator_is_the_owner_and_an_owner_is_required() {
        let name = scratch("create-for");
        cleanup(&name);
        let ws = create_for(&name, Kind::Team, " ada ").expect("create");
        assert_eq!(ws.kind, Kind::Team);
        assert_eq!(ws.role_of("ada"), Some(Role::Owner));
        assert!(ws.repos.is_empty());
        // Twice is a refusal, not a silent overwrite.
        assert!(create_for(&name, Kind::Solo, "ada").is_err());
        assert!(create_for(&scratch("no-owner"), Kind::Solo, "  ").is_err());
        cleanup(&name);
    }

    #[test]
    fn mapping_records_this_machines_path_and_refuses_an_unknown_repo() {
        let name = scratch("map-repo");
        cleanup(&name);
        create_for(&name, Kind::Solo, "ada").expect("create");
        add_repos(
            &name,
            vec![RepoRef {
                name: "shop-api".into(),
                remote: "https://github.com/org/shop-api.git".into(),
                default_branch: "main".into(),
                local_path: None,
            }],
        )
        .expect("add repos");

        let ws = map_repo(&name, "shop-api", Path::new("D:/code/shop-api")).expect("map");
        let want = Some(PathBuf::from("D:/code/shop-api"));
        assert_eq!(
            ws.repo("shop-api").and_then(|r| r.local_path.clone()),
            want
        );
        // From disk, not just from the value we were handed.
        let reread = load(&name).expect("load");
        assert_eq!(
            reread.repo("shop-api").and_then(|r| r.local_path.clone()),
            want
        );
        // A repo the workspace never named is an error, not a no-op.
        assert!(map_repo(&name, "nope", Path::new("D:/code/nope")).is_err());
        cleanup(&name);
    }
}
