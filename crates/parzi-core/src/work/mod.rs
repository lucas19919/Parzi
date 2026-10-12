//! Parzi Work: goals the agents fill in and the human watches. Each engine
//! keeps one append-only ledger (`log`), folded into goals, runs and asks
//! (`fold`); the desk and the CLI read views of it (`view`).

pub mod fold;
pub mod graph;
mod import;
pub mod log;
pub mod model;
pub mod view;

pub use fold::{Ask, Binding, Goal, Run, Step, Writer};
pub use import::import_legacy_tasks;
pub use log::{now_ms, Change, Settled, WorkLog};
pub use model::*;

/// The project a session's goals file under: "default" and "" are none.
#[must_use]
pub fn project_of_session(project: &str) -> Option<String> {
    let p = project.trim();
    (!p.is_empty() && p != "default").then(|| p.to_string())
}

/// A run's last message, made fit for a card: markdown marks and code
/// blocks go, whitespace folds, and it stops at `SUMMARY_CAP` chars.
#[must_use]
pub fn summarize(text: &str) -> String {
    let mut out: Vec<String> = vec![];
    let mut fenced = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced
            || t.is_empty()
            || t.chars()
                .all(|c| matches!(c, '-' | '*' | '_' | '|' | ':' | ' '))
        {
            continue;
        }
        let t = t.trim_start_matches('#').trim_start();
        let t = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .or_else(|| t.strip_prefix("> "))
            .unwrap_or(t);
        let t: String = t.chars().filter(|c| !matches!(c, '*' | '`')).collect();
        out.push(t.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    clip(&out.join(" "), SUMMARY_CAP)
}

/// Openers people start a request with that say nothing about the work.
const OPENERS: &[&str] = &[
    "hey there",
    "hey",
    "hi there",
    "hi",
    "hello",
    "ok so",
    "okay so",
    "ok",
    "okay",
    "so",
    "please",
    "pls",
    "can you please",
    "could you please",
    "would you please",
    "can you",
    "could you",
    "would you",
    "will you",
    "i want you to",
    "i'd like you to",
    "i would like you to",
    "i need you to",
    "help me",
    "let's",
    "lets",
];

/// A goal's title from what the person asked: the first line, openers
/// stripped, capitalized, at most 80 chars on a word boundary.
#[must_use]
pub fn title_from_prompt(prompt: &str) -> String {
    let first = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let opener = |t: &str| {
        OPENERS.iter().find_map(|o| {
            let head = t.get(..o.len())?;
            let next = t[o.len()..].chars().next();
            // Whole words only: "so" is not the start of "solve".
            (head.eq_ignore_ascii_case(o) && next.is_none_or(|c| !c.is_alphanumeric()))
                .then_some(o.len())
        })
    };
    let mut t = first.to_string();
    while let Some(cut) = opener(&t) {
        t = t[cut..]
            .trim_start_matches(|c: char| c == ',' || c == '!' || c == ':' || c.is_whitespace())
            .to_string();
    }
    let t = t.trim().trim_end_matches(['?', '.', '!', ',']).trim();
    let mut out = String::new();
    for word in t.split_whitespace() {
        if out.chars().count() + word.chars().count() + 1 > 80 {
            if out.is_empty() {
                out = clip(word, 79);
            }
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    let mut chars = out.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The next free key for a step made from a spawn: a, b, … z, aa, ab, …
#[must_use]
pub fn auto_step_key(taken: &[String]) -> String {
    let name = |mut n: usize| {
        let mut s = String::new();
        loop {
            s.insert(0, char::from(b'a' + u8::try_from(n % 26).unwrap_or(0)));
            if n < 26 {
                break;
            }
            n = n / 26 - 1;
        }
        s
    };
    (0..)
        .map(name)
        .find(|k| !taken.iter().any(|t| t == k))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_lose_the_pleasantries() {
        assert_eq!(
            title_from_prompt("hey, can you please fix the omnibar autosize?\nmore"),
            "Fix the omnibar autosize"
        );
        assert_eq!(title_from_prompt("solve x^2 = 4"), "Solve x^2 = 4");
        assert_eq!(
            title_from_prompt("\n\n  ok so lets add dark mode."),
            "Add dark mode"
        );
        assert_eq!(title_from_prompt(""), "");
        let long = "word ".repeat(40);
        let t = title_from_prompt(&long);
        assert!(t.chars().count() <= 80 && t.ends_with("word"), "{t}");
    }

    #[test]
    fn summaries_read_like_a_sentence() {
        let s = summarize(
            "## Done\n\nI **fixed** the `autosize`.\n\n```rust\nfn x() {}\n```\n- tests pass\n---\n",
        );
        assert_eq!(s, "Done I fixed the autosize. tests pass");
        assert_eq!(
            summarize(&"x".repeat(1_000)).chars().count(),
            SUMMARY_CAP + 1
        );
    }

    #[test]
    fn auto_keys_count_like_columns() {
        assert_eq!(auto_step_key(&[]), "a");
        assert_eq!(auto_step_key(&["a".into(), "b".into()]), "c");
        let all: Vec<String> = (b'a'..=b'z').map(|c| char::from(c).to_string()).collect();
        assert_eq!(auto_step_key(&all), "aa");
    }

    #[test]
    fn sessions_without_a_project_file_under_none() {
        assert_eq!(project_of_session("default"), None);
        assert_eq!(project_of_session(" "), None);
        assert_eq!(project_of_session("parzi").as_deref(), Some("parzi"));
    }
}
