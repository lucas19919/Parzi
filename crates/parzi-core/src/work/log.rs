//! The work ledger on disk: `~/.parzi/work/<writer>.jsonl`, append-only,
//! one writer per engine. Opening loads `<writer>.snap.json` and replays the
//! lines after it. A torn last line (a crash mid-write) is cut off. Appends
//! take `work/.lock` as well as this process's mutex, so a CLI run on the
//! same machine never hands out a sequence number twice.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::fold::{Touched, Writer};
use super::model::{Entry, GoalStatus, WorkEvent};
use crate::error::{ParziError, Result};

/// A snapshot every this many appends keeps opening fast.
pub const SNAPSHOT_EVERY: u64 = 256;
/// Past this size the log starts over after a snapshot.
const ROTATE_AT: u64 = 8 * 1024 * 1024;

/// What an append changed, as `parzi://work` carries it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub goals: Vec<String>,
    pub settled: Vec<Settled>,
}

/// A goal whose lead run just ended, for a toast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settled {
    pub goal: String,
    pub title: String,
    pub status: GoalStatus,
}

type Watcher = Box<dyn Fn(&Change) + Send + Sync>;

#[derive(Debug, Serialize, Deserialize)]
struct Snapshot {
    w: String,
    seq: u64,
    state: Writer,
}

#[derive(Debug, Serialize, Deserialize)]
struct WriterFile {
    writer: String,
    #[serde(default)]
    device: String,
}

/// A line as written by any version: the event stays raw so a kind this
/// build does not know is skipped, not fatal.
#[derive(Deserialize)]
struct RawEntry {
    w: String,
    seq: u64,
    #[serde(default)]
    at: u64,
    ev: Value,
}

struct Inner {
    state: Writer,
    /// Bytes of the log already folded in.
    offset: u64,
}

pub struct WorkLog {
    dir: PathBuf,
    writer: String,
    /// `work/.lock`, taken (after `inner`) around every look at the file.
    lock: File,
    inner: Mutex<Inner>,
    watchers: Mutex<Vec<Watcher>>,
}

#[must_use]
pub fn now_ms() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(0)
}

/// Holds `work/.lock` until dropped.
struct Held<'a>(&'a File);

impl<'a> Held<'a> {
    fn take(f: &'a File) -> Result<Self> {
        f.lock()?;
        Ok(Self(f))
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

impl WorkLog {
    /// This home's ledger, opened once per `PARZI_HOME`.
    pub fn shared() -> Result<Arc<Self>> {
        static SHARED: Mutex<Option<(PathBuf, Arc<WorkLog>)>> = Mutex::new(None);
        let dir = crate::paths::parzi_dir()?.join("work");
        let mut slot = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, log)) = slot.as_ref() {
            if *at == dir {
                return Ok(log.clone());
            }
        }
        let log = Arc::new(Self::open(&dir)?);
        *slot = Some((dir, log.clone()));
        Ok(log)
    }

    /// The ledger in `dir`, as this machine's writer (made on first use).
    pub fn open(dir: &Path) -> Result<Self> {
        let writer = writer_id(dir)?;
        Self::open_as(dir, &writer)
    }

    /// The ledger in `dir` written as `writer`.
    pub fn open_as(dir: &Path, writer: &str) -> Result<Self> {
        if !crate::device::valid_id(writer) {
            return Err(ParziError::Validation(format!(
                "`{writer}` is not a writer id"
            )));
        }
        std::fs::create_dir_all(dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join(".lock"))?;
        let log = Self {
            dir: dir.to_path_buf(),
            writer: writer.to_string(),
            lock,
            inner: Mutex::new(Inner {
                state: Writer::new(writer),
                offset: 0,
            }),
            watchers: Mutex::new(vec![]),
        };
        {
            let mut inner = log.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let inner = &mut *inner;
            let _held = Held::take(&log.lock)?;
            log.reload(&mut inner.state, &mut inner.offset)?;
        }
        Ok(log)
    }

    #[must_use]
    pub fn writer(&self) -> &str {
        &self.writer
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join(format!("{}.jsonl", self.writer))
    }

    fn snap_path(&self) -> PathBuf {
        self.dir.join(format!("{}.snap.json", self.writer))
    }

    pub fn watch(&self, f: impl Fn(&Change) + Send + Sync + 'static) {
        self.watchers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Box::new(f));
    }

    /// Every append is news (an ask or an orphan run changes the view too);
    /// a look that found nothing new is not.
    fn publish(&self, change: Option<Change>) {
        let Some(change) = change else {
            return;
        };
        for w in self
            .watchers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            w(&change);
        }
    }

    /// State from scratch: the snapshot, then every line after it. Cuts a
    /// torn last line. Call with the file lock held.
    fn reload(&self, state: &mut Writer, offset: &mut u64) -> Result<()> {
        *state = match std::fs::read_to_string(self.snap_path()) {
            Ok(text) => match serde_json::from_str::<Snapshot>(&text) {
                Ok(s) if s.w == self.writer => s.state,
                Ok(_) => Writer::new(&self.writer),
                Err(e) => {
                    tracing::warn!("work snapshot unreadable, replaying the log: {e}");
                    Writer::new(&self.writer)
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Writer::new(&self.writer),
            Err(e) => return Err(e.into()),
        };
        let path = self.log_path();
        let mut bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(e.into()),
        };
        let good = repair_len(&bytes);
        if good < bytes.len() {
            tracing::warn!(
                "work log {}: cut a torn last line ({} bytes)",
                path.display(),
                bytes.len() - good
            );
            let f = OpenOptions::new().write(true).open(&path)?;
            f.set_len(good as u64)?;
            f.sync_all()?;
            bytes.truncate(good);
        }
        self.fold_lines(state, &bytes);
        *offset = bytes.len() as u64;
        Ok(())
    }

    /// Folds whole lines; entries the snapshot already holds are skipped.
    fn fold_lines(&self, state: &mut Writer, bytes: &[u8]) -> Vec<(Entry, Touched)> {
        let mut out = vec![];
        for line in bytes.split(|b| *b == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let Some(entry) = parse_line(line) else {
                continue;
            };
            if entry.w != self.writer || entry.seq <= state.seq {
                continue;
            }
            if entry.seq != state.seq + 1 {
                tracing::warn!(
                    "work log {}: seq jumps from {} to {}",
                    self.writer,
                    state.seq,
                    entry.seq
                );
            }
            let touched = state.apply(&entry);
            out.push((entry, touched));
        }
        out
    }

    /// Lines another process appended since we last looked (whole lines
    /// only). Call with the file lock held.
    fn catch_up(&self, inner: &mut Inner) -> Result<Vec<(Entry, Touched)>> {
        let path = self.log_path();
        let len = match std::fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e.into()),
        };
        if len == inner.offset {
            return Ok(vec![]);
        }
        if len < inner.offset {
            // Rotated (or cut) by another process: start over.
            let before = inner.state.seq;
            self.reload(&mut inner.state, &mut inner.offset)?;
            tracing::info!("work log {} reloaded at {before}", self.writer);
            return Ok(vec![]);
        }
        let mut f = File::open(&path)?;
        f.seek(SeekFrom::Start(inner.offset))?;
        let mut fresh = vec![];
        f.take(len - inner.offset).read_to_end(&mut fresh)?;
        let whole = fresh.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        let folded = self.fold_lines(&mut inner.state, &fresh[..whole]);
        inner.offset += whole as u64;
        Ok(folded)
    }

    /// Appends events in order, each with the next sequence number, and
    /// tells the watchers. Returns the written entries.
    pub fn append(&self, events: Vec<WorkEvent>) -> Result<Vec<Entry>> {
        if events.is_empty() {
            return Ok(vec![]);
        }
        let (written, change) = {
            let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
            let inner = &mut *guard;
            let _held = Held::take(&self.lock)?;
            let mut touched: Vec<(Entry, Touched)> = self.catch_up(inner)?;
            let at = now_ms();
            let mut lines = String::new();
            let mut written = vec![];
            for (seq, ev) in (inner.state.seq + 1..).zip(events) {
                let entry = Entry {
                    w: self.writer.clone(),
                    seq,
                    at,
                    ev: ev.capped(),
                };
                lines.push_str(&serde_json::to_string(&entry)?);
                lines.push('\n');
                written.push(entry);
            }
            let mut f = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.log_path())?;
            f.write_all(lines.as_bytes())?;
            f.flush()?;
            inner.offset += lines.len() as u64;
            let before = inner.state.seq;
            for e in &written {
                let t = inner.state.apply(e);
                touched.push((e.clone(), t));
            }
            // A snapshot whenever a multiple of SNAPSHOT_EVERY was crossed.
            if before / SNAPSHOT_EVERY != inner.state.seq / SNAPSHOT_EVERY {
                if let Err(e) = self.snapshot(inner) {
                    tracing::warn!("work snapshot not written: {e}");
                }
            }
            let change = change_of(&inner.state, &touched);
            (written, change)
        };
        self.publish(Some(change));
        Ok(written)
    }

    /// Writes the snapshot (old enough things dropped first) and starts the
    /// log over when it has grown past the rotation size.
    fn snapshot(&self, inner: &mut Inner) -> Result<()> {
        let mut state = inner.state.clone();
        state.retain(now_ms());
        let snap = Snapshot {
            w: self.writer.clone(),
            seq: state.seq,
            state: state.clone(),
        };
        crate::atomic_write(&self.snap_path(), serde_json::to_string(&snap)?.as_bytes())?;
        inner.state = state;
        if inner.offset > ROTATE_AT {
            let old = self.dir.join(format!("{}.old.jsonl", self.writer));
            std::fs::rename(self.log_path(), old)?;
            inner.offset = 0;
        }
        Ok(())
    }

    /// Reads the state (after folding in what another process appended).
    pub fn read<R>(&self, f: impl FnOnce(&Writer) -> R) -> R {
        let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let inner = &mut *guard;
        let folded = match Held::take(&self.lock).and_then(|_held| self.catch_up(inner)) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("work log not caught up: {e}");
                vec![]
            }
        };
        let out = f(&inner.state);
        let change = (!folded.is_empty()).then(|| change_of(&inner.state, &folded));
        drop(guard);
        self.publish(change);
        out
    }

    /// A copy of the state.
    #[must_use]
    pub fn state(&self) -> Writer {
        self.read(Clone::clone)
    }

    /// Entries after `seq`, oldest first, as far as the current log file
    /// goes (a peer further behind needs the snapshot instead).
    pub fn since(&self, seq: u64) -> Result<Vec<Entry>> {
        let bytes = match std::fs::read(self.log_path()) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
            Err(e) => return Err(e.into()),
        };
        let whole = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        Ok(bytes[..whole]
            .split(|b| *b == b'\n')
            .filter_map(parse_line)
            .filter(|e| e.w == self.writer && e.seq > seq)
            .collect())
    }
}

fn change_of(state: &Writer, touched: &[(Entry, Touched)]) -> Change {
    let mut change = Change::default();
    for (_, t) in touched {
        for g in &t.goals {
            if !change.goals.contains(g) {
                change.goals.push(g.clone());
            }
        }
        if let Some(gid) = &t.settled {
            if let Some(g) = state.goals.get(gid) {
                change.settled.retain(|s| &s.goal != gid);
                change.settled.push(Settled {
                    goal: gid.clone(),
                    title: g.title.clone(),
                    status: state.status(g, now_ms()),
                });
            }
        }
    }
    change
}

fn parse_line(line: &[u8]) -> Option<Entry> {
    let raw: RawEntry = match serde_json::from_slice(line) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("work log: unreadable line skipped: {e}");
            return None;
        }
    };
    // A newer Parzi's event still holds its sequence number.
    let ev = serde_json::from_value::<WorkEvent>(raw.ev).unwrap_or_else(|e| {
        tracing::debug!("work log: event {} not understood: {e}", raw.seq);
        WorkEvent::Unknown
    });
    Some(Entry {
        w: raw.w,
        seq: raw.seq,
        at: raw.at,
        ev,
    })
}

/// How much of the log is whole lines that parse: a crash can leave the
/// last line half written, with or without its newline.
fn repair_len(bytes: &[u8]) -> usize {
    let mut end = bytes.len();
    // A tail with no newline was never finished.
    if !bytes.ends_with(b"\n") {
        end = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    }
    // A last full line that is not JSON was cut by a crash too.
    while end > 0 {
        let start = bytes[..end - 1]
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |i| i + 1);
        let line = &bytes[start..end - 1];
        if line.iter().all(u8::is_ascii_whitespace) || serde_json::from_slice::<Value>(line).is_ok()
        {
            break;
        }
        end = start;
    }
    end
}

/// This machine's writer id, `<device>-<4 random>`. A home copied to
/// another machine (another device id) gets a writer of its own.
fn writer_id(dir: &Path) -> Result<String> {
    let path = dir.join("writer.json");
    let device = crate::device::id();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(f) = serde_json::from_str::<WriterFile>(&text) {
            if crate::device::valid_id(&f.writer) && (f.device.is_empty() || f.device == device) {
                return Ok(f.writer);
            }
        }
    }
    let suffix = super::graph::fresh_id(4, &|_| false);
    let mut base: String = device.chars().take(59).collect();
    while base.ends_with('-') {
        base.pop();
    }
    let writer = format!("{base}-{suffix}");
    std::fs::create_dir_all(dir)?;
    crate::atomic_write(
        &path,
        serde_json::to_string_pretty(&WriterFile {
            writer: writer.clone(),
            device,
        })?
        .as_bytes(),
    )?;
    Ok(writer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::model::{RunCause, RunEndKind, RunOutcome};

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn started(n: usize) -> WorkEvent {
        WorkEvent::RunStarted {
            run: format!("r{n}"),
            session: format!("s{}", n % 7),
            model: "claude/m".into(),
            lane: "build".into(),
            cwd: String::new(),
            cause: RunCause::Send,
        }
    }

    fn goal(id: &str, root: &str) -> WorkEvent {
        WorkEvent::GoalCreated {
            goal: id.into(),
            title: format!("goal {id}"),
            project: None,
            root: Some(root.into()),
            prompt: String::new(),
            origin: RunCause::Send,
            later: None,
            continues: None,
        }
    }

    fn replay(dir: &Path, writer: &str) -> Writer {
        let mut w = Writer::new(writer);
        let text = std::fs::read_to_string(dir.join(format!("{writer}.jsonl"))).unwrap();
        for line in text.lines() {
            w.apply(&serde_json::from_str::<Entry>(line).unwrap());
        }
        w
    }

    #[test]
    fn appends_number_entries_and_survive_a_reopen() {
        let d = dir();
        let log = WorkLog::open_as(d.path(), "pc-ab12").unwrap();
        let first = log.append(vec![started(1), goal("g1", "s1")]).unwrap();
        assert_eq!(
            first.iter().map(|e| e.seq).collect::<Vec<_>>(),
            [1, 2],
            "gapless from one"
        );
        drop(log);
        let log = WorkLog::open_as(d.path(), "pc-ab12").unwrap();
        assert_eq!(log.state().seq, 2);
        assert_eq!(log.state().runs["r1"].goal.as_deref(), Some("g1"));
        let next = log.append(vec![started(2)]).unwrap();
        assert_eq!(next[0].seq, 3, "an append after a reopen goes on");
        assert_eq!(log.since(1).unwrap().len(), 2);
    }

    #[test]
    fn sequence_numbers_hold_across_processes() {
        // Two handles on one log stand in for the desk and a CLI run.
        let d = dir();
        let a = Arc::new(WorkLog::open_as(d.path(), "pc-ab12").unwrap());
        let b = Arc::new(WorkLog::open_as(d.path(), "pc-ab12").unwrap());
        let mut threads = vec![];
        for (i, log) in [a.clone(), b.clone(), a.clone(), b.clone()]
            .into_iter()
            .enumerate()
        {
            threads.push(std::thread::spawn(move || {
                for n in 0..40 {
                    log.append(vec![started(i * 100 + n)]).unwrap();
                }
            }));
        }
        for t in threads {
            t.join().unwrap();
        }
        let text = std::fs::read_to_string(d.path().join("pc-ab12.jsonl")).unwrap();
        let seqs: Vec<u64> = text
            .lines()
            .map(|l| serde_json::from_str::<Entry>(l).unwrap().seq)
            .collect();
        assert_eq!(seqs, (1..=160).collect::<Vec<u64>>(), "no gaps, no repeats");
        assert_eq!(a.state().runs.len(), 160, "each handle folds the other's");
        assert_eq!(b.state().runs.len(), 160);
    }

    #[test]
    fn a_snapshot_and_its_tail_equal_a_full_replay() {
        let d = dir();
        let log = WorkLog::open_as(d.path(), "pc-ab12").unwrap();
        for n in 0..300 {
            let mut evs = vec![started(n)];
            if n % 7 == 0 {
                evs.push(goal(&format!("g{n}"), &format!("s{}", n % 7)));
            }
            if n % 3 == 0 {
                evs.push(WorkEvent::RunEnded {
                    run: format!("r{n}"),
                    outcome: RunOutcome::bare(RunEndKind::Completed),
                });
            }
            log.append(evs).unwrap();
        }
        assert!(d.path().join("pc-ab12.snap.json").exists());
        let full = replay(d.path(), "pc-ab12");
        drop(log);
        let reopened = WorkLog::open_as(d.path(), "pc-ab12").unwrap();
        assert!(full.seq > SNAPSHOT_EVERY);
        assert_eq!(reopened.state(), full);
    }

    #[test]
    fn a_torn_last_line_is_cut_and_the_log_goes_on() {
        let d = dir();
        let log = WorkLog::open_as(d.path(), "pc-ab12").unwrap();
        log.append(vec![started(1), started(2)]).unwrap();
        drop(log);
        let path = d.path().join("pc-ab12.jsonl");
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(br#"{"w":"pc-ab12","seq":3,"at":1,"ev":{"kind":"run_st"#)
            .unwrap();
        drop(f);
        let log = WorkLog::open_as(d.path(), "pc-ab12").unwrap();
        assert_eq!(log.state().seq, 2);
        assert_eq!(log.append(vec![started(3)]).unwrap()[0].seq, 3);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(text
            .lines()
            .all(|l| serde_json::from_str::<Entry>(l).is_ok()));
        // A finished line that is not JSON is cut too.
        std::fs::write(&path, format!("{text}not json at all\n")).unwrap();
        assert_eq!(
            repair_len(std::fs::read(&path).unwrap().as_slice()),
            text.len()
        );
    }

    #[test]
    fn a_line_from_a_newer_parzi_keeps_its_number() {
        let d = dir();
        let log = WorkLog::open_as(d.path(), "pc-ab12").unwrap();
        log.append(vec![started(1)]).unwrap();
        drop(log);
        let path = d.path().join("pc-ab12.jsonl");
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(
            b"{\"w\":\"pc-ab12\",\"seq\":2,\"at\":5,\"ev\":{\"type\":\"goal_teleported\"}}\n",
        )
        .unwrap();
        drop(f);
        let log = WorkLog::open_as(d.path(), "pc-ab12").unwrap();
        assert_eq!(log.state().seq, 2);
        assert_eq!(
            log.append(vec![started(3)]).unwrap()[0].seq,
            3,
            "never reused"
        );
    }

    #[test]
    fn watchers_hear_changes_and_settled_goals() {
        let d = dir();
        let log = WorkLog::open_as(d.path(), "pc-ab12").unwrap();
        let heard = Arc::new(Mutex::new(Vec::<Change>::new()));
        let sink = heard.clone();
        log.watch(move |c| sink.lock().unwrap().push(c.clone()));
        log.append(vec![started(1)]).unwrap();
        log.append(vec![goal("g1", "s1")]).unwrap();
        log.append(vec![WorkEvent::RunEnded {
            run: "r1".into(),
            outcome: RunOutcome::bare(RunEndKind::Completed),
        }])
        .unwrap();
        let _ = log.state();
        let heard = heard.lock().unwrap();
        assert_eq!(heard.len(), 3, "every append is told, a quiet look is not");
        assert!(heard[0].goals.is_empty(), "an orphan run names no goal");
        assert_eq!(heard[1].goals, vec!["g1".to_string()]);
        assert_eq!(heard[2].settled[0].status, GoalStatus::Done);
        assert_eq!(heard[2].settled[0].title, "goal g1");
    }

    #[test]
    fn the_writer_id_is_kept_per_machine() {
        let d = dir();
        let a = writer_id(d.path()).unwrap();
        assert_eq!(writer_id(d.path()).unwrap(), a);
        assert!(crate::device::valid_id(&a), "{a}");
        assert!(a.starts_with(&crate::device::id()));
        std::fs::write(
            d.path().join("writer.json"),
            r#"{"writer":"other-pc-zz99","device":"other-pc"}"#,
        )
        .unwrap();
        assert_ne!(
            writer_id(d.path()).unwrap(),
            "other-pc-zz99",
            "a copied home"
        );
    }
}
