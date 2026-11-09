//! Brain note tools run in-process: listings of a project's notes and the
//! plain note reads and writes.

use std::collections::HashMap;

use parzi_core::brain;
use parzi_core::error::{ParziError, Result};
use serde_json::Value;

pub(super) fn execute_brain(name: &str, args: &Value) -> (bool, String) {
    let arg = |k: &str| args.get(k).and_then(Value::as_str).map_or("", str::trim);
    let note = |p: &str| {
        if p.to_ascii_lowercase().ends_with(".md") {
            p.to_string()
        } else {
            format!("{p}.md")
        }
    };
    let vault = match brain::Vault::open() {
        Ok(v) => v,
        Err(e) => return (false, e.to_string()),
    };
    let out = match name {
        "brain.search" if arg("query").is_empty() => brain_listing(&vault, arg("project")),
        "brain.search" => {
            let hits = vault.search(arg("query"));
            Ok(if hits.is_empty() {
                format!("no notes match `{}`", arg("query"))
            } else {
                hits.iter()
                    .map(|h| format!("- {} — {}\n  {}", h.path, h.title, h.snippet))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        "brain.read" if arg("path").is_empty() => {
            return (false, "brain.read needs a `path`".into())
        }
        "brain.read" => vault.read(&note(arg("path"))),
        "brain.delete" if arg("path").is_empty() => {
            return (false, "brain.delete needs a `path`".into())
        }
        "brain.delete" => {
            let path = note(arg("path"));
            let _plan = parzi_core::plan::lock_if_plan(&path);
            brain::delete(&path).map(|()| format!("deleted {path}"))
        }
        "brain.write" => match args.get("content").and_then(Value::as_str) {
            Some(content) if !arg("path").is_empty() => {
                let path = note(arg("path"));
                let _plan = parzi_core::plan::lock_if_plan(&path);
                vault
                    .write(&path, content)
                    .map(|m| format!("saved {} ({} bytes)", m.path, m.bytes))
            }
            _ => return (false, "brain.write needs `path` + `content`".into()),
        },
        _ => return (false, format!("unknown brain tool `{name}`")),
    };
    match out {
        Ok(text) => (true, text),
        Err(e) => (false, e.to_string()),
    }
}

fn note_line(n: &brain::NoteMeta, projects: bool) -> String {
    let mut line = format!("\n- {} — {}", n.path, n.title);
    if projects && !n.projects.is_empty() {
        line.push_str(&format!(" [{}]", n.projects.join(", ")));
    }
    if let Some(original) = &n.conflict_of {
        line.push_str(&format!(
            " (sync conflict copy of {original}; the user settles it in the Brain view)"
        ));
        return line;
    }
    if n.pinned {
        line.push_str(" (pinned)");
    }
    if !n.summary.is_empty() {
        line.push_str(": ");
        line.push_str(&n.summary);
    }
    line
}

const SHOWN: usize = 50;

fn note_lines<'a>(
    out: &mut String,
    notes: impl Iterator<Item = &'a brain::NoteMeta>,
    projects: bool,
) {
    let mut more = 0;
    for (i, n) in notes.enumerate() {
        if i < SHOWN {
            out.push_str(&note_line(n, projects));
        } else {
            more += 1;
        }
    }
    if more > 0 {
        out.push_str(&format!("\n- …and {more} more; use brain.search"));
    }
}

fn brain_listing(vault: &brain::Vault, project: &str) -> Result<String> {
    let (notes, projects) = vault.catalog();
    if project.eq_ignore_ascii_case(brain::EVERYWHERE) {
        let mut out = String::from("Notes for every session:");
        let before = out.len();
        note_lines(&mut out, notes.iter().filter(|n| n.everywhere()), false);
        if out.len() == before {
            out.push_str(" none");
        }
        return Ok(out);
    }
    if !project.is_empty() {
        let by_path: HashMap<&str, &brain::NoteMeta> =
            notes.iter().map(|n| (n.path.as_str(), n)).collect();
        let p = projects
            .iter()
            .find(|p| p.slug.eq_ignore_ascii_case(project))
            .ok_or_else(|| {
                ParziError::Validation(format!(
                    "no project `{project}` in the brain; brain.search with no query and no project lists them"
                ))
            })?;
        let mut out = format!(
            "{} ({}), folder {}\n- {} — project note",
            p.title, p.slug, p.folder, p.note
        );
        let mapped = p
            .notes
            .iter()
            .filter_map(|n| by_path.get(n.as_str()).copied());
        note_lines(&mut out, mapped, false);
        return Ok(out);
    }
    if notes.is_empty() {
        return Ok(format!("the brain is empty ({})", vault.root().display()));
    }
    let mut out = String::from("Projects:");
    if projects.is_empty() {
        out.push_str(" none");
    }
    for p in &projects {
        out.push_str(&format!(
            "\n- {} ({}), folder {}, {} notes",
            p.slug,
            p.title,
            p.folder,
            p.notes.len()
        ));
    }
    out.push_str("\n\nNotes:");
    note_lines(&mut out, notes.iter(), true);
    Ok(out)
}
