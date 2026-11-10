use serde::Serialize;

use crate::brain::Vault;
use crate::plan::{check_slug, read_note};

/// A goal from GOALS.md: an outcome the project works toward.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Goal {
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// A task from TASKS.md: a deliverable chunk of a goal.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoardTask {
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub track: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub goal: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub branch: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GoalsView {
    pub project: String,
    pub goals_path: String,
    pub tasks_path: String,
    #[serde(default)]
    pub goals: Vec<Goal>,
    #[serde(default)]
    pub tasks: Vec<BoardTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

const TITLE_CAP: usize = 120;
const FIELD_CAP: usize = 80;
const DETAIL_CAP: usize = 2_000;
const MAX_SECTIONS: usize = 200;

/// One `##` section: its title, its `key: value` fields, its body text.
pub type ParsedSection = (String, Vec<(String, String)>, String);

/// Loose-but-parseable: `##` sections, then optional `key: value`
/// lines (status, owner, track, goal, branch), then free body text.
/// Validators warn, never reject: anything unrecognized is body text.
#[must_use]
pub fn parse_sections(note: &str) -> Vec<ParsedSection> {
    let mut out: Vec<ParsedSection> = vec![];
    let mut title = String::new();
    let mut fields: Vec<(String, String)> = vec![];
    let mut body: Vec<String> = vec![];
    let mut in_section = false;
    let flush = |title: &mut String,
                 fields: &mut Vec<(String, String)>,
                 body: &mut Vec<String>,
                 out: &mut Vec<ParsedSection>| {
        if title.is_empty() && body.iter().all(|l| l.trim().is_empty()) {
            return;
        }
        let detail = body.join("\n").trim().to_string();
        out.push((
            std::mem::take(title),
            std::mem::take(fields),
            clip(&detail, DETAIL_CAP),
        ));
        body.clear();
    };
    for line in note.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix("## ").or_else(|| t.strip_prefix("##\t")) {
            if in_section {
                flush(&mut title, &mut fields, &mut body, &mut out);
                if out.len() >= MAX_SECTIONS {
                    return out;
                }
            }
            in_section = true;
            title = clip(h.trim(), TITLE_CAP);
            fields = vec![];
            body = vec![];
        } else if in_section {
            if let Some((k, v)) = field_of(t) {
                if fields.len() < 8 && !fields.iter().any(|(f, _)| f == &k) {
                    fields.push((k, clip(v.trim(), FIELD_CAP)));
                    continue;
                }
            }
            body.push(line.to_string());
        }
    }
    if in_section {
        flush(&mut title, &mut fields, &mut body, &mut out);
    }
    out
}

fn field_of(line: &str) -> Option<(String, String)> {
    let (k, v) = line.split_once(':')?;
    let k = k.trim().to_ascii_lowercase();
    if !matches!(k.as_str(), "status" | "owner" | "track" | "goal" | "branch") {
        return None;
    }
    let v = v.trim();
    if v.is_empty() || v.len() > FIELD_CAP {
        return None;
    }
    Some((k, v.to_string()))
}

fn clip(s: &str, cap: usize) -> String {
    if s.len() <= cap {
        return s.to_string();
    }
    let mut end = cap;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn field(fields: &[(String, String)], key: &str) -> String {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

#[must_use]
pub fn goals_path(slug: &str) -> String {
    format!("projects/{slug}/GOALS.md")
}

#[must_use]
pub fn tasks_path(slug: &str) -> String {
    format!("projects/{slug}/TASKS.md")
}

/// A GOALS.md / TASKS.md change belongs to this project board.
#[must_use]
pub fn board_of_note(path: &str) -> Option<String> {
    let p = path.replace('\\', "/");
    let rest = p.strip_prefix("projects/")?;
    let (slug, file) = rest.split_once('/')?;
    let is_board = file.eq_ignore_ascii_case("GOALS.md")
        || file.eq_ignore_ascii_case("TASKS.md")
        || file.eq_ignore_ascii_case("plan.md");
    (is_board && check_slug(slug).is_ok()).then(|| slug.to_string())
}

#[must_use]
pub fn load(vault: &Vault, slug: &str) -> GoalsView {
    let slug = slug.trim().to_string();
    let mut view = GoalsView {
        project: slug.clone(),
        goals_path: goals_path(&slug),
        tasks_path: tasks_path(&slug),
        goals: vec![],
        tasks: vec![],
        error: None,
    };
    if check_slug(&slug).is_err() {
        return view;
    }
    match read_note(vault, &view.goals_path.clone()) {
        Ok(None) => {}
        Ok(Some(text)) => {
            for (title, fields, detail) in parse_sections(&text) {
                view.goals.push(Goal {
                    title,
                    status: field(&fields, "status"),
                    owner: field(&fields, "owner"),
                    detail,
                });
            }
        }
        Err(e) => view.error = Some(e),
    }
    match read_note(vault, &view.tasks_path.clone()) {
        Ok(None) => {}
        Ok(Some(text)) => {
            for (title, fields, detail) in parse_sections(&text) {
                view.tasks.push(BoardTask {
                    title,
                    status: field(&fields, "status"),
                    owner: field(&fields, "owner"),
                    track: field(&fields, "track"),
                    goal: field(&fields, "goal"),
                    branch: field(&fields, "branch"),
                    detail,
                });
            }
        }
        Err(e) => {
            let msg = format!("TASKS.md: {e}");
            view.error = Some(match view.error.take() {
                Some(prev) => format!("{prev}; {msg}"),
                None => msg,
            });
        }
    }
    view
}

/// Move a task's `status:` line as its track reports back. Only the
/// first `## {title}` section changes; anything else stays byte for byte.
/// Returns whether a section matched.
///
/// # Errors
///
/// Fails when TASKS.md cannot be read or written.
pub fn set_task_status(
    vault: &Vault,
    slug: &str,
    title: &str,
    status: &str,
) -> Result<bool, String> {
    let path = tasks_path(slug.trim());
    let text = vault.read(&path).map_err(|e| e.to_string())?;
    let want = title.trim();
    if want.is_empty() {
        return Ok(false);
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    let mut i = 0;
    let mut found = false;
    while i < lines.len() {
        let t = lines[i].trim();
        let is_want = !found
            && t.strip_prefix("## ")
                .or_else(|| t.strip_prefix("##\t"))
                .map(str::trim)
                == Some(want);
        if !is_want {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        }
        found = true;
        out.push(lines[i].to_string());
        i += 1;
        while i < lines.len() && is_status_line(lines[i]) {
            i += 1;
        }
        out.push(format!("status: {status}"));
    }
    if !found {
        return Ok(false);
    }
    let mut next = out.join("\n");
    if text.ends_with('\n') {
        next.push('\n');
    }
    vault.write(&path, &next).map_err(|e| e.to_string())?;
    Ok(true)
}

fn is_status_line(line: &str) -> bool {
    let t = line.trim().to_ascii_lowercase();
    t.starts_with("status:") || t.starts_with("status :")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_split_and_fields_land() {
        let note = "# Board\n\n## Ship ParziOS\nstatus: active\nowner: eric\n\nThe big outcome.\n\n## Parked idea\n\nJust a thought.\n";
        let secs = parse_sections(note);
        assert_eq!(secs.len(), 2);
        assert_eq!(secs[0].0, "Ship ParziOS");
        assert_eq!(field(&secs[0].1, "status"), "active");
        assert_eq!(field(&secs[0].1, "owner"), "eric");
        assert!(secs[0].2.contains("The big outcome."));
        assert_eq!(secs[1].0, "Parked idea");
        assert!(field(&secs[1].1, "status").is_empty());
    }

    #[test]
    fn unknown_keys_stay_body_text() {
        let note = "## T\nvelocity: 5\n\nReal body.";
        let secs = parse_sections(note);
        assert_eq!(secs.len(), 1);
        assert!(field(&secs[0].1, "status").is_empty());
        assert!(secs[0].2.contains("velocity: 5"));
    }

    #[test]
    fn board_notes_map_to_their_project() {
        assert_eq!(
            board_of_note("projects/parzi/GOALS.md").as_deref(),
            Some("parzi")
        );
        assert_eq!(
            board_of_note("projects/parzi/TASKS.md").as_deref(),
            Some("parzi")
        );
        assert_eq!(
            board_of_note("projects/parzi/plan.md").as_deref(),
            Some("parzi")
        );
        assert!(board_of_note("projects/parzi/other.md").is_none());
    }

    #[test]
    fn status_moves_and_nothing_else_does() {
        let dir = tempfile::tempdir().expect("temp vault");
        let vault = Vault::at(dir.path());
        vault
            .write(
                "projects/demo/TASKS.md",
                "# TASKS\n\n## Sync daemon\nstatus: planned\ntrack: daemon\n\nKeep it fresh.\n\n## Top bar\n\nNo status yet.\n",
            )
            .expect("seed tasks");
        assert!(set_task_status(&vault, "demo", "Sync daemon", "active").unwrap());
        assert!(set_task_status(&vault, "demo", "Top bar", "done").unwrap());
        assert!(!set_task_status(&vault, "demo", "No such task", "done").unwrap());
        let text = vault.read("projects/demo/TASKS.md").expect("read back");
        assert!(
            text.contains("## Sync daemon\nstatus: active\ntrack: daemon\n"),
            "{text}"
        );
        assert!(text.contains("## Top bar\nstatus: done\n"), "{text}");
        assert!(text.contains("Keep it fresh."), "{text}");
    }
}
