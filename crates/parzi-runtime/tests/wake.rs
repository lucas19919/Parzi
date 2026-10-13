//! A session drives the sessions it starts in the background, and Parzi
//! wakes it: each reports back when its run ends, built from its
//! `last_run`, and an idle parent runs again on the reports, once for
//! children ending together, with its own posture. No wake for a stopped
//! parent or a child the person stopped. (The hourly cap is a unit test.)

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use parzi_core::config::ParziConfig;
use parzi_core::context::InterSessionMessage;
use parzi_core::store::{Event, SessionStatus, SessionStore};
use parzi_providers::{PermissionDecision, TurnEnd};
use parzi_runtime::Orchestrator;
use serde_json::json;
use tokio::sync::Semaphore;

/// What a woken parent saw: its prompt, its effort, and whether a file
/// write passed the gate without anyone to ask.
#[derive(Debug, Clone)]
struct Woke {
    prompt: String,
    effort: Option<String>,
    write: PermissionDecision,
}

struct Rig {
    orch: Arc<Orchestrator>,
    store: SessionStore,
    release: Arc<Semaphore>,
    woke: Arc<Mutex<Vec<Woke>>>,
    lead: String,
}

/// A parent that starts `n` children in the background; the children
/// hold until `release` lets them finish. Lanes ask by default and nobody
/// answers, so only a restored Full posture lets a write through.
async fn rig(n: usize) -> Rig {
    home("wake");
    let release = Arc::new(Semaphore::new(0));
    let woke: Arc<Mutex<Vec<Woke>>> = Arc::default();
    let (gate, log) = (release.clone(), woke.clone());
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let (gate, log) = (gate.clone(), log.clone());
            async move {
                let prompt = a.spec.prompt.clone();
                if prompt.contains("<parzi:inter") {
                    let write = a
                        .ask("Write", json!({"file_path": "out.txt"}), &["out.txt"])
                        .await;
                    log.lock().unwrap().push(Woke {
                        prompt,
                        effort: a.spec.effort.clone(),
                        write,
                    });
                    a.say("collected the parts");
                    return Ok(TurnEnd::Completed);
                }
                if prompt.contains("do part") {
                    tokio::select! {
                        p = gate.acquire() => p.unwrap().forget(),
                        () = a.cancel.cancelled() => return Ok(TurnEnd::Interrupted),
                    }
                    a.own_tool("1", "Edit", json!({"file_path": "part.rs"}), || async {
                        (true, "ok".to_string())
                    })
                    .await;
                    a.say("part finished");
                    return Ok(TurnEnd::Completed);
                }
                for i in 0..n {
                    let (ok, out) = a
                        .parzi(
                            "session.spawn",
                            json!({"title": format!("Part {i}"), "prompt": format!("do part {i}"), "wait": false}),
                        )
                        .await;
                    assert!(ok, "{out}");
                    assert!(out.contains("do not poll"), "{out}");
                }
                a.say("started the parts");
                Ok(TurnEnd::Completed)
            }
        }),
    );
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "ask".into();
    cfg.orchestrator.max_concurrent = 8;
    cfg.orchestrator.queue_when_busy = true;
    let (orch, store) = orch_with(cfg, &[fake]);
    let (meta, _rx) = orch
        .spawn(
            "wake-tests",
            "build",
            "claude",
            "build it in parts",
            None,
            "",
            "high",
            vec![],
            Some("full".into()),
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let rig = Rig {
        orch,
        store,
        release,
        woke,
        lead: meta.id,
    };
    let kids = rig.children();
    assert_eq!(kids.len(), n);
    for c in kids {
        assert!(wait_status(&rig.store, &c, SessionStatus::Active).await);
    }
    rig
}

impl Rig {
    fn children(&self) -> Vec<String> {
        self.store
            .list_children(&self.lead)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect()
    }

    /// Reports the parent holds, from its transcript.
    fn reports(&self) -> Vec<String> {
        self.store
            .events(&self.lead)
            .unwrap()
            .into_iter()
            .filter_map(|e| match e {
                Event::System { text } => InterSessionMessage::decode(&text).map(|m| m.body),
                _ => None,
            })
            .collect()
    }

    /// Every child's report is in, then the debounce has passed.
    async fn quiet(&self, children: usize) {
        for _ in 0..200 {
            if self.reports().len() >= children {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(self.reports().len(), children, "every child reported");
        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    fn woken(&self) -> Vec<Woke> {
        self.woke.lock().unwrap().clone()
    }
}

#[tokio::test]
async fn children_ending_together_wake_the_parent_once_with_its_posture() {
    let rig = rig(3).await;
    assert!(rig.woken().is_empty(), "nothing ended yet");
    rig.release.add_permits(3);
    rig.quiet(3).await;
    for _ in 0..200 {
        if !rig.woken().is_empty()
            && rig.store.get(&rig.lead).unwrap().status != SessionStatus::Active
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let woke = rig.woken();
    assert_eq!(woke.len(), 1, "one wake for three reports: {woke:?}");
    let w = &woke[0];
    for i in 0..3 {
        assert!(
            w.prompt
                .contains(&format!("Session \"Part {i}\" finished.")),
            "{}",
            w.prompt
        );
    }
    assert_eq!(w.prompt.matches("<parzi:inter").count(), 3, "{}", w.prompt);
    assert!(
        w.prompt.contains("\npart finished"),
        "the summary: {}",
        w.prompt
    );
    assert!(w.prompt.contains("Files: part.rs."), "{}", w.prompt);
    assert!(
        w.prompt.contains("More: session.read_session "),
        "{}",
        w.prompt
    );
    assert_eq!(w.effort.as_deref(), Some("high"), "its effort came back");
    assert_eq!(
        w.write,
        PermissionDecision::Allow,
        "Full access came back: an Ask lane with nobody to ask would deny"
    );
    for body in rig.reports() {
        assert!(body.chars().count() <= 1_500);
    }
    let lead = rig.store.get(&rig.lead).unwrap();
    assert_eq!(
        lead.last_run.map(|o| o.summary).as_deref(),
        Some("collected the parts"),
        "the wake is the parent's latest run"
    );
}

#[tokio::test]
async fn a_stopped_parent_hears_but_sleeps() {
    let rig = rig(1).await;
    rig.orch.kill(&rig.lead).await.unwrap();
    rig.release.add_permits(1);
    rig.quiet(1).await;
    assert!(rig.woken().is_empty());
    assert_eq!(
        rig.store.get(&rig.lead).unwrap().status,
        SessionStatus::Killed
    );
}

#[tokio::test]
async fn a_child_the_person_stopped_wakes_nobody() {
    let rig = rig(1).await;
    let child = rig.children().remove(0);
    rig.orch.kill(&child).await.unwrap();
    rig.quiet(1).await;
    assert!(rig.woken().is_empty());
    assert!(
        rig.reports()[0].contains("was stopped"),
        "{:?}",
        rig.reports()
    );
}
