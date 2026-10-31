mod common;

use std::sync::{Arc, Mutex};

use common::*;
use parzi_core::store::Event;
use parzi_providers::TurnEnd;
use parzi_runtime::handler::RunEvent;
use serde_json::{json, Value};

const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

type Answers = Arc<Mutex<Vec<(bool, String)>>>;

/// Runs one agent turn that makes these publishes in order, and returns each answer.
async fn publish(
    calls: Vec<Value>,
) -> (
    String,
    parzi_core::store::SessionStore,
    Vec<(bool, String)>,
    tokio::sync::mpsc::UnboundedReceiver<RunEvent>,
) {
    home("artifacts");
    let answers: Answers = Arc::default();
    let seen = answers.clone();
    let fake = Fake::new(
        "claude",
        script(move |a: Agent| {
            let (calls, seen) = (calls.clone(), seen.clone());
            async move {
                for args in calls {
                    let got = a.parzi("ui.show_artifact", args).await;
                    seen.lock().unwrap().push(got);
                }
                Ok(TurnEnd::Completed)
            }
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, rx) = orch
        .spawn(
            "artifact-tests",
            "claude",
            "publish",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let got = answers.lock().unwrap().clone();
    (meta.id, store, got, rx)
}

fn published(store: &parzi_core::store::SessionStore, id: &str) -> Vec<(String, u32, Value)> {
    events(store, id)
        .into_iter()
        .filter_map(|e| match e {
            Event::Artifact {
                id,
                version,
                payload,
                ..
            } => Some((id, version, payload)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn id_and_format_are_optional_as_the_schema_says() {
    let (id, store, answers, _rx) = publish(vec![
        json!({"title": "My Report", "kind": "markdown", "content": "# Hi"}),
    ])
    .await;
    assert!(answers[0].0, "{answers:?}");
    assert_eq!(published(&store, &id)[0].0, "my-report");
}

#[tokio::test]
async fn versions_count_what_changed_and_a_new_title_is_a_change() {
    let doc = |title: &str, content: &str| json!({"id": "doc", "title": title, "kind": "markdown", "content": content});
    let (id, store, answers, _rx) = publish(vec![
        doc("One", "a"),
        doc("One", "a"),
        doc("Two", "a"),
        doc("Two", "b"),
    ])
    .await;
    assert!(answers[1].1.contains("unchanged"), "{answers:?}");
    let versions: Vec<u32> = published(&store, &id).iter().map(|a| a.1).collect();
    assert_eq!(versions, [1, 2, 3]);
}

#[tokio::test]
async fn publishing_tells_the_window_at_once() {
    let (_, _, _, mut rx) = publish(vec![
        json!({"id": "doc", "kind": "markdown", "content": "a"}),
    ])
    .await;
    let mut told = vec![];
    while let Ok(ev) = rx.try_recv() {
        if let RunEvent::Artifact { id, version, .. } = ev {
            told.push((id, version));
        }
    }
    assert_eq!(told, [("doc".to_string(), 1)]);
}

#[tokio::test]
async fn a_picture_outlives_the_file_it_came_from() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("shot.png");
    let again = dir.path().join("same-shot.png");
    std::fs::write(&first, PIXEL).unwrap();
    std::fs::write(&again, PIXEL).unwrap();
    let pic = |p: &std::path::Path| json!({"id": "shot", "kind": "image", "content": p.to_string_lossy()});
    let (id, store, answers, _rx) = publish(vec![pic(&first), pic(&again)]).await;
    assert!(answers[0].0, "{answers:?}");
    assert!(
        answers[1].1.contains("unchanged"),
        "the same picture from another path is the same picture: {answers:?}"
    );
    std::fs::remove_file(&first).unwrap();
    std::fs::remove_file(&again).unwrap();
    let kept = published(&store, &id)[0].2["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(std::path::Path::new(&kept).starts_with(store.artifact_dir(&id)));
    assert_eq!(std::fs::read(&kept).unwrap(), PIXEL);
}

#[tokio::test]
async fn a_picture_that_is_not_there_or_not_a_picture_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("note.png");
    std::fs::write(&text, "not a picture").unwrap();
    let (id, store, answers, _rx) = publish(vec![
        json!({"id": "a", "kind": "image", "content": dir.path().join("gone.png").to_string_lossy()}),
        json!({"id": "b", "kind": "image", "content": text.to_string_lossy()}),
    ])
    .await;
    assert!(!answers[0].0 && !answers[1].0, "{answers:?}");
    assert!(published(&store, &id).is_empty());
}

#[tokio::test]
async fn parallel_publishes_of_one_id_get_their_own_versions() {
    home("artifacts");
    let fake = Fake::new(
        "claude",
        script(|a: Agent| async move {
            let one = json!({"id": "doc", "kind": "markdown", "content": "one"});
            let two = json!({"id": "doc", "kind": "markdown", "content": "two"});
            let _ = tokio::join!(
                a.parzi("ui.show_artifact", one),
                a.parzi("ui.show_artifact", two)
            );
            Ok(TurnEnd::Completed)
        }),
    );
    let (orch, store) = orch(std::slice::from_ref(&fake));
    let (meta, _rx) = orch
        .spawn(
            "artifact-tests",
            "claude",
            "publish",
            None,
            "",
            "low",
            vec![],
            None,
        )
        .await
        .unwrap();
    settle(&store, &meta.id).await;
    let mut versions: Vec<u32> = published(&store, &meta.id).iter().map(|a| a.1).collect();
    versions.sort_unstable();
    assert_eq!(versions, [1, 2]);
}
