//! A project's plan: a short goal and an ordered list of targets, kept as a
//! markdown note in the brain at `projects/<slug>/plan.md`. Each target names
//! the sessions and drafts working on it, so the plan follows the real work
//! instead of drifting away from it.

use crate::error::Result;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectPlan {
    pub goal: String,
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    pub title: String,
    pub done: bool,
    /// Sessions and drafts working on this target.
    pub sessions: Vec<String>,
}

/// The note a project's plan lives in, relative to the brain.
#[must_use]
pub fn note_path(slug: &str) -> String {
    format!("projects/{slug}/plan.md")
}

impl ProjectPlan {
    /// Reads a plan note. Lines it does not understand are left out; a note
    /// with neither goal nor targets reads as an empty plan.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let body = strip_front_matter(text);
        let mut plan = Self::default();
        let mut section = "";
        let mut goal: Vec<&str> = vec![];
        for line in body.lines() {
            if let Some(h) = line.strip_prefix("# ") {
                section = if h.trim().eq_ignore_ascii_case("goal") {
                    "goal"
                } else if h.trim().eq_ignore_ascii_case("targets") {
                    "targets"
                } else {
                    ""
                };
                continue;
            }
            match section {
                "goal" => goal.push(line),
                "targets" => {
                    if let Some(t) = parse_target(line) {
                        plan.targets.push(t);
                    }
                }
                _ => {}
            }
        }
        plan.goal = goal.join("\n").trim().to_string();
        plan
    }

    /// The note's text, front matter included, so the brain files it under
    /// the project.
    #[must_use]
    pub fn render(&self, slug: &str) -> String {
        let mut out = format!(
            "---\ntitle: Project plan\nprojects: [{slug}]\ndescription: The goal and ordered targets for {slug}\n---\n# Goal\n\n{}\n\n# Targets\n\n",
            self.goal.trim()
        );
        for t in &self.targets {
            let mark = if t.done { "x" } else { " " };
            let title = t.title.replace('\n', " ");
            if t.sessions.is_empty() {
                out.push_str(&format!("- [{mark}] {}\n", title.trim()));
            } else {
                out.push_str(&format!(
                    "- [{mark}] {} <!-- sessions: {} -->\n",
                    title.trim(),
                    t.sessions.join(",")
                ));
            }
        }
        out
    }

    /// Replaces the goal and the ordered targets with what an agent wrote,
    /// keeping each target's linked sessions by its title.
    pub fn rewrite(&mut self, goal: Option<&str>, targets: Option<Vec<(String, bool)>>) {
        if let Some(g) = goal {
            self.goal = g.trim().to_string();
        }
        if let Some(list) = targets {
            let old = std::mem::take(&mut self.targets);
            self.targets = list
                .into_iter()
                .filter(|(title, _)| !title.trim().is_empty())
                .map(|(title, done)| {
                    let sessions = old
                        .iter()
                        .find(|o| same_title(&o.title, &title))
                        .map(|o| o.sessions.clone())
                        .unwrap_or_default();
                    Target {
                        title: title.trim().to_string(),
                        done,
                        sessions,
                    }
                })
                .collect();
        }
    }

    /// Ties a session or draft to the target with this title. False when no
    /// target has that title.
    pub fn link(&mut self, title: &str, session: &str) -> bool {
        let Some(t) = self
            .targets
            .iter_mut()
            .find(|t| same_title(&t.title, title))
        else {
            return false;
        };
        if !t.sessions.iter().any(|s| s == session) {
            t.sessions.push(session.to_string());
        }
        true
    }
}

fn same_title(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn strip_front_matter(text: &str) -> &str {
    let t = text.trim_start_matches('\u{feff}');
    if let Some(rest) = t.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            return rest[end + 4..].trim_start_matches(['\r', '\n']);
        }
    }
    t
}

fn parse_target(line: &str) -> Option<Target> {
    let l = line.trim_start();
    let rest = l.strip_prefix("- [").or_else(|| l.strip_prefix("* ["))?;
    let (mark, rest) = rest.split_once(']')?;
    let done = mark.trim().eq_ignore_ascii_case("x");
    let (title, sessions) = match rest.split_once("<!-- sessions:") {
        Some((title, tail)) => {
            let ids = tail.trim_end().trim_end_matches("-->");
            let ids = ids
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            (title, ids)
        }
        None => (rest, vec![]),
    };
    let title = title.trim().to_string();
    (!title.is_empty()).then_some(Target {
        title,
        done,
        sessions,
    })
}

/// The project's plan, or `None` when it has none yet.
pub fn load(slug: &str) -> Option<ProjectPlan> {
    crate::brain::read(&note_path(slug))
        .ok()
        .map(|t| ProjectPlan::parse(&t))
}

pub fn save(slug: &str, plan: &ProjectPlan) -> Result<()> {
    crate::brain::write(&note_path(slug), &plan.render(slug))?;
    Ok(())
}

/// One change at a time: two sessions writing the plan, or two helpers
/// linking themselves, would otherwise keep only the last write.
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Loads the plan (an empty one when there is none), lets `change` edit it,
/// and saves it when `change` says it changed something.
pub fn update<R>(slug: &str, change: impl FnOnce(&mut ProjectPlan) -> (bool, R)) -> Result<R> {
    let _one = WRITING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut plan = load(slug).unwrap_or_default();
    let (changed, out) = change(&mut plan);
    if changed {
        save(slug, &plan)?;
    }
    Ok(out)
}

/// Ties a session or draft to the target with this title in the project's
/// plan. Says what happened, for an agent to read.
#[must_use]
pub fn link_session(slug: &str, title: &str, session: &str) -> String {
    if load(slug).is_none() {
        return format!("no project plan in {slug} to link the target to");
    }
    match update(slug, |plan| {
        let linked = plan.link(title, session);
        (linked, linked)
    }) {
        Ok(true) => format!("linked to the target \"{title}\""),
        Ok(false) => format!("no target \"{title}\" in the project plan"),
        Err(e) => format!("could not link the target: {e}"),
    }
}

/// Where the plan note sits on disk, for opening it.
pub fn doc_path(slug: &str) -> Option<std::path::PathBuf> {
    let path = crate::paths::brain_dir().ok()?.join(note_path(slug));
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_reads_back_what_it_wrote() {
        let plan = ProjectPlan {
            goal: "Ship the cut-down Parzi.".into(),
            targets: vec![
                Target {
                    title: "Pins above the composer".into(),
                    done: false,
                    sessions: vec!["a1".into(), "b2".into()],
                },
                Target {
                    title: "One send state".into(),
                    done: true,
                    sessions: vec![],
                },
            ],
        };
        let text = plan.render("parzi");
        assert!(text.contains("projects: [parzi]"));
        assert_eq!(ProjectPlan::parse(&text), plan);
    }

    #[test]
    fn a_rewrite_keeps_links_by_title_and_follows_the_new_order() {
        let mut plan = ProjectPlan::parse(
            "# Goal\nold goal\n\n# Targets\n- [ ] First <!-- sessions: s1 -->\n- [ ] Second\n",
        );
        plan.rewrite(
            Some("new goal"),
            Some(vec![
                ("second".into(), false),
                ("First".into(), true),
                ("Third".into(), false),
            ]),
        );
        assert_eq!(plan.goal, "new goal");
        let titles: Vec<&str> = plan.targets.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["second", "First", "Third"]);
        assert_eq!(
            plan.targets[1].sessions,
            ["s1"],
            "the link follows its target"
        );
        assert!(plan.targets[1].done);
        assert!(plan.link("third", "d9"));
        assert!(plan.link("third", "d9"), "linking twice is fine");
        assert_eq!(plan.targets[2].sessions, ["d9"]);
        assert!(!plan.link("missing", "x"));
    }

    #[test]
    fn hand_written_notes_read_too() {
        let plan = ProjectPlan::parse(
            "---\ntitle: x\n---\nintro\n# Goal\n\nA goal\nover two lines\n# Targets\n* [X] done one\n- [ ] open one\nnot a target\n",
        );
        assert_eq!(plan.goal, "A goal\nover two lines");
        assert_eq!(plan.targets.len(), 2);
        assert!(plan.targets[0].done && !plan.targets[1].done);
    }
}
