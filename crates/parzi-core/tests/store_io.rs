use parzi_core::store::{Event, SessionStatus, SessionStore};

fn open() -> SessionStore {
    static HOME: std::sync::Once = std::sync::Once::new();
    HOME.call_once(|| {
        let dir = std::env::temp_dir().join(format!("parzi-test-store-io-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
    });
    SessionStore::open().unwrap()
}

fn user(t: &str) -> Event {
    Event::User { text: t.into() }
}

fn events_file(id: &str) -> std::path::PathBuf {
    parzi_core::paths::sessions_dir()
        .unwrap()
        .join(id)
        .join("events.jsonl")
}

#[test]
fn append_through_keeps_reads_in_sync() {
    let store = open();
    let s = store.create("append", "t-append", "lane", "m").unwrap();
    for i in 0..5 {
        store.append(&s.id, &user(&format!("m{i}"))).unwrap();
    }
    let evs = store.events(&s.id).unwrap();
    assert_eq!(evs.len(), 5);
    let raw = std::fs::read_to_string(events_file(&s.id)).unwrap();
    assert_eq!(raw.lines().filter(|l| !l.is_empty()).count(), 5);
    assert!(raw.ends_with('\n'), "every append ends its line");
}

#[test]
fn another_writer_is_picked_up_by_the_tail_read() {
    let a = open();
    let s = a.create("two writers", "t-two", "lane", "m").unwrap();
    a.append(&s.id, &user("from a")).unwrap();
    assert_eq!(a.events(&s.id).unwrap().len(), 1);

    let b = open();
    b.append(&s.id, &user("from b")).unwrap();

    let evs = a.events(&s.id).unwrap();
    assert_eq!(evs.len(), 2, "cached reader must catch up from disk");
    assert!(matches!(&evs[1], Event::User { text } if text == "from b"));
}

#[test]
fn a_bad_line_does_not_brick_the_transcript() {
    use std::io::Write;
    let store = open();
    let s = store.create("torn", "t-torn", "lane", "m").unwrap();
    store.append(&s.id, &user("before")).unwrap();
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(events_file(&s.id))
            .unwrap();
        writeln!(f, "{{\"kind\":\"user\",\"text\":\"tru").unwrap();
        writeln!(f, "{{\"kind\":\"quantum\",\"text\":\"from the future\"}}").unwrap();
    }
    store.append(&s.id, &user("after")).unwrap();

    let evs = store.events(&s.id).unwrap();
    assert_eq!(evs.len(), 2, "readable events survive: {evs:?}");
    assert!(matches!(&evs[1], Event::User { text } if text == "after"));
}

#[test]
fn fork_keeps_lines_this_build_cannot_parse() {
    use std::io::Write;
    let store = open();
    let s = store.create("fork src", "t-fork", "lane", "m").unwrap();
    store.append(&s.id, &user("one")).unwrap();
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(events_file(&s.id))
            .unwrap();
        writeln!(f, "{{\"kind\":\"quantum\",\"text\":\"from the future\"}}").unwrap();
    }
    let forked = store.fork(&s.id, None).unwrap();
    let raw = std::fs::read_to_string(events_file(&forked.id)).unwrap();
    assert!(
        raw.contains("quantum"),
        "an unknown kind must survive a fork: {raw}"
    );
    assert_eq!(store.events(&forked.id).unwrap().len(), 1);
}

#[test]
fn session_md_is_lazy_and_closes_the_widget_fence() {
    let store = open();
    let s = store.create("md", "t-md", "lane", "m").unwrap();
    let md_path = parzi_core::paths::sessions_dir()
        .unwrap()
        .join(&s.id)
        .join("session.md");
    let header_len = std::fs::metadata(&md_path).unwrap().len();

    store
        .append(
            &s.id,
            &Event::Widget {
                fence: "parzi-widget".into(),
                payload: serde_json::json!({"widget": 1, "type": "markdown", "text": "hi"}),
            },
        )
        .unwrap();
    for i in 0..20 {
        store.append(&s.id, &user(&format!("line {i}"))).unwrap();
    }
    assert_eq!(
        std::fs::metadata(&md_path).unwrap().len(),
        header_len,
        "C-5: appends must not re-render session.md"
    );

    store.set_status(&s.id, SessionStatus::Done).unwrap();
    let md = std::fs::read_to_string(&md_path).unwrap();
    assert!(md.contains("line 19"), "run end flushes the human view");
    assert_eq!(
        md.matches("```").count() % 2,
        0,
        "C-5: every fence is closed: {md}"
    );
    let after_widget = md.split("```").nth(2).unwrap_or_default();
    assert!(
        after_widget.contains("line 0"),
        "text after a widget must be outside the fence"
    );
}

#[test]
fn transcript_md_renders_on_read() {
    let store = open();
    let s = store.create("read", "t-read", "lane", "m").unwrap();
    store.append(&s.id, &user("only on read")).unwrap();
    let md = store.transcript_md(&s.id).unwrap();
    assert!(md.contains("only on read"));
    let on_disk = std::fs::read_to_string(
        parzi_core::paths::sessions_dir()
            .unwrap()
            .join(&s.id)
            .join("session.md"),
    )
    .unwrap();
    assert_eq!(on_disk, md, "the read path also refreshes the file");
}

#[test]
fn list_sees_a_title_change_through_the_index() {
    let store = open();
    let s = store.create("before", "t-list", "lane", "m").unwrap();
    let find = |st: &SessionStore| -> String {
        st.list()
            .unwrap()
            .into_iter()
            .find(|m| m.id == s.id)
            .map(|m| m.title)
            .unwrap_or_default()
    };
    assert_eq!(find(&store), "before");
    assert_eq!(find(&store), "before", "second call is served by the index");
    store.set_title(&s.id, "after").unwrap();
    assert_eq!(find(&store), "after", "C-6: a changed session is re-read");

    let bad = store.create("bad", "t-list", "lane", "m").unwrap();
    std::fs::write(
        parzi_core::paths::sessions_dir()
            .unwrap()
            .join(&bad.id)
            .join("meta.json"),
        "{ not json",
    )
    .unwrap();
    assert_eq!(find(&store), "after");
}

#[test]
fn appends_do_not_rewrite_meta_but_list_stays_ordered() {
    let store = open();
    let s = store.create("coalesce", "t-coalesce", "lane", "m").unwrap();
    let meta_path = parzi_core::paths::sessions_dir()
        .unwrap()
        .join(&s.id)
        .join("meta.json");
    let before = std::fs::read_to_string(&meta_path).unwrap();
    store.append(&s.id, &user("one")).unwrap();
    assert_eq!(
        std::fs::read_to_string(&meta_path).unwrap(),
        before,
        "meta.json is written at turn boundaries, not per event"
    );
    assert!(store.get(&s.id).unwrap().updated > s.updated);
    store.flush(&s.id).unwrap();
    assert_ne!(
        std::fs::read_to_string(&meta_path).unwrap(),
        before,
        "flush pushes the coalesced stamp"
    );
}

#[test]
fn the_event_cache_is_bounded_and_keeps_unflushed_runs() {
    let store = open();
    let live = store.create("live", "t-cap-live", "lane", "m").unwrap();
    store.append(&live.id, &user("half a turn")).unwrap();
    let live_updated = store.get(&live.id).unwrap().updated;

    let mut ids = vec![];
    for i in 0..40 {
        let s = store
            .create(&format!("browse {i}"), &format!("t-cap-{i}"), "lane", "m")
            .unwrap();
        store.append(&s.id, &user(&format!("m{i}"))).unwrap();
        store.flush(&s.id).unwrap();
        store.events(&s.id).unwrap();
        ids.push(s.id);
    }
    assert!(
        store.cached_sessions() <= 16,
        "cache grew to {} sessions",
        store.cached_sessions()
    );
    let first = store.events(&ids[0]).unwrap();
    assert_eq!(first.len(), 1);
    assert!(matches!(&first[0], Event::User { text } if text == "m0"));
    assert_eq!(store.get(&live.id).unwrap().updated, live_updated);
}

#[test]
fn a_torn_last_line_does_not_swallow_the_next_append() {
    use std::io::Write;
    let store = open();
    let s = store
        .create("torn tail", "t-torn-tail", "lane", "m")
        .unwrap();
    store.append(&s.id, &user("one")).unwrap();
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(events_file(&s.id))
            .unwrap();
        write!(f, "{{\"kind\":\"user\",\"text\":\"cut off").unwrap();
    }
    store.append(&s.id, &user("two")).unwrap();
    let texts: Vec<String> = store
        .events(&s.id)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e {
            Event::User { text } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["one", "two"]);
    let fresh = open();
    assert_eq!(fresh.events(&s.id).unwrap().len(), 2, "a cold read agrees");
}

#[test]
fn fork_keeps_the_working_folder_and_settings() {
    let store = open();
    let s = store
        .create("src", "t-fork-cwd", "lane-x", "model-y")
        .unwrap();
    store.set_cwd(&s.id, "/work/repo").unwrap();
    store.set_context(&s.id, 1200, 200_000).unwrap();
    store.add_usage(&s.id, 10, 20, 0.5).unwrap();
    store.append(&s.id, &user("hi")).unwrap();
    let f = store.fork(&s.id, None).unwrap();
    assert_eq!(f.cwd, "/work/repo");
    assert_eq!(
        (f.project.as_str(), f.lane.as_str(), f.model.as_str()),
        ("t-fork-cwd", "lane-x", "model-y")
    );
    assert_eq!(f.context_limit, 200_000);
    assert_eq!(
        (f.tokens_in, f.tokens_out),
        (0, 0),
        "spend stays with the source"
    );
    assert_eq!(f.parent_id, None);
    assert_eq!(store.events(&f.id).unwrap().len(), 1);
}

#[test]
fn concurrent_setters_keep_every_update() {
    let store = open();
    let s = store.create("race", "t-race", "lane", "m").unwrap();
    std::thread::scope(|scope| {
        for t in 0..4 {
            let (store, id) = (store.clone(), s.id.clone());
            scope.spawn(move || {
                for i in 0..25 {
                    store.add_usage(&id, 1, 2, 0.0).unwrap();
                    store.set_title(&id, &format!("title {t}-{i}")).unwrap();
                }
            });
        }
    });
    let meta = store.get(&s.id).unwrap();
    assert_eq!((meta.tokens_in, meta.tokens_out), (100, 200));
    assert!(meta.title.starts_with("title "));
}

#[test]
fn counts_and_tails_without_copying_the_transcript() {
    let store = open();
    let s = store.create("tail", "t-tail", "lane", "m").unwrap();
    assert_eq!(store.event_count(&s.id).unwrap(), 0);
    for i in 0..5 {
        store.append(&s.id, &user(&format!("m{i}"))).unwrap();
    }
    assert_eq!(store.event_count(&s.id).unwrap(), 5);
    let tail = store.events_from(&s.id, 3).unwrap();
    assert_eq!(tail.len(), 2);
    assert!(matches!(&tail[0], Event::User { text } if text == "m3"));
    assert!(store.events_from(&s.id, 9).unwrap().is_empty());
    let other = open();
    other.append(&s.id, &user("m5")).unwrap();
    assert_eq!(store.event_count(&s.id).unwrap(), 6, "sees another writer");
    assert!(store.event_count("not-a-session").is_err());
}

#[test]
fn a_corrupt_meta_is_skipped_by_list_until_healed() {
    let store = open();
    let bad = store.create("corrupt", "t-corrupt", "lane", "m").unwrap();
    let path = parzi_core::paths::sessions_dir()
        .unwrap()
        .join(&bad.id)
        .join("meta.json");
    std::fs::write(&path, "{ not json").unwrap();
    let has = |st: &SessionStore| st.list().unwrap().iter().any(|m| m.id == bad.id);
    assert!(!has(&store));
    assert!(!has(&store), "a cached failure stays skipped");
    std::fs::remove_file(&path).unwrap();
    let ok = serde_json::json!({
        "id": bad.id, "title": "healed",
        "created": "2026-01-01T00:00:00Z", "updated": "2026-01-01T00:00:00Z",
    });
    parzi_core::atomic_write(&path, ok.to_string().as_bytes()).unwrap();
    assert!(has(&open()), "a healed meta.json lists again");
}
