use parzi_core::theme::Theme;

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
fn widget_shapes_reject_malformed_payloads() {
    use parzi_core::widgets::validate_widget as v;
    assert!(v(&serde_json::json!({"widget": 1, "type": "progress", "value": "lots"})).is_err());
    assert!(v(&serde_json::json!({"widget": 1, "type": "progress", "value": 0.7})).is_ok());
    assert!(v(&serde_json::json!({"widget": 1, "type": "list", "items": 5})).is_err());
    assert!(v(&serde_json::json!({"widget": 1, "type": "list", "items": ["a"]})).is_ok());
    assert!(v(&serde_json::json!({"widget": 1, "type": "table", "rows": {}})).is_err());
    assert!(v(&serde_json::json!({"widget": 1, "type": "table", "columns": "x"})).is_err());
    assert!(v(&serde_json::json!({"widget": 1, "type": "chart-bar", "points": [1, "x"]})).is_err());
    assert!(
        v(&serde_json::json!({"widget": 1, "type": "chart-bar", "points": [1, 2.5, null]})).is_ok()
    );
    assert!(
        v(&serde_json::json!({"widget": 1, "type": "kanban", "columns": [{"cards": 1}]})).is_err()
    );
    assert!(v(&serde_json::json!({"widget": 1, "type": "kanban", "columns": [{"title": "t", "cards": ["a"]}]})).is_ok());
    let huge = "x".repeat(70_000);
    assert!(v(&serde_json::json!({"widget": 1, "type": "markdown", "text": huge})).is_err());
}

#[test]
fn widget_series_validate_per_series() {
    use parzi_core::widgets::validate_widget as v;
    let multi = serde_json::json!({"widget": 1, "type": "chart-line",
        "series": [{"name": "a", "points": [1, 2]}, {"name": "b", "points": [3]}]});
    assert!(v(&multi).is_ok());
    let noshape = serde_json::json!({"widget": 1, "type": "chart-bar",
        "series": [{"name": "a"}]});
    assert!(v(&noshape).is_err());
    let badnum = serde_json::json!({"widget": 1, "type": "chart-bar",
        "series": [{"points": [1, "x"]}]});
    assert!(v(&badnum).is_err());
    let many: Vec<_> = (0..9)
        .map(|i| serde_json::json!({"name": i, "points": [1]}))
        .collect();
    assert!(v(&serde_json::json!({"widget": 1, "type": "chart-line", "series": many})).is_err());
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
fn session_meta_parent_id_defaults_to_none_for_legacy_files() {
    use parzi_core::store::SessionMeta;
    let legacy = serde_json::json!({
        "id": "abc",
        "title": "old",
        "created": "2026-01-01T00:00:00Z",
        "updated": "2026-01-01T00:00:00Z",
    });
    let m: SessionMeta = serde_json::from_value(legacy).unwrap();
    assert_eq!(m.parent_id, None);
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
fn subsession_hierarchy_lists_children() {
    use parzi_core::store::SessionStore;
    let dir = std::env::temp_dir().join(format!("parzi-test-logic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("PARZI_HOME", &dir);
    let store = SessionStore::open().unwrap();
    let parent = store.create("team-parent", "t", "", "m").unwrap();
    assert_eq!(parent.parent_id, None);
    let child = store
        .create_with_parent("team-child", "t", "", "m", Some(&parent.id))
        .unwrap();
    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
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
        context_tokens: 0,
        context_limit: 0,
        cwd: "".into(),
        created: Utc::now(),
        updated: Utc::now(),
    };
    let all = vec![
        mk("p", None, SessionStatus::Done),
        mk("c", Some("p"), SessionStatus::Idle),
        mk("g", Some("c"), SessionStatus::Active),
        mk("solo", None, SessionStatus::Done),
        mk("keep", None, SessionStatus::Idle),
        mk("keepkid", Some("keep"), SessionStatus::Idle),
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
fn theme_emits_css_vars_with_a_neutral_default() {
    let t = Theme::default();
    assert_eq!(t.background.image, "");
    let css = t.to_css_vars();
    assert!(css.contains("--accent:#E6E8EE"));
    assert!(css.contains("--bg-blur:"));
}

#[test]
fn widget_histogram_and_scatter_validate() {
    use parzi_core::widgets::validate_widget as v;
    let h = serde_json::json!({"widget": 1, "type": "histogram", "title": "lat", "values": [1, 2, 2, 3], "bins": 2});
    assert!(v(&h).is_ok());
    let bad = serde_json::json!({"widget": 1, "type": "histogram", "values": ["a"]});
    assert!(v(&bad).is_err());
    let bins = serde_json::json!({"widget": 1, "type": "histogram", "values": [1], "bins": 99});
    assert!(v(&bins).is_err());
    let s = serde_json::json!({"widget": 1, "type": "scatter", "points": [[1, 2], [3, 4]]});
    assert!(v(&s).is_ok());
    let flat = serde_json::json!({"widget": 1, "type": "scatter", "points": [1, 2]});
    assert!(v(&flat).is_err());
    let nan = serde_json::json!({"widget": 1, "type": "scatter", "points": [[1, 2], [3]]});
    assert!(v(&nan).is_err());
}
