//! A session's plan (its checklist, kept on the session) and its name.
//! `plan.write` (also called by its old name `todo.write`) sets the plan; `plan.read` reads this
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

#[cfg(test)]
mod tests {
    use super::*;
    use parzi_core::store::PlanStep;

    #[test]
    fn a_plan_reads_as_marked_lines() {
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
    }
}
