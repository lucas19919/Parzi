//! Reading `PROJECT.md` back (PLAN §2): tolerant of ordering, blank lines
//! and trailing `#` comments, strict on an unknown grammar major (§15.10).

use super::{slug_of, Criterion, Project, Roster, Status, GRAMMAR_VERSION};
use crate::error::{ParziError, Result};

/// Refuse an unknown grammar major, never silently (§15.10). Returns the
/// body with the `parzi:` line removed; a file without one is read as v1 so
/// hand-written files from before the version line still load.
pub(crate) fn strip_version(text: &str) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut checked = false;
    for line in text.lines() {
        let t = line.trim();
        if !checked && !t.is_empty() {
            checked = true;
            if let Some(rest) = t.strip_prefix("parzi:") {
                let major: u32 = rest.trim().parse().map_err(|_| {
                    ParziError::Validation(format!("bad grammar version `{}`", rest.trim()))
                })?;
                if major != GRAMMAR_VERSION {
                    return Err(ParziError::Validation(format!(
                        "grammar version {major} unsupported (this Parzi reads {GRAMMAR_VERSION}) — update Parzi"
                    )));
                }
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// Strip a trailing ` # comment` from a key line (the §2 sample has them).
fn strip_comment(value: &str) -> &str {
    let bytes = value.as_bytes();
    for (i, c) in value.char_indices() {
        if c == '#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            return value[..i].trim_end();
        }
    }
    value.trim_end()
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn parse(text: &str) -> Result<Project> {
    let body = strip_version(text)?;
    let mut p = Project::default();
    let mut why: Vec<String> = vec![];
    let mut section = Section::None;
    for line in body.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("## ") {
            section = Section::of(rest);
            continue;
        }
        if let Some(rest) = t.strip_prefix("# ") {
            p.title = rest
                .trim()
                .strip_prefix("Project:")
                .unwrap_or(rest.trim())
                .trim()
                .to_string();
            continue;
        }
        match section {
            Section::None => parse_key(t, &mut p)?,
            Section::Why => why.push(line.trim_end().to_string()),
            Section::What => {
                if let Some(c) = parse_criterion(t) {
                    p.what.push(c);
                }
            }
            Section::Constraints => {
                if let Some(rest) = t.strip_prefix("- ") {
                    p.constraints.push(rest.trim().to_string());
                }
            }
            Section::Other => {}
        }
    }
    p.why = why.join("\n").trim().to_string();
    if p.slug.is_empty() {
        p.slug = slug_of(&p.title);
    }
    if p.title.is_empty() {
        return Err(ParziError::Validation(
            "PROJECT.md has no `# Project: <title>` line".into(),
        ));
    }
    Ok(p)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Why,
    What,
    Constraints,
    Other,
}

impl Section {
    fn of(header: &str) -> Self {
        match header.trim().to_ascii_lowercase().as_str() {
            "why" => Section::Why,
            "what" => Section::What,
            "constraints" => Section::Constraints,
            _ => Section::Other,
        }
    }
}

fn parse_criterion(t: &str) -> Option<Criterion> {
    let (done, rest) = if let Some(r) = t.strip_prefix("- [ ]") {
        (false, r)
    } else if let Some(r) = t.strip_prefix("- [x]").or_else(|| t.strip_prefix("- [X]")) {
        (true, r)
    } else if let Some(r) = t.strip_prefix("- ") {
        (false, r)
    } else {
        return None;
    };
    let text = rest.trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(Criterion { text, done })
}

fn parse_key(t: &str, p: &mut Project) -> Result<()> {
    let Some((key, value)) = t.split_once(':') else {
        return Ok(());
    };
    let value = strip_comment(value).trim();
    match key.trim().to_ascii_lowercase().as_str() {
        "workspace" => p.workspace = value.to_string(),
        "slug" => p.slug = value.to_string(),
        "repos" => p.repos = split_list(value),
        "roster" => parse_roster(value, &mut p.roster),
        "budget" => p.budget_usd = parse_budget(value),
        "status" => p.status = Status::parse(value)?,
        "critical" => p.critical.extend(split_list(value)),
        _ => {}
    }
    Ok(())
}

fn parse_roster(value: &str, roster: &mut Roster) {
    for part in value.split(',') {
        let Some((role, model)) = part.split_once('=') else {
            continue;
        };
        let model = model.trim().to_string();
        match role.trim().to_ascii_lowercase().as_str() {
            "header" => roster.header = model,
            "orchestrator" => roster.orchestrator = model,
            "coder" | "implementation" => roster.coder = model,
            _ => {}
        }
    }
}

/// `40 USD`, `40`, `$40.50` — the first number wins, the currency is USD.
fn parse_budget(value: &str) -> Option<f64> {
    let cleaned: String = value
        .chars()
        .map(|c| if c == '$' { ' ' } else { c })
        .collect();
    cleaned
        .split_whitespace()
        .find_map(|tok| tok.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n >= 0.0)
}
