//! A call before it runs: arguments fitted to the tool, a vendor shell sent
//! to Parzi's, refusals worded, and near misses on a tool's name.

use std::time::Duration;

use serde_json::Value;

use crate::tools::{bool_arg, command_arg, display_name, int_arg, Approval, CallNames};

pub(super) fn command_text(input: &Value) -> String {
    command_arg(input).unwrap_or_default()
}

/// A session name over the limits, cut to fit at a word boundary instead of
/// refused, with the note the result carries.
pub(super) fn fitted_name(name: &str, args: &Value) -> Option<(Value, String)> {
    use parzi_core::text::{fit_name, fold, NAME_CHARS, NAME_WORDS};
    if name != "session.rename" {
        return None;
    }
    let asked = fold(args.get("title")?.as_str()?);
    if asked.chars().count() <= NAME_CHARS && asked.split(' ').count() <= NAME_WORDS {
        return None;
    }
    let fitted = fit_name(&asked);
    let mut out = args.clone();
    out["title"] = Value::String(fitted.clone());
    let note = format!(
        " \"{asked}\" was over {NAME_WORDS} words or {NAME_CHARS} characters, so it was cut to \"{fitted}\"; rename again for a better short name."
    );
    Some((out, note))
}

pub(super) fn legacy_args(name: &str, args: &Value) -> Value {
    let mut a = args.clone();
    if name == "ask.user" {
        if let Some(o) = a.as_object_mut() {
            if !o.contains_key("question") {
                if let Some(r) = o.remove("request") {
                    o.insert("question".into(), r);
                }
            }
        }
    }
    a
}

/// Agents' own shell tools, refused while Parzi serves the shell.
pub(super) const VENDOR_SHELL: &[&str] = &[
    "Bash",
    "bash",
    "PowerShell",
    "Monitor",
    "BashOutput",
    "KillShell",
];

/// Why an agent's own shell tool was refused, and the Parzi call that does the
/// same, with its arguments filled in from the refused call.
pub(super) fn shell_detour(tool: &str, input: &Value, names: &CallNames) -> String {
    let command = command_text(input);
    let command = if command.trim().is_empty() {
        "<the same command>".to_string()
    } else {
        command
    };
    let id = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| input.get(*k).and_then(Value::as_str))
            .filter(|id| id.starts_with("sh-"))
            .unwrap_or("<the id shell_start returned>")
            .to_string()
    };
    let call = |name: &str, args: Value| format!("{} with {args}", names.tool(name));
    let off = format!("{tool} is off in Parzi.");
    match tool {
        "BashOutput" => format!(
            "{off} Read a background command's output: call {}.",
            call(
                "shell.logs",
                serde_json::json!({"id": id(&["bash_id", "shell_id", "id"])})
            )
        ),
        "KillShell" => format!(
            "{off} Stop a background command: call {}.",
            call(
                "shell.kill",
                serde_json::json!({"id": id(&["shell_id", "bash_id", "id"])})
            )
        ),
        "Monitor" => format!(
            "{off} Start the script: call {}, then read its new lines with {} and the id it returns.",
            call("shell.start", serde_json::json!({"cmd": command})),
            names.tool("shell.logs")
        ),
        _ if bool_arg(input, "run_in_background") == Some(true) => format!(
            "{off} Call {}, then read its output with {}.",
            call("shell.start", serde_json::json!({"cmd": command})),
            names.tool("shell.logs")
        ),
        _ => {
            let mut args = serde_json::json!({"cmd": command});
            if let Some(ms) = int_arg(input, "timeout") {
                args["timeout_ms"] = Value::from(ms);
            }
            format!("{off} Call {}.", call("shell.exec", args))
        }
    }
}

/// The plan tools' own refusal. `ToolHost::unavailable` and the read-only
/// check before them say why first, so this is only the last word.
pub(super) fn denied(name: &str) -> (bool, String) {
    (
        false,
        format!("`{name}` did not run: agents are read-only or it is off in Settings"),
    )
}

/// Why a call did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    /// Agents are read-only (Settings).
    ReadOnly,
    /// Not in this session's tools (Settings).
    NotEnabled,
    /// The shell is switched off in Settings.
    ShellOff,
    /// Its connector sets it to Deny (Settings).
    ConnectorOff,
    /// The person said no on the card.
    Declined,
    /// The card's wait ran out with no answer.
    Unanswered,
    /// This run has no one who can answer a card.
    NoOneToAsk,
    /// The run was stopped while the card waited.
    Stopped,
}

/// What happened to a call of `what` (the tool, named as the agent calls it)
/// and what the agent can do next.
pub(super) fn refusal_text(what: &str, why: Refusal) -> String {
    match why {
        Refusal::ReadOnly => format!(
            "{what} was denied: agents are read-only in Settings. Reading still works; say what you would do instead, and the person can allow changes in Settings."
        ),
        Refusal::NotEnabled => format!(
            "{what} is not enabled for agents in Settings. Do it another way, or ask the person to turn it on."
        ),
        Refusal::ShellOff => format!(
            "{what} was denied: the shell is switched off in Settings. Do it without a command, or ask the person to turn the shell on."
        ),
        Refusal::ConnectorOff => format!(
            "{what} is set to Deny for its connector in Settings. Do it another way, or ask the person to allow it there."
        ),
        Refusal::Declined => format!(
            "The person declined {what}. Do not retry it as is: ask them in chat what they want instead."
        ),
        Refusal::Unanswered => format!(
            "Nobody answered the approval for {what} in time, so it did not run. Ask the person in chat whether they still want it, then try again."
        ),
        Refusal::NoOneToAsk => format!(
            "{what} needs the person's approval, and no one can answer an approval in this run, so it did not run. Say what you would do, so the person can do it or allow it."
        ),
        Refusal::Stopped => "stopped: the run was cancelled".into(),
    }
}

/// An approver's answer, read with how long it took: an approver that reports
/// a card that ran out as a plain no answers at the end of the wait.
pub(super) fn answered(
    answer: Approval,
    waited: Duration,
    wait: Duration,
) -> std::result::Result<(), Refusal> {
    match answer {
        Approval::Allow => Ok(()),
        Approval::Expired => Err(Refusal::Unanswered),
        Approval::Deny if waited + Duration::from_secs(1) >= wait => Err(Refusal::Unanswered),
        Approval::Deny => Err(Refusal::Declined),
    }
}

pub(super) fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if ca == *cb {
                diagonal
            } else {
                1 + diagonal.min(above).min(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

/// Up to three offered tools an agent asking for `asked` most likely meant:
/// the same namespace, a near spelling, or one name inside the other.
pub(super) fn close_matches(asked: &str, offered: &[String]) -> Vec<String> {
    let want = crate::tools::to_mcp(&display_name(asked)).to_lowercase();
    let space = |s: &str| s.split('_').next().unwrap_or("").to_string();
    let mut near: Vec<(usize, &String)> = offered
        .iter()
        .filter_map(|name| {
            let have = crate::tools::to_mcp(name).to_lowercase();
            let d = edit_distance(&want, &have);
            let longest = want.chars().count().max(have.chars().count());
            let close = d <= longest / 3
                || space(&want) == space(&have)
                || have.contains(&want)
                || want.contains(&have);
            close.then_some((d, name))
        })
        .collect();
    near.sort();
    near.into_iter().take(3).map(|(_, n)| n.clone()).collect()
}

pub(super) fn no_such_tool(asked: &str, offered: &[String], names: &CallNames) -> String {
    let near: Vec<String> = close_matches(asked, offered)
        .iter()
        .map(|n| names.short(n))
        .collect();
    let head = format!("There is no tool {}.", names.tool(asked));
    match near.as_slice() {
        [] => format!("{head} Call one of the tools Parzi lists."),
        [one] => format!("{head} Did you mean {one}?"),
        [first @ .., last] => format!("{head} Did you mean {} or {last}?", first.join(", ")),
    }
}
