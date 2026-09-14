//! Pure-logic gates: context assembly, compaction, widgets, theme vars.

use parzi_core::context::{AttachedFile, ContextBuilder, Role};
use parzi_core::store::Event;
use parzi_core::theme::Theme;

fn user(t: &str) -> Event {
    Event::User { text: t.into() }
}

#[test]
fn assembles_newest_first_under_budget() {
    let history: Vec<Event> = (0..20)
        .map(|i| user(&format!("message number {i}")))
        .collect();
    let b = ContextBuilder {
        system_parts: vec!["sys".into()],
        history,
        files: vec![],
    };
    let ctx = b.assemble(60);
    // Tiny budget: only the newest messages survive, order preserved.
    assert!(!ctx.messages.is_empty());
    let texts: Vec<&str> = ctx.messages.iter().map(|m| m.content.as_str()).collect();
    let last = texts.last().unwrap();
    assert!(
        last.contains("message number 19"),
        "newest must survive: {last}"
    );
    assert!(ctx.estimated_tokens <= 60 + 40); // estimate granularity, never wild
}

#[test]
fn files_cap_and_truncate() {
    let big = "x".repeat(100_000);
    let b = ContextBuilder {
        system_parts: vec![],
        history: vec![],
        files: vec![AttachedFile::text("a.rs".into(), big)],
    };
    let ctx = b.assemble(100);
    assert!(ctx.messages.iter().any(|m| m.role == Role::System));
}

#[test]
fn compact_keeps_tail_and_folds_head() {
    let history: Vec<Event> = (0..30).map(|i| user(&format!("m{i}"))).collect();
    let out = ContextBuilder::compact(&history, 20);
    assert_eq!(out.len(), 21);
    assert!(matches!(out[0], Event::Checkpoint { .. }));
}

#[test]
fn widget_validation_fails_safe() {
    let good = serde_json::json!({"widget": 1, "type": "progress", "title": "t", "value": 0.5});
    assert!(parzi_core::widgets::validate_widget(&good).is_ok());
    let bad_type = serde_json::json!({"widget": 1, "type": "nuke"});
    assert!(parzi_core::widgets::validate_widget(&bad_type).is_err());
    let bad_ver = serde_json::json!({"widget": 2, "type": "progress"});
    assert!(parzi_core::widgets::validate_widget(&bad_ver).is_err());
    let big_rows: Vec<_> = (0..60).map(|i| serde_json::json!([i])).collect();
    let big = serde_json::json!({"widget": 1, "type": "table", "rows": big_rows});
    assert!(parzi_core::widgets::validate_widget(&big).is_err());
}

#[test]
fn diagram_caps_hold() {
    let nodes: Vec<_> = (0..201)
        .map(|i| serde_json::json!({"id": format!("n{i}")}))
        .collect();
    let d = serde_json::json!({"diagram": 1, "nodes": nodes, "edges": []});
    assert!(parzi_core::widgets::validate_diagram(&d).is_err());
    let ok = serde_json::json!({
        "diagram": 1, "nodes": [{"id": "a"}, {"id": "b"}],
        "edges": [{"from": "a", "to": "b"}],
    });
    assert!(parzi_core::widgets::validate_diagram(&ok).is_ok());
}

#[test]
fn widget_markdown_validates_and_chart_caps_hold() {
    let md = serde_json::json!({"widget": 1, "type": "markdown", "text": "hello"});
    assert!(parzi_core::widgets::validate_widget(&md).is_ok());
    let empty = serde_json::json!({"widget": 1, "type": "markdown", "text": "  "});
    assert!(parzi_core::widgets::validate_widget(&empty).is_err());
    let pts: Vec<_> = (0..201).map(|i| serde_json::json!(i)).collect();
    let big = serde_json::json!({"widget": 1, "type": "chart-line", "points": pts});
    assert!(parzi_core::widgets::validate_widget(&big).is_err());
}

#[test]
fn artifact_validation_versions_and_dedups() {
    use parzi_core::artifacts::{is_same_content, next_version, slugify_id, validate_artifact};
    assert_eq!(slugify_id("Hello World!"), "hello-world");
    assert!(!slugify_id("!!!").is_empty());
    let good = serde_json::json!({
        "artifact": 1, "id": "auth-hook", "title": "Auth",
        "kind": "code", "language": "rust", "content": "fn main() {}",
    });
    let a = validate_artifact(&good).unwrap();
    assert_eq!(a.id, "auth-hook");
    assert_eq!(a.version, 1);
    let bad_kind = serde_json::json!({"artifact": 1, "id": "x", "kind": "nuke", "content": "hi"});
    assert!(validate_artifact(&bad_kind).is_err());
    let bad_lang = serde_json::json!({"artifact": 1, "id": "x", "kind": "code", "language": "cobol-x", "content": "hi"});
    assert!(validate_artifact(&bad_lang).is_err());
    let v2 = next_version("auth-hook", std::slice::from_ref(&a));
    assert_eq!(v2, 2);
    assert!(is_same_content(
        "auth-hook",
        "fn main() {}",
        std::slice::from_ref(&a)
    ));
    assert!(!is_same_content(
        "auth-hook",
        "different",
        std::slice::from_ref(&a)
    ));
}

#[test]
fn legacy_tasks_migrate_to_lanes() {
    use parzi_core::lanes::migrate_tasks_to_lanes;
    // Missing project migrates zero, never errors.
    let n = migrate_tasks_to_lanes("no-such-project-xyz").unwrap();
    assert_eq!(n, 0);
}

#[test]
fn session_meta_parent_id_defaults_to_none_for_legacy_files() {
    use parzi_core::store::SessionMeta;
    // Legacy meta.json without `parent_id` must still parse (backwards compat).
    let legacy = serde_json::json!({
        "id": "abc",
        "title": "old",
        "created": "2026-01-01T00:00:00Z",
        "updated": "2026-01-01T00:00:00Z",
    });
    let m: SessionMeta = serde_json::from_value(legacy).unwrap();
    assert_eq!(m.parent_id, None);
    // Round-trips with and without a parent.
    let with_parent = serde_json::json!({
        "id": "child",
        "title": "sub",
        "parent_id": "abc",
        "created": "2026-01-01T00:00:00Z",
        "updated": "2026-01-01T00:00:00Z",
    });
    let c: SessionMeta = serde_json::from_value(with_parent).unwrap();
    assert_eq!(c.parent_id.as_deref(), Some("abc"));
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v["parent_id"], "abc");
    let v0 = serde_json::to_value(&m).unwrap();
    assert!(v0.get("parent_id").is_none(), "None must be skipped");
}

#[test]
fn subsession_hierarchy_lists_and_reparents() {
    use parzi_core::store::SessionStore;
    let store = SessionStore::open().unwrap();
    let parent = store.create("team-parent", "t", "", "m").unwrap();
    assert_eq!(parent.parent_id, None);
    let child = store
        .create_with_parent("team-child", "t", "", "m", Some(&parent.id))
        .unwrap();
    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
    // Arbitrary depth: grandchild under the child.
    let grand = store
        .create_with_parent("team-grand", "t", "", "m", Some(&child.id))
        .unwrap();
    let kids = store.list_children(&parent.id).unwrap();
    assert!(kids.iter().any(|m| m.id == child.id));
    assert!(store
        .list_children(&child.id)
        .unwrap()
        .iter()
        .any(|m| m.id == grand.id));
    // Detach back to top-level.
    store.set_parent(&child.id, None).unwrap();
    assert!(store.list_children(&parent.id).unwrap().is_empty());
    // Guards: no self-parenting, no linking to missing sessions.
    assert!(store.set_parent(&parent.id, Some(&parent.id)).is_err());
    assert!(store
        .set_parent(&parent.id, Some("missing-session"))
        .is_err());
    if let Ok(dir) = parzi_core::paths::sessions_dir() {
        for id in [&parent.id, &child.id, &grand.id] {
            let _ = std::fs::remove_dir_all(dir.join(id));
        }
    }
}

#[test]
fn purge_cascade_kills_descendants_at_any_depth() {
    use chrono::Utc;
    use parzi_core::store::{cascade_kill_ids, SessionMeta, SessionStatus};
    let mk = |id: &str, parent: Option<&str>, status: SessionStatus| SessionMeta {
        id: id.into(),
        title: id.into(),
        pinned: false,
        project: "t".into(),
        lane: "".into(),
        model: "m".into(),
        parent_id: parent.map(|s| s.into()),
        status,
        tokens_in: 0,
        tokens_out: 0,
        cost_usd: 0.0,
        cwd: "".into(),
        created: Utc::now(),
        updated: Utc::now(),
    };
    let all = vec![
        mk("p", None, SessionStatus::Done),        // finished parent
        mk("c", Some("p"), SessionStatus::Idle),   // live child: still purged
        mk("g", Some("c"), SessionStatus::Active), // grandchild: still purged
        mk("solo", None, SessionStatus::Done),     // unrelated finished: purged
        mk("keep", None, SessionStatus::Idle),     // unrelated live: kept
        mk("keepkid", Some("keep"), SessionStatus::Idle), // child of live: kept
    ];
    let kill = cascade_kill_ids(&all);
    for id in ["p", "c", "g", "solo"] {
        assert!(kill.contains(id), "{id} should be purged");
    }
    for id in ["keep", "keepkid"] {
        assert!(!kill.contains(id), "{id} should survive");
    }
}

#[test]
fn theme_emits_css_vars_with_asuka_default() {
    let t = Theme::default();
    assert_eq!(t.background.image, "backgrounds/asuka.png");
    let css = t.to_css_vars();
    assert!(css.contains("--parzi-accent:#7C8CFF"));
    assert!(css.contains("--parzi-glass-blur:18px"));
}
