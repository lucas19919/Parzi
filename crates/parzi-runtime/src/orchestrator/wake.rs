
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use parzi_core::context::InterKind;
use parzi_core::store::{Event, RunCause, RunEndKind, SessionMeta, SessionStatus};
use parzi_core::text::{clip, fold};

use crate::handler::RunEvent;
use crate::inter;

use super::normalize_effort;
use super::queue::{posture, take_report_to, Pump, QueuedRun};

const DEBOUNCE: Duration = Duration::from_secs(2);
pub(crate) const REPORT_CAP: usize = 1_500;
pub(crate) const WAKES_PER_HOUR: usize = 12;
const HOUR: Duration = Duration::from_secs(60 * 60);

pub(crate) const CAP_NOTE: &str = "Parzi stopped waking this session: it was woken 12 times in the last hour. The reports of the sessions it started are above; send a message to go on.";

#[derive(Default)]
pub(crate) struct WakeState {
    pending: HashMap<String, usize>,
    woken: HashMap<String, Vec<Instant>>,
    capped: HashSet<String>,
}

pub(crate) type Wakes = Arc<Mutex<WakeState>>;

impl WakeState {
    fn allow(&mut self, parent: &str, now: Instant) -> (bool, bool) {
        let list = self.woken.entry(parent.to_string()).or_default();
        list.retain(|t| now.saturating_duration_since(*t) < HOUR);
        if list.len() < WAKES_PER_HOUR {
            self.capped.remove(parent);
            return (true, false);
        }
        (false, self.capped.insert(parent.to_string()))
    }

    fn woke(&mut self, parent: &str, now: Instant) {
        self.woken.entry(parent.to_string()).or_default().push(now);
    }
}

fn one_line(s: &str, n: usize) -> String {
    clip(&fold(s), n)
}

pub(crate) fn report(child: &SessionMeta, stopped: bool, cap: usize) -> String {
    let o = child.last_run.as_ref();
    let end = if stopped {
        Some(RunEndKind::Stopped)
    } else {
        o.map(|o| o.end)
    };
    let mut out = format!(
        "Session \"{}\" {}.",
        one_line(&child.title, 100),
        end.map_or("ended", RunEndKind::word)
    );
    if let Some(item) = &child.plan_item {
        out.push_str(&format!(" It works on plan item {item}."));
    }
    if let Some(sum) = o.map(|o| o.summary.trim()).filter(|s| !s.is_empty()) {
        out.push('\n');
        out.push_str(&one_line(sum, 600));
    }
    if let Some(o) = o.filter(|o| !o.files.is_empty()) {
        let names: Vec<&str> = o.files.iter().take(8).map(|f| f.path.as_str()).collect();
        let more = o.files.len() - names.len() + o.files_more;
        out.push_str(&format!("\nFiles: {}", names.join(", ")));
        if more > 0 {
            out.push_str(&format!(" (+{more} more)"));
        }
        out.push('.');
    }
    if let Some(o) = o.filter(|o| !o.checks.is_empty()) {
        let words: Vec<String> = o
            .checks
            .iter()
            .map(|c| {
                let how = if c.ok { "passed" } else { "failed" };
                format!("{} {how}", one_line(&c.cmd, 60))
            })
            .collect();
        out.push_str(&format!("\nChecks: {}.", words.join(", ")));
    }
    if !child.branch.is_empty() {
        out.push_str(&format!("\nBranch: {}.", child.branch));
    }
    let more = format!("\nMore: session.read {}", child.id);
    let room = cap.saturating_sub(more.chars().count());
    format!("{}{more}", clip(&out, room))
}

impl Pump {
    pub(super) async fn report_to_parent(&self, child: &str, stopped: bool) {
        let Some(parent) = take_report_to(child) else {
            return;
        };
        let Ok(child_meta) = self.store.get(child) else {
            return;
        };
        let Ok(parent_meta) = self.store.get(&parent) else {
            return;
        };
        let body = report(&child_meta, stopped, REPORT_CAP);
        let msg = inter::from_caller(&child_meta, InterKind::Text, body);
        let (at, live) = {
            let mut h = self.handles.lock().await;
            h.retain(|_, x| !x.finished());
            let at = self.store.event_count(&parent).unwrap_or(0);
            if let Err(e) = inter::deliver(&self.store, &parent, &msg) {
                tracing::warn!("report for {parent} not delivered: {e}");
                return;
            }
            (at, h.contains_key(&parent))
        };
        if live || stopped || parent_meta.status == SessionStatus::Killed {
            return;
        }
        if !self.wake_allowed(&parent) {
            return;
        }
        let first = {
            let mut w = self.wakes.lock().unwrap_or_else(PoisonError::into_inner);
            match w.pending.get_mut(&parent) {
                Some(from) => {
                    *from = (*from).min(at);
                    false
                }
                None => {
                    w.pending.insert(parent.clone(), at);
                    true
                }
            }
        };
        if first {
            let p = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(DEBOUNCE).await;
                p.wake(&parent).await;
            });
        }
    }

    fn wake_allowed(&self, parent: &str) -> bool {
        let (ok, tell) = self
            .wakes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .allow(parent, Instant::now());
        if tell {
            let text = CAP_NOTE.to_string();
            let _ = self
                .store
                .append(parent, &Event::System { text: text.clone() });
            let _ = self
                .bus
                .send((parent.to_string(), RunEvent::Notice { text }));
        }
        ok
    }

    async fn wake(&self, parent: &str) {
        let from = self
            .wakes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            .remove(parent);
        let Some(from) = from else {
            return;
        };
        let Ok(meta) = self.store.get(parent) else {
            return;
        };
        if matches!(meta.status, SessionStatus::Killed | SessionStatus::Queued) {
            return;
        }
        {
            let mut h = self.handles.lock().await;
            h.retain(|_, x| !x.finished());
            if h.contains_key(parent) {
                return;
            }
        }
        if self
            .queue
            .lock()
            .await
            .iter()
            .any(|q| q.session_id == parent)
        {
            return;
        }
        if !self.wake_allowed(parent) {
            return;
        }
        self.wakes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .woke(parent, Instant::now());
        let last = posture(parent).unwrap_or_default();
        let q = QueuedRun {
            session_id: parent.to_string(),
            project: meta.project.clone(),
            model_spec: meta.model.clone(),
            prompt: "Sessions you started in the background have ended; their reports are above."
                .into(),
            cwd: meta.cwd.clone(),
            effort: normalize_effort(&last.effort),
            attachments: vec![],
            approver: None,
            restore_approver: last.ask,
            prompt_recorded: true,
            mode_override: last.mode_override,
            inbox_from: Some(from),
            cause: RunCause::Wake,
        };
        tracing::info!(session = %parent, "woken by the sessions it started");
        let _ = self.dispatch(q).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parzi_core::store::{Check, FileTouch, RunOutcome, Via};

    #[test]
    fn a_parent_is_woken_at_most_twelve_times_an_hour_and_told_once() {
        let mut w = WakeState::default();
        let start = Instant::now();
        for i in 0..WAKES_PER_HOUR {
            let at = start + Duration::from_secs(i as u64);
            assert_eq!(w.allow("p", at), (true, false), "wake {i}");
            w.woke("p", at);
        }
        let at = start + Duration::from_secs(60);
        assert_eq!(w.allow("p", at), (false, true), "the cap, told once");
        assert_eq!(w.allow("p", at), (false, false), "and not again");
        assert_eq!(w.allow("q", at), (true, false), "another parent is free");
        let later = start + HOUR + Duration::from_secs(30);
        assert_eq!(
            w.allow("p", later),
            (true, false),
            "an hour on it counts again"
        );
        assert!(CAP_NOTE.contains(&WAKES_PER_HOUR.to_string()));
    }

    fn child() -> SessionMeta {
        let now = chrono::Utc::now();
        let mut o = RunOutcome::bare(RunEndKind::Completed, 1);
        o.summary = "Wrote the parser. ".repeat(100);
        o.files = (0..12)
            .map(|i| FileTouch {
                path: format!("src/f{i}.rs"),
                via: Via::Tool,
            })
            .collect();
        o.checks = vec![Check {
            cmd: "cargo test".into(),
            ok: true,
        }];
        SessionMeta {
            id: "c1".into(),
            title: "Write the parser".into(),
            pinned: false,
            project: "demo".into(),
            model: String::new(),
            parent_id: Some("p1".into()),
            status: SessionStatus::Done,
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
            context_tokens: 0,
            context_limit: 0,
            cwd: String::new(),
            last_run: Some(o),
            plan_item: Some("parser".into()),
            branch: "plan/parser".into(),
            effort: String::new(),
            created: now,
            updated: now,
        }
    }

    #[test]
    fn a_report_says_how_the_run_went_within_its_cap() {
        let c = child();
        let text = report(&c, false, REPORT_CAP);
        assert!(
            text.chars().count() <= REPORT_CAP,
            "{}",
            text.chars().count()
        );
        assert!(
            text.starts_with(
                "Session \"Write the parser\" finished. It works on plan item parser.\nWrote the parser."
            ),
            "{text}"
        );
        assert!(text.contains("\nFiles: src/f0.rs, "), "{text}");
        assert!(text.contains("(+4 more)."), "{text}");
        assert!(text.contains("\nChecks: cargo test passed."), "{text}");
        assert!(text.contains("\nBranch: plan/parser."), "{text}");
        assert!(text.ends_with("\nMore: session.read c1"), "{text}");
        let stopped = report(&c, true, REPORT_CAP);
        assert!(stopped.starts_with("Session \"Write the parser\" was stopped."));
        let mut bare = c;
        bare.last_run = None;
        bare.plan_item = None;
        bare.branch.clear();
        assert_eq!(
            report(&bare, false, REPORT_CAP),
            "Session \"Write the parser\" ended.\nMore: session.read c1"
        );
    }
}
