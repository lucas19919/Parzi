//! The desk's side of Parzi Work: the Work view, the session strip and Home
//! read this PC's ledger here, and every ledger change reaches the window
//! as `parzi://work`.

use parzi_core::work::view::{self, Engine, GoalDetail, Lens, ServerLink, WorkView};
use parzi_core::work::{now_ms, WorkLog, Writer};
use tauri::{AppHandle, Emitter, State};

use crate::remote::RemoteState;

/// Every append (a run's start or end, a goal, a step, an ask) reaches the
/// UI; it coalesces them.
pub(crate) fn watch(app: &AppHandle) {
    match WorkLog::shared() {
        Ok(log) => {
            let app = app.clone();
            log.watch(move |change| {
                let _ = app.emit("parzi://work", change);
            });
        }
        Err(e) => tracing::warn!("work ledger not watched: {e}"),
    }
}

/// This PC's session ids are shown as they are.
fn raw(id: &str) -> String {
    id.to_string()
}

fn here(now: u64) -> Lens<'static> {
    Lens {
        engine: Engine::here(),
        tag: &raw,
        owned: true,
        now,
    }
}

fn work_of(w: &Writer, server: ServerLink, now: u64) -> WorkView {
    let lens = here(now);
    WorkView {
        goals: view::goal_views(w, &lens),
        asks: view::open_asks(w, &lens),
        // The digest ("since you left") comes with Later and review.
        digest: None,
        server,
    }
}

fn shared() -> Result<std::sync::Arc<WorkLog>, String> {
    WorkLog::shared().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn work_view(far: State<'_, RemoteState>) -> Result<WorkView, String> {
    // The server's goals join the view once it speaks the work ops.
    let server = ServerLink {
        linked: crate::remote::linked(&far),
        ops: false,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let log = shared()?;
        Ok(log.read(|w| work_of(w, server, now_ms())))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn work_goal(id: String) -> Result<GoalDetail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let log = shared()?;
        let id = id.trim().to_string();
        log.read(|w| view::goal_detail(w, &id, &here(now_ms())))
            .ok_or_else(|| format!("no goal `{id}`"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use parzi_core::work::{Entry, RunCause, RunEndKind, RunOutcome, WorkEvent};

    fn sample() -> Writer {
        let mut w = Writer::new("pc-ab12");
        let evs = [
            WorkEvent::RunStarted {
                run: "r1".into(),
                session: "s1".into(),
                model: "claude/opus".into(),
                lane: "build".into(),
                cwd: String::new(),
                cause: RunCause::Send,
            },
            WorkEvent::GoalCreated {
                goal: "k7m2q".into(),
                title: "Fix the omnibar autosize".into(),
                project: Some("parzi".into()),
                root: Some("s1".into()),
                prompt: "fix the omnibar autosize".into(),
                origin: RunCause::Send,
                later: None,
                continues: None,
            },
            WorkEvent::RunEnded {
                run: "r1".into(),
                outcome: RunOutcome {
                    summary: "Fixed it.".into(),
                    ..RunOutcome::bare(RunEndKind::Completed)
                },
            },
        ];
        for (i, ev) in evs.into_iter().enumerate() {
            w.apply(&Entry {
                w: "pc-ab12".into(),
                seq: i as u64 + 1,
                at: 1_000 + i as u64,
                ev,
            });
        }
        w
    }

    #[test]
    fn the_view_is_this_pc_with_the_server_link() {
        let server = ServerLink {
            linked: true,
            ops: false,
        };
        let v = serde_json::to_value(work_of(&sample(), server, 2_000)).unwrap();
        assert_eq!(
            v["server"],
            serde_json::json!({"linked": true, "ops": false})
        );
        assert_eq!(v["digest"], serde_json::Value::Null);
        assert_eq!(v["asks"], serde_json::json!([]));
        let g = &v["goals"][0];
        assert_eq!(g["id"], "k7m2q");
        assert_eq!(g["status"], "done");
        assert_eq!(g["root"], "s1", "this PC's ids are untagged");
        assert_eq!(g["owned"], true);
        assert_eq!(g["engine"]["kind"], "here");
        assert_eq!(g["engine"]["name"], "This PC");
        assert_eq!(g["last"]["summary"], "Fixed it.");
        assert_eq!(g["last"]["end"], "completed");
    }

    #[test]
    fn the_event_payload_names_goals_and_settled_ones() {
        let change = parzi_core::work::Change {
            goals: vec!["k7m2q".into()],
            settled: vec![parzi_core::work::Settled {
                goal: "k7m2q".into(),
                title: "Fix it".into(),
                status: parzi_core::work::GoalStatus::NeedsYou,
            }],
        };
        assert_eq!(
            serde_json::to_value(&change).unwrap(),
            serde_json::json!({
                "goals": ["k7m2q"],
                "settled": [{"goal": "k7m2q", "title": "Fix it", "status": "needs_you"}],
            })
        );
    }
}
