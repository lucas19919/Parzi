//! Store I/O gates (E5, C-2, C-5, C-6): append-through reads, tolerant
//! parsing, the lazily rendered `session.md` and the `list()` index.

use parzi_core::store::{Event, SessionStatus, SessionStore};

/// The store in a home of this test binary's own, so a local `cargo test`
/// never writes sessions into the real `~/.parzi`.
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
    // The file is the truth, not just the cache.
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

    // A second store (think: CLI next to the GUI) appends behind our back.
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
        // A half-written line from an older build, and an event kind this
        // build has never heard of (C-2).
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

    // Run end renders it once.
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

    // A meta.json nobody can parse is logged, not silently dropped, and must
    // not take the rest of the list with it.
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
    // Readers still see the newer stamp.
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
    // One session that stays mid-turn: appended to, never flushed.
    let live = store.create("live", "t-cap-live", "lane", "m").unwrap();
    store.append(&live.id, &user("half a turn")).unwrap();
    let live_updated = store.get(&live.id).unwrap().updated;

    // Browse far more transcripts than the cache may hold.
    let mut ids = vec![];
    for i in 0..40 {
        let s = store
            .create(&format!("browse {i}"), &format!("t-cap-{i}"), "lane", "m")
            .unwrap();
        store.append(&s.id, &user(&format!("m{i}"))).unwrap();
        store.flush(&s.id).unwrap(); // turn boundary: nothing left in memory
        store.events(&s.id).unwrap();
        ids.push(s.id);
    }
    assert!(
        store.cached_sessions() <= 16,
        "cache grew to {} sessions",
        store.cached_sessions()
    );
    // An evicted transcript re-reads correctly.
    let first = store.events(&ids[0]).unwrap();
    assert_eq!(first.len(), 1);
    assert!(matches!(&first[0], Event::User { text } if text == "m0"));
    // The unflushed stamp of the live run survived the eviction pressure.
    assert_eq!(store.get(&live.id).unwrap().updated, live_updated);
}
