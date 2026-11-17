//! A session's plan (its checklist, kept on the session) and its name.
//! `plan.write` and `todo.write` set the plan; `plan.read` reads this
//! session's plan or another's in the same project.

use parzi_core::store::{SessionPlan, SessionStore, StepState};
use parzi_core::text::{fold, short_id, NAME_CHARS, NAME_WORDS};
use serde_json::Value;

pub(crate) fn execute(store: &SessionStore, sid: &str, name: &str, args: &Value) -> (bool, String) {
    let out = match name {
        "plan.read" => read(store, sid, args),
        "session.rename" => rename(store, sid, args),
        _ => Err(format!("unknown tool `{name}`")),
    };
    match out {
        Ok(text) => (true, text),
        Err(e) => (false, e),
    }
}

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn rename(store: &SessionStore, sid: &str, args: &Value) -> Result<String, String> {
    let title = str_arg(args, "title")
        .map(|t| fold(&t))
        .ok_or("session.rename needs a `title`")?;
    let (len, words) = (title.chars().count(), title.split(' ').count());
    if len > NAME_CHARS || words > NAME_WORDS {
        return Err(format!(
            "that name is {len} chars in {words} words; keep it to {NAME_CHARS} chars, 2 to {NAME_WORDS} words"
        ));
    }
    store.set_title(sid, &title).map_err(|e| e.to_string())?;
    Ok(format!("This session is now called \"{title}\"."))
}

/// A plan as lines an agent or a person reads: `[x] done`, `[>] active`.
pub(crate) fn lines(plan: &SessionPlan) -> String {
    plan.items
        .iter()
        .map(|i| {
            let mark = match i.state {
                StepState::Done => "[x]",
                StepState::Active => "[>]",
                StepState::Pending => "[ ]",
            };
            format!("{mark} {}", i.title)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn read(store: &SessionStore, sid: &str, args: &Value) -> Result<String, String> {
    let me = store.get(sid).map_err(|e| e.to_string())?;
    let target = match str_arg(args, "session") {
        None => me,
        Some(want) => {
            let all = store.list().map_err(|e| e.to_string())?;
            let found = all
                .into_iter()
                .find(|m| m.id == want || m.id.starts_with(&want))
                .ok_or_else(|| format!("no session `{want}`"))?;
            if found.project != me.project {
                return Err(format!(
                    "session {} is in another project; plan.read reads this project's sessions",
                    short_id(&found.id)
                ));
            }
            found
        }
    };
    let whose = if target.id == sid {
        "This session".to_string()
    } else {
        format!("Session {} \"{}\"", short_id(&target.id), target.title)
    };
    Ok(match &target.plan {
        None => format!("{whose} has no plan yet. Write one with plan.write when the work has three steps or more."),
        Some(p) => format!(
            "{whose}: {} of {} done.\n{}",
            p.done(),
            p.items.len(),
            lines(p)
        ),
    })
}

/// The two answers offered under a plan proposal. Anything else the person
/// types is what they want changed.
pub(crate) const USE_PLAN: &str = "Use this plan";
pub(crate) const NOT_NOW: &str = "Not now";

/// The question for a proposed plan: the steps, numbered.
pub(crate) fn proposal_text(plan: &SessionPlan) -> String {
    let mut out = vec!["Use this plan for this session?".to_string()];
    if let Some(s) = &plan.summary {
        out.push(s.clone());
    }
    out.extend(plan.items.iter().map(|i| match &i.detail {
        Some(d) => format!("{}: {d}", i.title),
        None => i.title.clone(),
    }));
    if let Some(d) = &plan.doc {
        out.push(format!("Full plan: {d}"));
    }
    out.join("\n")
}

/// The proposal as data for the card: summary, steps with their detail, and
/// the fuller plan document.
pub(crate) fn proposal_json(plan: &SessionPlan) -> String {
    let steps: Vec<Value> = plan
        .items
        .iter()
        .map(|i| serde_json::json!({ "title": i.title, "detail": i.detail }))
        .collect();
    serde_json::json!({
        "plan": true,
        "summary": plan.summary,
        "steps": steps,
        "doc": plan.doc,
    })
    .to_string()
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Save it. `note` is what to tell the agent besides the saved result.
    Save(&'static str),
    /// Nothing is saved; the text goes back to the agent.
    Hold(String),
}

pub(crate) fn verdict(answer: &str) -> Verdict {
    let a = answer.trim();
    if a == USE_PLAN {
        return Verdict::Save("The person approved this plan.");
    }
    if a == NOT_NOW {
        return Verdict::Hold(
            "The person said not now, so nothing was saved. Carry on without this plan, and do not propose it again unless they ask.".into(),
        );
    }
    let lower = a.to_ascii_lowercase();
    if lower.starts_with("skipped") {
        return Verdict::Save("The person skipped the review, so it is saved as you wrote it.");
    }
    if a.is_empty() || lower.starts_with("the user didn't answer") {
        return Verdict::Hold(
            "Nobody answered, so nothing was saved. Say in your reply what plan you would propose; the person can ask you to save it.".into(),
        );
    }
    Verdict::Hold(format!(
        "The person wants changes before this plan is saved: \"{a}\". Nothing was saved. Change it and call plan.write again with propose: true."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use parzi_core::store::PlanStep;

    #[test]
    fn the_persons_answer_decides_whether_the_plan_is_saved() {
        assert!(matches!(verdict(USE_PLAN), Verdict::Save(_)));
        assert!(matches!(verdict("Skipped: auto"), Verdict::Save(_)));
        assert!(matches!(verdict(NOT_NOW), Verdict::Hold(_)));
        assert!(matches!(verdict(""), Verdict::Hold(_)));
        match verdict("drop the second step") {
            Verdict::Hold(why) => assert!(why.contains("drop the second step")),
            Verdict::Save(_) => panic!("a change request is not a yes"),
        }
    }

    #[test]
    fn a_plan_reads_as_marked_lines_and_a_proposal_with_its_context() {
        let step = |title: &str, state, detail: Option<&str>| PlanStep {
            title: title.into(),
            state,
            detail: detail.map(str::to_string),
        };
        let p = SessionPlan {
            items: vec![
                step("Lexer", StepState::Done, None),
                step(
                    "Parser",
                    StepState::Active,
                    Some("Pratt, for the operators"),
                ),
                step("Tests", StepState::Pending, None),
            ],
            summary: Some("A small parser for the config language.".into()),
            doc: Some("/s/plan.md".into()),
        };
        assert_eq!(lines(&p), "[x] Lexer\n[>] Parser\n[ ] Tests");
        assert_eq!(
            proposal_text(&p),
            "Use this plan for this session?\nA small parser for the config language.\nLexer\nParser: Pratt, for the operators\nTests\nFull plan: /s/plan.md"
        );
        let j: Value = serde_json::from_str(&proposal_json(&p)).unwrap();
        assert_eq!(j["steps"][1]["detail"], "Pratt, for the operators");
        assert_eq!(j["doc"], "/s/plan.md");
    }
}
