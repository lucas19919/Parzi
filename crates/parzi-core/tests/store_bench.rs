//! E5 benchmark: cost of one session's transcript, appended and read back.
//!
//! Ignored by default (it writes tens of thousands of files' worth of bytes);
//! run it by hand:
//!
//! ```text
//! cargo test -p parzi-core --test store_bench -- --ignored --nocapture
//! PARZI_BENCH_EVENTS=2000 cargo test ... (smaller run)
//! ```
//!
//! It reports wall time, the store's own filesystem call counts and the bytes
//! the store put on disk, so "writes per append" is a measured number.

use parzi_core::store::{Event, SessionStore};

#[test]
#[ignore = "benchmark: run with --ignored --nocapture"]
fn append_and_read_10k_events() {
    let home = tempfile::tempdir().expect("tempdir");
    std::env::set_var("PARZI_HOME", home.path());

    let n: usize = std::env::var("PARZI_BENCH_EVENTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    // One `assemble()` per turn: a run re-reads the transcript this often.
    let turn_every = 8;

    let store = SessionStore::open().expect("store");
    let s = store
        .create("bench", "proj", "lane", "model")
        .expect("create");

    let before = parzi_core::store::io_counts();
    let t0 = std::time::Instant::now();
    for i in 0..n {
        let e = if i % 3 == 0 {
            Event::User {
                text: format!("message number {i} with a little body text"),
            }
        } else if i % 3 == 1 {
            Event::Assistant {
                text: format!("reply number {i} with a little body text"),
                done: true,
            }
        } else {
            Event::ToolResult {
                id: format!("t{i}"),
                name: "fs.read".into(),
                ok: true,
                output: format!("output of call {i}"),
                ms: 3,
            }
        };
        store.append(&s.id, &e).expect("append");
        if i % turn_every == turn_every - 1 {
            let evs = store.events(&s.id).expect("events");
            assert_eq!(evs.len(), i + 1);
        }
    }
    let append_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = std::time::Instant::now();
    let read = store.events(&s.id).expect("read back");
    let read_ms = t1.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(read.len(), n);

    // Run end: the one place `session.md` and `meta.json` are written now.
    let t2 = std::time::Instant::now();
    store
        .set_status(&s.id, parzi_core::store::SessionStatus::Done)
        .expect("finish");
    let finish_ms = t2.elapsed().as_secs_f64() * 1000.0;

    let after = parzi_core::store::io_counts();
    let (reads, writes, stats) = (after.0 - before.0, after.1 - before.1, after.2 - before.2);

    let dir = home.path().join("sessions").join(&s.id);
    let size_of = |name: &str| {
        std::fs::metadata(dir.join(name))
            .map(|m| m.len())
            .unwrap_or(0)
    };

    println!("--- store bench: {n} events, assemble every {turn_every} ---");
    println!(
        "append loop:      {append_ms:.0} ms ({:.3} ms/event)",
        append_ms / n as f64
    );
    println!("read back all:    {read_ms:.0} ms");
    println!("run end (flush):  {finish_ms:.0} ms");
    println!(
        "file writes:      {writes} ({:.2} per append)",
        writes as f64 / n as f64
    );
    println!(
        "file reads:       {reads} ({:.2} per append)",
        reads as f64 / n as f64
    );
    println!("stats:            {stats}");
    println!(
        "on disk:          events.jsonl {} KB, session.md {} KB, meta.json {} B",
        size_of("events.jsonl") / 1024,
        size_of("session.md") / 1024,
        size_of("meta.json")
    );
}
