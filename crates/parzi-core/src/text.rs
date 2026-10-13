//! Short text made from longer text: a session's first name from what the
//! person asked, and a run's summary from its last reply.

/// At most `n` chars, ending in `…` when something was cut.
#[must_use]
pub fn clip(s: &str, n: usize) -> String {
    if s.char_indices().nth(n).is_none() {
        return s.to_string();
    }
    let head: String = s.chars().take(n.saturating_sub(1)).collect();
    format!("{}…", head.trim_end())
}

/// One line: every run of whitespace, newlines included, is one space.
#[must_use]
pub fn fold(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first 8 chars of an id, as ids are shown to people and agents.
#[must_use]
pub fn short_id(id: &str) -> &str {
    id.char_indices().nth(8).map_or(id, |(at, _)| &id[..at])
}

/// A run's last message, made fit for a card: markdown marks and code
/// blocks go, whitespace folds, and it stops at `cap` chars.
#[must_use]
pub fn summarize(text: &str, cap: usize) -> String {
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
    clip(&out.join(" "), cap)
}

/// Openers people start a request with that say nothing about the work.
/// Longer ones first, so "can you please" goes before "can you".
const OPENERS: &[&str] = &[
    "hey there",
    "hi there",
    "hello there",
    "ok so",
    "okay so",
    "so basically",
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
    "i want to",
    "i'd like to",
    "help me",
    "let's",
    "lets",
    "let us",
    "hey",
    "hi",
    "hello",
    "ok",
    "okay",
    "so",
    "please",
    "pls",
    "now",
    "quick question",
];

/// Words a name should not end on: what follows them was cut off.
const DANGLING: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "so", "then", "to", "for", "with", "of", "in", "on",
    "at", "by", "from", "into", "onto", "that", "which", "who", "is", "are", "be", "my", "our",
    "your", "as", "if", "when", "while", "than", "via", "about",
];

/// The one rule for a session's name, from its prompt or `session.rename`:
/// at most 5 words and 60 chars.
pub const NAME_WORDS: usize = 5;
pub const NAME_CHARS: usize = 60;
/// What a session is called when its prompt says nothing at all.
pub const UNNAMED: &str = "New session";

/// A session's first name from what the person asked: the first clause of
/// the first line, the openers ("hey", "can you", "please", "lets", "I want
/// you to") stripped, at most five words, in sentence case. A request that
/// is nothing but an opener keeps its words; an empty one is a new session.
#[must_use]
pub fn name_from_prompt(prompt: &str) -> String {
    let first = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("```") && !l.starts_with('<'))
        .unwrap_or("");
    let first = first.trim_start_matches(['#', '>', '-', '*', ' ']);
    let stripped = strip_openers(first);
    let named = shorten(first_clause(&stripped));
    let named = if named.is_empty() {
        shorten(first)
    } else {
        named
    };
    if named.is_empty() {
        return UNNAMED.into();
    }
    sentence_case(&named)
}

fn strip_openers(line: &str) -> String {
    let opener = |t: &str| {
        OPENERS.iter().find_map(|o| {
            let head = t.get(..o.len())?;
            let next = t[o.len()..].chars().next();
            // Whole words only: "so" is not the start of "solve".
            (head.eq_ignore_ascii_case(o) && next.is_none_or(|c| !c.is_alphanumeric()))
                .then_some(o.len())
        })
    };
    let mut t = line.to_string();
    while let Some(cut) = opener(&t) {
        t = t[cut..]
            .trim_start_matches(|c: char| {
                matches!(c, ',' | '!' | ':' | '.' | '?') || c.is_whitespace()
            })
            .to_string();
    }
    t
}

/// A request up to the end of its first sentence or clause ("Fix the
/// autosize. Then..." or "...with the line: hello"), when what comes
/// before is at least two words. A dot inside a word ("notes.txt") is not
/// an end.
fn first_clause(t: &str) -> &str {
    const ENDS: &[&str] = &[
        ". ",
        "? ",
        "! ",
        ": ",
        "; ",
        ", ",
        " - ",
        " (",
        " because ",
        " so ",
        " and then ",
    ];
    let cut = ENDS
        .iter()
        .filter_map(|e| t.find(e))
        .filter(|at| t[..*at].split_whitespace().count() >= 2)
        .min();
    match cut {
        Some(at) => &t[..at],
        None => t,
    }
}

/// At most `NAME_WORDS` words and `NAME_CHARS` chars, without trailing
/// punctuation or a word that leaves the name hanging.
fn shorten(t: &str) -> String {
    let mut words: Vec<&str> = vec![];
    let mut len = 0;
    for w in t.split_whitespace().take(NAME_WORDS) {
        let add = w.chars().count() + usize::from(!words.is_empty());
        if len + add > NAME_CHARS {
            if words.is_empty() {
                words.push(w);
            }
            break;
        }
        len += add;
        words.push(w);
    }
    let trim = |w: &str| {
        w.trim_end_matches(['?', '.', '!', ',', ':', ';', '"', '\''])
            .to_string()
    };
    while words.len() > 1 {
        let last = trim(words[words.len() - 1]).to_lowercase();
        if last.is_empty() || DANGLING.contains(&last.as_str()) {
            words.pop();
        } else {
            break;
        }
    }
    let joined = words.join(" ");
    let out = trim(&joined);
    if out.chars().count() > NAME_CHARS {
        clip(&out, NAME_CHARS)
    } else {
        out
    }
}

fn sentence_case(t: &str) -> String {
    let mut chars = t.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_the_pleasantries() {
        let cases = [
            (
                "hey, can you please fix the omnibar autosize?\nmore",
                "Fix the omnibar autosize",
            ),
            ("solve x^2 = 4", "Solve x^2 = 4"),
            ("\n\n  ok so lets add dark mode.", "Add dark mode"),
            (
                "I want you to refactor the session store so it caches metas",
                "Refactor the session store",
            ),
            (
                "Create a file named notes.txt in the current folder with the single line: hello",
                "Create a file named notes.txt",
            ),
            (
                "Fix the omnibar autosize. It jumps when I paste. Thanks!",
                "Fix the omnibar autosize",
            ),
            (
                "Bump the version (to 0.1.28) everywhere",
                "Bump the version",
            ),
            ("please run the tests", "Run the tests"),
            ("Lets test something", "Test something"),
            ("hey!", "Hey"),
            ("", UNNAMED),
            ("   \n  ", UNNAMED),
            ("Note: x", "Note: x"),
            ("Plan the next release", "Plan the next release"),
            ("start this", "Start this"),
            ("fix it", "Fix it"),
            // Five words at most, and never ending on a dangling word.
            ("add a test for the parser, then ship", "Add a test"),
            ("why is the PR badge grey?", "Why is the PR badge"),
        ];
        for (prompt, want) in cases {
            assert_eq!(name_from_prompt(prompt), want, "{prompt:?}");
        }
    }

    #[test]
    fn names_stay_short() {
        let long = "word ".repeat(40);
        assert_eq!(name_from_prompt(&long), "Word word word word word");
        let wide = format!("fix {}", "x".repeat(200));
        let n = name_from_prompt(&wide);
        assert!(n.chars().count() <= NAME_CHARS, "{n}");
        assert!(n.starts_with("Fix"), "{n}");
        let one = "y".repeat(90);
        assert!(name_from_prompt(&one).chars().count() <= NAME_CHARS);
    }

    #[test]
    fn summaries_read_like_a_sentence() {
        let s = summarize(
            "## Done\n\nI **fixed** the `autosize`.\n\n```rust\nfn x() {}\n```\n- tests pass\n---\n",
            600,
        );
        assert_eq!(s, "Done I fixed the autosize. tests pass");
        let long = summarize(&"x".repeat(1_000), 600);
        assert_eq!(long.chars().count(), 600);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn clip_counts_chars_and_stays_within_its_cap() {
        assert_eq!(clip("héllo", 3), "hé…");
        assert_eq!(clip("hé", 2), "hé");
        assert_eq!(clip("abc", 3), "abc");
    }

    #[test]
    fn folds_and_short_ids() {
        assert_eq!(fold("  a\n b\t\tc "), "a b c");
        assert_eq!(short_id("0123456789abcdef"), "01234567");
        assert_eq!(short_id("abc"), "abc");
        assert_eq!(short_id("ééééééééé"), "éééééééé", "chars, not bytes");
    }
}
