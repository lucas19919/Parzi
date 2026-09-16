//! `PROJECT.md` — the Parzi grammar of a project (PLAN §2), plus the
//! `critical:` matcher used when a lease transfer needs a human (§4).
//!
//! Small enough to read in a minute, parsed line by line like the living
//! plan: no YAML engine, tolerant of ordering, blank lines and trailing
//! `#` comments, strict on an unknown grammar major (§15.10). The file here
//! holds the types, the paths and the renderer; `grammar` reads the text
//! back and `critical` is the glob matcher.

mod critical;
mod grammar;

pub use critical::{glob_match, is_critical, normalize_path};
pub use grammar::parse;
pub(crate) use grammar::strip_version;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::{lanes, workspace};

/// First line of PROJECT.md and PLAN.md: `parzi: 1`.
pub const GRAMMAR_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Drafting,
    Planned,
    Running,
    Done,
    Parked,
}

impl Status {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Drafting => "drafting",
            Status::Planned => "planned",
            Status::Running => "running",
            Status::Done => "done",
            Status::Parked => "parked",
        }
    }

    fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "drafting" => Ok(Status::Drafting),
            "planned" => Ok(Status::Planned),
            "running" => Ok(Status::Running),
            "done" => Ok(Status::Done),
            "parked" => Ok(Status::Parked),
            other => Err(ParziError::Validation(format!(
                "unknown project status `{other}`"
            ))),
        }
    }
}

/// One `provider/model` per role (§1.2). Free-form strings: the model
/// registry lives in parzi-providers, not in the grammar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roster {
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub orchestrator: String,
    #[serde(default)]
    pub coder: String,
}

/// One acceptance criterion under `## What`, testable, checkable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Criterion {
    pub text: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub slug: String,
    pub title: String,
    pub workspace: String,
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub roster: Roster,
    #[serde(default)]
    pub budget_usd: Option<f64>,
    #[serde(default)]
    pub status: Status,
    /// Globs whose lease transfer needs a human (§4), repo-prefixed.
    #[serde(default)]
    pub critical: Vec<String>,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub what: Vec<Criterion>,
    #[serde(default)]
    pub constraints: Vec<String>,
}

/// `<workspace dir>/projects/<slug>`.
#[must_use]
pub fn dir(workspace: &str, slug: &str) -> PathBuf {
    workspace::dir(workspace)
        .join("projects")
        .join(slug_of(slug))
}

#[must_use]
pub fn project_file(workspace: &str, slug: &str) -> PathBuf {
    dir(workspace, slug).join("PROJECT.md")
}

pub fn load(workspace: &str, slug: &str) -> Result<Project> {
    let clean = lanes::safe_name(slug)?;
    let path = project_file(workspace, &clean);
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ParziError::Store(format!("project not found: {clean}"))
        } else {
            ParziError::Io(e)
        }
    })?;
    let mut project = parse(&raw)?;
    // The directory is the identity, as for workspaces.
    project.slug = clean;
    Ok(project)
}

/// Step 2 of the flow (§8): a project in `Drafting`, ready for the header
/// agent. The wizard supplies what a person chose — title, repos, roster,
/// budget; everything else (why, what, constraints) the header drafts with
/// them, so it starts empty. An existing slug is never overwritten.
pub fn create(
    workspace: &str,
    title: &str,
    repos: Vec<String>,
    roster: Roster,
    budget_usd: Option<f64>,
) -> Result<Project> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(ParziError::Validation(
            "give the project a title".to_string(),
        ));
    }
    let workspace = lanes::safe_name(workspace)?;
    let slug = slug_of(&title);
    if project_file(&workspace, &slug).is_file() {
        return Err(ParziError::Config(format!(
            "{workspace} already has a project called {slug}"
        )));
    }
    let project = Project {
        slug,
        title,
        workspace,
        repos,
        roster,
        budget_usd,
        status: Status::Drafting,
        ..Project::default()
    };
    save(&project)?;
    Ok(project)
}

pub fn save(project: &Project) -> Result<()> {
    let clean = lanes::safe_name(&project.slug)?;
    lanes::safe_name(&project.workspace)?;
    let text = render(project);
    crate::atomic_write(&project_file(&project.workspace, &clean), text.as_bytes())
}

/// Every project slug in a workspace, sorted. Missing tree = none.
#[must_use]
pub fn list(workspace: &str) -> Vec<String> {
    let root = workspace::dir(workspace).join("projects");
    let Ok(entries) = std::fs::read_dir(root) else {
        return vec![];
    };
    let mut slugs: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().join("PROJECT.md").is_file())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    slugs.sort();
    slugs
}

/// Title → slug: lowercase, non-alphanumerics folded to `-`, no run of
/// dashes, capped at 48 chars. Never empty.
#[must_use]
pub fn slug_of(title: &str) -> String {
    let mut out = String::new();
    for c in title.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let slug: String = out.trim_matches('-').chars().take(48).collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    }
}

/// `40 USD` / `40.5 USD`: the grammar's own spelling, so it parses back.
fn fmt_budget(n: f64) -> String {
    if (n.fract()).abs() < f64::EPSILON {
        format!("{n:.0} USD")
    } else {
        format!("{n} USD")
    }
}

/// Render PROJECT.md. `parse(render(p)) == p` for every project.
#[must_use]
pub fn render(p: &Project) -> String {
    let mut out = format!("parzi: {GRAMMAR_VERSION}\n# Project: {}\n", p.title);
    out.push_str(&format!("workspace: {}\n", p.workspace));
    out.push_str(&format!("slug: {}\n", p.slug));
    out.push_str(&format!("repos: {}\n", p.repos.join(", ")));
    out.push_str(&format!(
        "roster: header = {}, orchestrator = {}, coder = {}\n",
        p.roster.header, p.roster.orchestrator, p.roster.coder
    ));
    if let Some(b) = p.budget_usd {
        out.push_str(&format!("budget: {}\n", fmt_budget(b)));
    }
    out.push_str(&format!("status: {}\n", p.status.as_str()));
    if !p.critical.is_empty() {
        out.push_str(&format!("critical: {}\n", p.critical.join(", ")));
    }
    out.push_str("\n## Why\n\n");
    if !p.why.is_empty() {
        out.push_str(p.why.trim());
        out.push('\n');
    }
    out.push_str("\n## What\n\n");
    for c in &p.what {
        out.push_str(&format!(
            "- [{}] {}\n",
            if c.done { "x" } else { " " },
            c.text
        ));
    }
    out.push_str("\n## Constraints\n\n");
    for c in &p.constraints {
        out.push_str(&format!("- {c}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{create, load, Roster, Status};

    #[test]
    fn a_new_project_is_drafting_and_a_slug_is_taken_only_once() {
        let ws = format!("t-project-create-{}", std::process::id());
        let scrub = || {
            let _ = std::fs::remove_dir_all(crate::workspace::dir(&ws));
        };
        scrub();
        let p = create(
            &ws,
            "  Checkout flow  ",
            vec!["shop-api".to_string()],
            Roster::default(),
            Some(40.0),
        )
        .expect("create");
        assert_eq!(p.slug, "checkout-flow");
        assert_eq!(p.title, "Checkout flow");
        assert_eq!(p.status, Status::Drafting);
        assert!(p.why.is_empty() && p.what.is_empty() && p.constraints.is_empty());
        assert_eq!(
            load(&ws, "checkout-flow").expect("load").budget_usd,
            Some(40.0)
        );
        // Same slug from a different title is still the same project.
        assert!(create(&ws, "Checkout Flow!", vec![], Roster::default(), None).is_err());
        assert!(create(&ws, "   ", vec![], Roster::default(), None).is_err());
        scrub();
    }
}
