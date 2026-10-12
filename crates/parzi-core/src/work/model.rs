//! What the work ledger records. One writer appends `Entry` lines; every
//! surface (the Work view, the agent brief, the CLI) is a fold over them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Caps applied before an event is written: the ledger stays small and a
/// runaway agent cannot bloat it.
pub const TITLE_CAP: usize = 120;
pub const PROMPT_CAP: usize = 2_000;
pub const SUMMARY_CAP: usize = 600;
pub const FILES_CAP: usize = 200;
pub const CHECKS_CAP: usize = 10;
pub const STEPS_CAP: usize = 30;
pub const DETAIL_CAP: usize = 4 * 1024;
const NOTE_CAP: usize = 2_000;
const ERROR_CAP: usize = 600;
const LABEL_CAP: usize = 200;

/// One line of a writer's log. `seq` is gapless per writer; `at` (ms) is for
/// display and the writer's own age rules, never for ordering across writers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub w: String,
    pub seq: u64,
    pub at: u64,
    pub ev: WorkEvent,
}

/// Why a run started. Only Send, Later and Cli can make a goal: a run woken
/// by a message or restored after a restart works on whatever it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunCause {
    #[default]
    Send,
    Later,
    Wake,
    Message,
    Cli,
    Restart,
    /// A `/compact` turn: housekeeping, never written to the ledger.
    Compact,
}

impl RunCause {
    #[must_use]
    pub fn makes_goals(self) -> bool {
        matches!(self, Self::Send | Self::Later | Self::Cli)
    }

    /// Whether runs with this cause are recorded at all.
    #[must_use]
    pub fn ledgered(self) -> bool {
        self != Self::Compact
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim().to_ascii_lowercase().as_str() {
            "send" => Self::Send,
            "later" => Self::Later,
            "wake" => Self::Wake,
            "message" => Self::Message,
            "cli" => Self::Cli,
            "restart" => Self::Restart,
            "compact" => Self::Compact,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEndKind {
    Completed,
    Failed,
    Paused,
    Stopped,
    Crashed,
    Interrupted,
    NotStarted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    #[default]
    Todo,
    Running,
    Done,
    Failed,
    Skipped,
    Check,
}

impl StepStatus {
    /// Done or skipped: nothing that needs it waits any longer.
    #[must_use]
    pub fn settled(self) -> bool {
        matches!(self, Self::Done | Self::Skipped)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Later,
    Working,
    NeedsYou,
    Paused,
    Check,
    Done,
    Failed,
    Stopped,
    CutOff,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalMark {
    Accepted,
    Reopened,
    Archived,
    Unarchived,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskKind {
    Approval,
    Question,
    Request,
    Budget,
    Continue,
}

impl AskKind {
    /// Something a live run waits on: gone when the engine restarts.
    #[must_use]
    pub fn waits(self) -> bool {
        matches!(self, Self::Approval | Self::Question | Self::Request)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskHow {
    Allowed,
    Denied,
    Answered,
    Continued,
    Expired,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Later {
    #[serde(default)]
    pub auto_start: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub lane: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    Tool,
    Git,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTouch {
    pub path: String,
    pub via: Via,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub cmd: String,
    pub ok: bool,
}

/// A run's outcome, as `RunEnded` carries it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunOutcome {
    pub end: RunEndKind,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub ms: u64,
    #[serde(default)]
    pub turns: u32,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub files: Vec<FileTouch>,
    #[serde(default)]
    pub files_more: usize,
    #[serde(default)]
    pub checks: Vec<Check>,
    #[serde(default)]
    pub artifacts: u32,
}

impl RunOutcome {
    /// An outcome with nothing measured: a run cut off by a restart.
    #[must_use]
    pub fn bare(end: RunEndKind) -> Self {
        Self {
            end,
            error: None,
            ms: 0,
            turns: 0,
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: None,
            summary: String::new(),
            files: vec![],
            files_more: 0,
            checks: vec![],
            artifacts: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkEvent {
    /// First line of a writer: which machine and engine it is.
    Hello {
        device: String,
        name: String,
        engine: String,
    },
    GoalCreated {
        goal: String,
        title: String,
        #[serde(default)]
        project: Option<String>,
        /// The session that leads it; None for a Later goal not started yet.
        #[serde(default)]
        root: Option<String>,
        #[serde(default)]
        prompt: String,
        origin: RunCause,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        later: Option<Later>,
        /// The earlier goal of the same root session this one follows.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        continues: Option<String>,
    },
    GoalStarted {
        goal: String,
        root: String,
    },
    GoalRetitled {
        goal: String,
        title: String,
    },
    GoalMarked {
        goal: String,
        mark: GoalMark,
    },
    LaterChanged {
        goal: String,
        #[serde(default)]
        later: Option<Later>,
    },
    NoteAdded {
        goal: String,
        by: String,
        text: String,
        #[serde(default)]
        step: Option<String>,
    },
    PlanSet {
        goal: String,
        steps: Vec<PlanStep>,
    },
    StepAdded {
        goal: String,
        step: PlanStep,
        #[serde(default)]
        auto: bool,
    },
    StepMarked {
        goal: String,
        step: String,
        status: StepStatus,
        #[serde(default)]
        note: Option<String>,
        #[serde(default)]
        outcome: Option<String>,
    },
    StepLinked {
        goal: String,
        step: String,
        session: String,
    },
    RunStarted {
        run: String,
        session: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        lane: String,
        #[serde(default)]
        cwd: String,
        cause: RunCause,
    },
    RunEnded {
        run: String,
        #[serde(flatten)]
        outcome: RunOutcome,
    },
    AskOpened {
        ask: String,
        session: String,
        kind: AskKind,
        label: String,
        #[serde(default)]
        detail: Value,
        #[serde(default)]
        expires: Option<u64>,
    },
    AskResolved {
        ask: String,
        how: AskHow,
        #[serde(default)]
        by: String,
    },
    /// A kind a newer Parzi wrote: it keeps its sequence number, nothing more.
    #[serde(other)]
    Unknown,
}

/// At most `n` chars, with an ellipsis when something was cut.
#[must_use]
pub fn clip(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((at, _)) => {
            let head = s[..at].trim_end();
            format!("{head}…")
        }
        None => s.to_string(),
    }
}

impl WorkEvent {
    /// The same event with every free text and list cut to its cap.
    #[must_use]
    pub fn capped(self) -> Self {
        match self {
            Self::GoalCreated {
                goal,
                title,
                project,
                root,
                prompt,
                origin,
                later,
                continues,
            } => Self::GoalCreated {
                goal,
                title: clip(title.trim(), TITLE_CAP),
                project,
                root,
                prompt: clip(&prompt, PROMPT_CAP),
                origin,
                later,
                continues,
            },
            Self::GoalRetitled { goal, title } => Self::GoalRetitled {
                goal,
                title: clip(title.trim(), TITLE_CAP),
            },
            Self::NoteAdded {
                goal,
                by,
                text,
                step,
            } => Self::NoteAdded {
                goal,
                by,
                text: clip(&text, NOTE_CAP),
                step,
            },
            Self::PlanSet { goal, steps } => Self::PlanSet {
                goal,
                steps: steps.into_iter().take(STEPS_CAP).map(cap_step).collect(),
            },
            Self::StepAdded { goal, step, auto } => Self::StepAdded {
                goal,
                step: cap_step(step),
                auto,
            },
            Self::StepMarked {
                goal,
                step,
                status,
                note,
                outcome,
            } => Self::StepMarked {
                goal,
                step,
                status,
                note: note.map(|n| clip(&n, NOTE_CAP)),
                outcome: outcome.map(|o| clip(&o, SUMMARY_CAP)),
            },
            Self::RunEnded { run, mut outcome } => {
                outcome.summary = clip(&outcome.summary, SUMMARY_CAP);
                outcome.error = outcome.error.map(|e| clip(&e, ERROR_CAP));
                if outcome.files.len() > FILES_CAP {
                    outcome.files_more += outcome.files.len() - FILES_CAP;
                    outcome.files.truncate(FILES_CAP);
                }
                if outcome.checks.len() > CHECKS_CAP {
                    let cut = outcome.checks.len() - CHECKS_CAP;
                    outcome.checks.drain(..cut);
                }
                Self::RunEnded { run, outcome }
            }
            Self::AskOpened {
                ask,
                session,
                kind,
                label,
                detail,
                expires,
            } => {
                let detail = if detail.to_string().len() > DETAIL_CAP {
                    Value::String(clip(&detail.to_string(), DETAIL_CAP / 2))
                } else {
                    detail
                };
                Self::AskOpened {
                    ask,
                    session,
                    kind,
                    label: clip(&label, LABEL_CAP),
                    detail,
                    expires,
                }
            }
            other => other,
        }
    }
}

fn cap_step(s: PlanStep) -> PlanStep {
    PlanStep {
        key: clip(s.key.trim(), 24),
        title: clip(s.title.trim(), TITLE_CAP),
        needs: s.needs.into_iter().take(STEPS_CAP).collect(),
        lane: s.lane,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_read_back_as_written() {
        let e = Entry {
            w: "pc-ab12".into(),
            seq: 3,
            at: 10,
            ev: WorkEvent::RunEnded {
                run: "r1".into(),
                outcome: RunOutcome {
                    summary: "ok".into(),
                    files: vec![FileTouch {
                        path: "a.rs".into(),
                        via: Via::Git,
                    }],
                    ..RunOutcome::bare(RunEndKind::Completed)
                },
            },
        };
        let line = serde_json::to_string(&e).unwrap();
        assert!(line.contains(r#""type":"run_ended""#), "{line}");
        assert!(line.contains(r#""end":"completed""#), "{line}");
        assert_eq!(serde_json::from_str::<Entry>(&line).unwrap(), e);
    }

    #[test]
    fn a_newer_kind_reads_as_unknown() {
        let ev: WorkEvent = serde_json::from_str(r#"{"type":"goal_teleported","x":1}"#).unwrap();
        assert_eq!(ev, WorkEvent::Unknown);
    }

    #[test]
    fn caps_cut_text_and_lists() {
        let mut outcome = RunOutcome::bare(RunEndKind::Completed);
        outcome.summary = "x".repeat(2_000);
        outcome.files = (0..250)
            .map(|i| FileTouch {
                path: format!("f{i}"),
                via: Via::Tool,
            })
            .collect();
        outcome.checks = (0..12)
            .map(|i| Check {
                cmd: format!("c{i}"),
                ok: true,
            })
            .collect();
        let WorkEvent::RunEnded { outcome, .. } = (WorkEvent::RunEnded {
            run: "r".into(),
            outcome,
        })
        .capped() else {
            unreachable!()
        };
        assert_eq!(outcome.summary.chars().count(), SUMMARY_CAP + 1);
        assert_eq!(outcome.files.len(), FILES_CAP);
        assert_eq!(outcome.files_more, 50);
        assert_eq!(outcome.checks.len(), CHECKS_CAP);
        assert_eq!(outcome.checks[0].cmd, "c2", "the latest checks stay");
    }

    #[test]
    fn causes_parse_and_only_people_make_goals() {
        assert_eq!(RunCause::parse("CLI"), Some(RunCause::Cli));
        assert!(RunCause::Send.makes_goals() && RunCause::Later.makes_goals());
        assert!(!RunCause::Message.makes_goals() && !RunCause::Wake.makes_goals());
        assert!(!RunCause::Compact.ledgered());
    }
}
