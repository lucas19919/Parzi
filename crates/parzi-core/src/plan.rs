//! Project plans (`projects/<slug>/plan.md`) are gone: a session owns its
//! plan, and work not started yet is a draft session. This reads an old
//! plan once, turns its unfinished items into drafts, and archives the note.

use serde_json::Value;

use crate::brain::Vault;
use crate::store::{SessionStatus, SessionStore};
use crate::text::{clip, fold};

const FENCE: &str = "parzi-plan";

/// An item of an old project plan, as far as the move to drafts needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldItem {
    pub title: String,
    pub detail: String,
    pub needs: Vec<String>,
    pub open: bool,
}

fn block(note: &str) -> Option<String> {
    let mut lines = note.lines();
    lines.find(|l| {
        l.trim()
            .strip_prefix("```")
            .is_some_and(|rest| rest.trim() == FENCE)
    })?;
    let mut out = String::new();
    for l in lines {
        if l.trim() == "```" {
            return Some(out);
        }
        out.push_str(l);
        out.push('\n');
    }
    Some(out)
}

/// The items of an old `plan.md`, or why it cannot be read.
pub fn parse(note: &str) -> Result<Vec<OldItem>, String> {
    let raw = block(note).ok_or("the note has no ```parzi-plan block")?;
    let v: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("the parzi-plan block is not valid JSON: {e}"))?;
    let text = |it: &Value, k: &str| {
        fold(it.get(k).and_then(Value::as_str).unwrap_or(""))
            .trim()
            .to_string()
    };
    Ok(v.get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|it| {
            let id = text(it, "id");
            let title = clip(
                &Some(text(it, "title"))
                    .filter(|t| !t.is_empty())
                    .unwrap_or(id),
                80,
            );
            if title.is_empty() {
                return None;
            }
            let state = text(it, "state").to_ascii_lowercase();
            Some(OldItem {
                title,
                detail: clip(&text(it, "detail"), 1_000),
                needs: it
                    .get("needs")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
                open: !matches!(
                    state.as_str(),
                    "done" | "finished" | "dropped" | "cancelled" | "canceled"
                ),
            })
        })
        .collect())
}

/// Moves every project's old `plan.md` to drafts: one draft per unfinished
/// item (its detail as the brief), then renames the note
/// `plan.archived.md` so nothing is lost and it is never read again.
/// Returns how many drafts were made.
pub fn to_drafts(vault: &Vault, store: &SessionStore) -> usize {
    let mut made = 0;
    for p in vault.projects() {
        let path = format!("projects/{}/plan.md", p.slug);
        let Ok(full) = vault.resolve(&path) else {
            continue;
        };
        if !full.is_file() {
            continue;
        }
        let Ok(note) = std::fs::read_to_string(&full) else {
            continue;
        };
        let items = match parse(&note) {
            Ok(items) => items,
            Err(e) => {
                tracing::warn!(project = %p.slug, "old plan not moved to drafts: {e}");
                vec![]
            }
        };
        // A draft per open item that no session already carries under that title.
        let known: Vec<String> = store
            .list()
            .unwrap_or_default()
            .into_iter()
            .filter(|m| m.project == p.slug && m.status == SessionStatus::Draft)
            .map(|m| m.title)
            .collect();
        for it in items.iter().filter(|i| i.open && !known.contains(&i.title)) {
            let mut brief = if it.detail.is_empty() {
                it.title.clone()
            } else {
                it.detail.clone()
            };
            if !it.needs.is_empty() {
                brief.push_str(&format!(
                    "\n\nFrom the old project plan; it came after: {}.",
                    it.needs.join(", ")
                ));
            }
            if store
                .create_draft(&it.title, &p.slug, "", &p.folder, &brief, None)
                .is_ok()
            {
                made += 1;
            }
        }
        let archived = full.with_file_name("plan.archived.md");
        if let Err(e) = std::fs::rename(&full, &archived) {
            tracing::warn!(project = %p.slug, "old plan not archived: {e}");
        }
    }
    made
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_open_and_settled_items() {
        let note = "# Plan\n\n```parzi-plan\n{\"items\": [\n {\"id\": \"a\", \"title\": \"Lexer\", \"state\": \"done\"},\n {\"id\": \"b\", \"title\": \"Parser\", \"detail\": \"Write it.\", \"needs\": [\"a\"]},\n {\"id\": \"c\", \"state\": \"dropped\"}\n]}\n```\n";
        let items = parse(note).unwrap();
        assert_eq!(items.len(), 3);
        assert!(!items[0].open);
        assert!(items[1].open && items[1].detail == "Write it." && items[1].needs == ["a"]);
        assert_eq!(
            items[2].title, "c",
            "an item without a title is named by its id"
        );
        assert!(!items[2].open);
    }

    #[test]
    fn a_note_without_a_plan_block_is_an_error() {
        assert!(parse("just text").is_err());
    }
}
