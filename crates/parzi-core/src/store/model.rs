use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    #[default]
    Idle,
    Done,
    Killed,
    Queued,
}

impl SessionStatus {
    pub(crate) fn is_terminal(self) -> bool {
        matches!(self, Self::Idle | Self::Done | Self::Killed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub lane: String,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub status: SessionStatus,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub context_tokens: u64,
    #[serde(default)]
    pub context_limit: u64,
    #[serde(default)]
    pub cwd: String,
    /// How its latest run ended and what that run did; None before any.
    #[serde(default)]
    pub last_run: Option<RunOutcome>,
    /// The plan item it works on. A child counts for its parent's item.
    #[serde(default)]
    pub plan_item: Option<String>,
    /// The git branch its folder was on when its latest run ended.
    #[serde(default)]
    pub branch: String,
    /// The effort its latest run used ("" before any).
    #[serde(default)]
    pub effort: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

/// Why a run started: a person's send (the desk's or the CLI's), or Parzi
/// on its own (a message from another session, a wake, a restart).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunCause {
    #[default]
    #[serde(alias = "later")]
    Send,
    Wake,
    Message,
    Cli,
    Restart,
    /// A `/compact` turn: housekeeping, never recorded as the session's run.
    Compact,
}

impl RunCause {
    /// A person asked for it, at the desk or on the command line.
    #[must_use]
    pub fn from_person(self) -> bool {
        matches!(self, Self::Send | Self::Cli)
    }

    /// Whether its outcome becomes the session's `last_run`.
    #[must_use]
    pub fn recorded(self) -> bool {
        self != Self::Compact
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw.trim().to_ascii_lowercase().as_str() {
            "send" => Self::Send,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    /// Named by one of the run's edit tool calls.
    Tool,
    /// Seen changed by git only.
    Git,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTouch {
    pub path: String,
    pub via: Via,
}

/// A build, test or lint command the run ran, and whether it passed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub cmd: String,
    pub ok: bool,
}

/// A run's outcome as its session keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunOutcome {
    pub end: RunEndKind,
    /// Its last finished reply, or why it stopped, as one short paragraph.
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub files: Vec<FileTouch>,
    /// Files changed beyond those listed.
    #[serde(default)]
    pub files_more: usize,
    /// The latest result of each check command, oldest first.
    #[serde(default)]
    pub checks: Vec<Check>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub ms: u64,
    /// When it ended (ms since the epoch).
    #[serde(default)]
    pub ended: u64,
}

impl RunOutcome {
    pub const SUMMARY_CAP: usize = 600;
    pub const FILES_CAP: usize = 20;
    pub const CHECKS_CAP: usize = 10;

    /// An outcome with nothing measured: a run cut off, or never started.
    #[must_use]
    pub fn bare(end: RunEndKind, ended: u64) -> Self {
        Self {
            end,
            summary: String::new(),
            files: vec![],
            files_more: 0,
            checks: vec![],
            cost_usd: None,
            ms: 0,
            ended,
        }
    }

    /// The same outcome cut to what a session keeps: the summary to
    /// `SUMMARY_CAP` chars, the files to `FILES_CAP` (the rest counted),
    /// and the checks to the latest result of each command, the last
    /// `CHECKS_CAP` of those.
    #[must_use]
    pub fn capped(mut self) -> Self {
        self.summary = crate::text::clip(self.summary.trim(), Self::SUMMARY_CAP);
        if self.files.len() > Self::FILES_CAP {
            self.files_more += self.files.len() - Self::FILES_CAP;
            self.files.truncate(Self::FILES_CAP);
        }
        let mut latest: Vec<Check> = vec![];
        for c in self.checks {
            latest.retain(|k| k.cmd != c.cmd);
            latest.push(c);
        }
        let cut = latest.len().saturating_sub(Self::CHECKS_CAP);
        latest.drain(..cut);
        self.checks = latest;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    System {
        text: String,
    },
    User {
        text: String,
    },
    Assistant {
        text: String,
        #[serde(default)]
        done: bool,
    },
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    ToolResult {
        id: String,
        name: String,
        #[serde(default)]
        ok: bool,
        output: String,
        #[serde(default)]
        ms: u64,
    },
    Widget {
        fence: String,
        payload: serde_json::Value,
    },
    Artifact {
        id: String,
        title: String,
        artifact_kind: String,
        version: u32,
        payload: serde_json::Value,
    },
    Checkpoint {
        summary: String,
    },
    Reasoning {
        text: String,
    },
    Error {
        message: String,
        #[serde(default)]
        class: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outcome_keeps_the_latest_check_of_each_command_within_its_caps() {
        let mut o = RunOutcome::bare(RunEndKind::Completed, 5);
        o.summary = "x".repeat(2_000);
        o.files = (0..25)
            .map(|i| FileTouch {
                path: format!("f{i}"),
                via: Via::Tool,
            })
            .collect();
        o.checks = (0..12)
            .map(|i| Check {
                cmd: format!("c{i}"),
                ok: true,
            })
            .chain([Check {
                cmd: "c11".into(),
                ok: false,
            }])
            .collect();
        let o = o.capped();
        assert_eq!(o.summary.chars().count(), RunOutcome::SUMMARY_CAP);
        assert_eq!(o.files.len(), 20);
        assert_eq!(o.files_more, 5);
        assert_eq!(o.checks.len(), 10);
        assert_eq!(o.checks[0].cmd, "c2", "the oldest go");
        assert_eq!(
            o.checks.last(),
            Some(&Check {
                cmd: "c11".into(),
                ok: false
            }),
            "a rerun counts once, with its latest result"
        );
    }

    #[test]
    fn outcomes_read_as_the_desk_expects() {
        let mut o = RunOutcome::bare(RunEndKind::NotStarted, 7);
        o.files.push(FileTouch {
            path: "a.rs".into(),
            via: Via::Git,
        });
        let v = serde_json::to_value(&o).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "end": "not_started", "summary": "", "files": [{"path": "a.rs", "via": "git"}],
                "files_more": 0, "checks": [], "cost_usd": null, "ms": 0, "ended": 7,
            })
        );
        assert_eq!(RunCause::parse("CLI"), Some(RunCause::Cli));
        assert!(RunCause::Send.from_person() && !RunCause::Wake.from_person());
        assert!(!RunCause::Compact.recorded());
        let old: RunCause = serde_json::from_str(r#""later""#).unwrap();
        assert_eq!(old, RunCause::Send, "a queued run from before still loads");
    }

    #[test]
    fn a_meta_from_before_reads_with_no_run_and_no_plan() {
        let m: SessionMeta = serde_json::from_str(
            r#"{"id": "x", "created": "2026-01-01T00:00:00Z", "updated": "2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert!(m.last_run.is_none() && m.plan_item.is_none());
        assert_eq!((m.branch.as_str(), m.effort.as_str()), ("", ""));
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["last_run"], serde_json::Value::Null);
        assert_eq!(v["plan_item"], serde_json::Value::Null);
    }
}
