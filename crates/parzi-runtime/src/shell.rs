use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};

use parzi_providers::process;
#[cfg(windows)]
use parzi_providers::process::CREATE_NO_WINDOW;

/// Foreground default: Claude-compatible two minutes.
pub const FOREGROUND_DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// Foreground ceiling: ten minutes, same spirit as BASH_MAX_TIMEOUT_MS.
pub const FOREGROUND_MAX_TIMEOUT: Duration = Duration::from_secs(600);
/// What a single tool result carries inline.
pub const OUTPUT_CAP: usize = 12 * 1024;
/// In-memory ring per shell; everything also appends to the spill file.
const RING_KEEP: usize = 64 * 1024;
/// Spill files stop growing here; the tool result says so when it happens.
const SPILL_CAP: u64 = 8 * 1024 * 1024;
/// Foreground stderr kept for the tool result.
const STDERR_KEEP: usize = 16 * 1024;
/// Pipes still open after exit (a grandchild holding them) are read this long, then dropped.
const DRAIN_FOR: Duration = Duration::from_secs(2);
/// Concurrent background shells per session before we refuse.
pub const MAX_BACKGROUND_PER_SESSION: usize = 8;
/// Background shells are killed past this age; servers are restarted, not squatted on.
pub const BACKGROUND_MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);

/// Append one chunk to the ring and the spill file (until `SPILL_CAP`).
fn ingest_output(
    ring: &mut Vec<u8>,
    spill: &mut Option<std::fs::File>,
    spilled: &mut u64,
    chunk: &[u8],
) {
    use std::io::Write as _;
    let room = usize::try_from(SPILL_CAP.saturating_sub(*spilled))
        .unwrap_or(usize::MAX)
        .min(chunk.len());
    if let Some(f) = spill.as_mut() {
        if room > 0 && f.write_all(&chunk[..room]).is_ok() {
            *spilled += room as u64;
        } else {
            // Capped or failing: stop touching the file.
            *spill = None;
        }
    }
    ring.extend_from_slice(chunk);
    if ring.len() > RING_KEEP {
        let cut = ring.len() - RING_KEEP;
        ring.drain(..cut);
    }
}

/// Keep at most `keep` trailing bytes, cutting on a char boundary.
fn keep_tail(text: &mut String, keep: usize) -> bool {
    if text.len() <= keep {
        return false;
    }
    let mut cut = text.len() - keep;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    text.drain(..cut);
    true
}

/// Collect output still in flight after the child is gone, so a fast
/// exit never reports empty output. Bounded overall: a grandchild
/// holding (or chattering on) the pipe cannot hang the turn.
async fn drain_stdout(
    stdout: &mut ChildStdout,
    ring: &mut Vec<u8>,
    spill: &mut Option<std::fs::File>,
    spilled: &mut u64,
) {
    let until = tokio::time::Instant::now() + DRAIN_FOR;
    let mut buf = [0u8; 8192];
    while let Ok(Ok(n)) = tokio::time::timeout_at(until, stdout.read(&mut buf)).await {
        if n == 0 {
            break;
        }
        ingest_output(ring, spill, spilled, &buf[..n]);
    }
}

/// Drain stderr off to the side so a chatty child never wedges stdout.
fn drain_stderr<F>(mut stderr: ChildStderr, mut sink: F) -> tokio::task::JoinHandle<()>
where
    F: FnMut(&[u8]) + Send + 'static,
{
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => sink(&buf[..n]),
            }
        }
    })
}

/// Kill a child's tree, then the child, off the async workers (taskkill
/// and ps block). Tree first: taskkill /T cannot find it once the root is gone.
fn reap(mut child: Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let mut job = move || {
        if let Some(pid) = child.id() {
            process::kill_tree(pid);
        }
        let _ = child.start_kill();
    };
    match tokio::runtime::Handle::try_current() {
        Ok(rt) => drop(rt.spawn_blocking(job)),
        Err(_) => job(),
    }
}

/// A foreground child. Dropped unfinished (Stop cancels the call) it
/// takes its whole tree down instead of leaving it running.
struct Fg(Option<Child>);

impl Drop for Fg {
    fn drop(&mut self) {
        if let Some(child) = self.0.take() {
            reap(child);
        }
    }
}

pub struct ExecResult {
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub text: String,
    pub truncated: bool,
    pub spill: Option<PathBuf>,
}

pub struct LogTail {
    pub text: String,
    pub next_offset: u64,
    pub running: bool,
    pub exit: Option<i32>,
}

struct LiveShell {
    /// None while the slot is reserved but the process is not up yet.
    child: Option<Child>,
    /// Last RING_KEEP bytes; `total` counts everything ever appended.
    buf: Vec<u8>,
    total: u64,
    spill: Option<std::fs::File>,
    spilled: u64,
    started: Instant,
    exit: Option<i32>,
    exited_at: Option<Instant>,
    /// The pump read the last output after exit; only then is it done.
    drained: bool,
}

impl LiveShell {
    fn reserved() -> Self {
        Self {
            child: None,
            buf: Vec::new(),
            total: 0,
            spill: None,
            spilled: 0,
            started: Instant::now(),
            exit: None,
            exited_at: None,
            drained: false,
        }
    }

    fn mark_exit(&mut self, code: i32) {
        self.exit = Some(code);
        self.exited_at.get_or_insert_with(Instant::now);
    }

    /// The exit code once the output after exit is in the log too. A pump
    /// that never reports back is given up on after twice the drain time.
    fn settled(&self) -> Option<i32> {
        let late = self.exited_at.is_some_and(|t| t.elapsed() > DRAIN_FOR * 2);
        self.exit.filter(|_| self.drained || late)
    }

    /// Mark killed; the child comes back to be reaped outside the lock.
    fn kill(&mut self) -> Option<Child> {
        self.mark_exit(-1);
        self.child.take()
    }

    /// Lifetime + completion, enforced lazily on every touch. Says whether
    /// it still runs, plus the child to reap when the lifetime cap just hit.
    fn enforce(&mut self) -> (bool, Option<Child>) {
        if self.exit.is_some() {
            return (false, None);
        }
        if self.started.elapsed() > BACKGROUND_MAX_LIFETIME {
            return (false, self.kill());
        }
        let Some(child) = self.child.as_mut() else {
            return (true, None);
        };
        let code = match child.try_wait() {
            Ok(None) => return (true, None),
            // A signal death has no code; it is still done.
            Ok(Some(status)) => status.code().unwrap_or(-1),
            Err(_) => -1,
        };
        self.mark_exit(code);
        (false, None)
    }

    fn ingest(&mut self, chunk: &[u8]) {
        ingest_output(&mut self.buf, &mut self.spill, &mut self.spilled, chunk);
        self.total += chunk.len() as u64;
    }
}

#[derive(Default)]
struct Registry {
    shells: HashMap<String, LiveShell>,
    seq: u64,
}

/// A panic while holding the lock must not brick every shell tool after it.
fn lock(inner: &Mutex<Registry>) -> MutexGuard<'_, Registry> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

fn append(inner: &Mutex<Registry>, id: &str, chunk: &[u8]) {
    if let Some(sh) = lock(inner).shells.get_mut(id) {
        sh.ingest(chunk);
    }
}

/// The harness shell, per session. Cloned handles share one registry;
/// completion is keyed on process `exit`, never on pipe close (a
/// background child inheriting stdio would otherwise hang the wait
/// forever — the classic cross-spawn hang).
#[derive(Clone, Default)]
pub struct ShellRegistry {
    inner: Arc<Mutex<Registry>>,
    sid: String,
}

impl ShellRegistry {
    pub fn new(sid: &str) -> Self {
        Self {
            inner: Arc::default(),
            sid: sid.to_string(),
        }
    }

    fn reg(&self) -> MutexGuard<'_, Registry> {
        lock(&self.inner)
    }

    fn dir(&self) -> std::io::Result<PathBuf> {
        let dir = parzi_core::paths::sessions_dir()
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .join(&self.sid)
            .join("shell");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn spawn_shell(
        cmd: &str,
        workdir: &std::path::Path,
    ) -> std::io::Result<(Child, ChildStdout, ChildStderr)> {
        let (program, mut args) = process::shell_program();
        args.push(cmd.to_string());
        let mut c = Command::new(program);
        // Same scrubbed environment as hooks and MCP servers: no API keys.
        c.args(&args)
            .env_clear()
            .envs(crate::mcp::child_env(&HashMap::new()))
            .current_dir(workdir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        c.creation_flags(CREATE_NO_WINDOW);
        let mut child = c.spawn()?;
        process::adopt(&child);
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("shell did not offer stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| std::io::Error::other("shell did not offer stderr"))?;
        Ok((child, stdout, stderr))
    }

    /// One-shot foreground execution with a deadline.
    pub async fn exec(
        &self,
        cmd: &str,
        workdir: &std::path::Path,
        timeout: Duration,
    ) -> std::io::Result<ExecResult> {
        let timeout = timeout.clamp(Duration::from_secs(1), FOREGROUND_MAX_TIMEOUT);
        // One deadline for the whole call: output must not keep pushing it back.
        let deadline = tokio::time::Instant::now() + timeout;
        let spill_path = self
            .dir()?
            .join(format!("fg-{}.log", uuid::Uuid::new_v4().simple()));
        let (child, mut stdout, stderr) = Self::spawn_shell(cmd, workdir)?;
        let mut fg = Fg(Some(child));
        let Some(child) = fg.0.as_mut() else {
            return Err(std::io::Error::other("the shell vanished"));
        };
        let mut spill = std::fs::File::create(&spill_path).ok();
        let mut ring: Vec<u8> = Vec::new();
        let mut spilled: u64 = 0;
        let err_keep = Arc::new(Mutex::new(Vec::<u8>::new()));
        let err_sink = err_keep.clone();
        let mut err_task = drain_stderr(stderr, move |chunk| {
            let mut s = err_sink.lock().unwrap_or_else(PoisonError::into_inner);
            s.extend_from_slice(chunk);
            if s.len() > STDERR_KEEP {
                let cut = s.len() - STDERR_KEEP;
                s.drain(..cut);
            }
        });
        let mut timed_out = false;
        let mut out_open = true;
        let exit = loop {
            let mut buf = [0u8; 8192];
            tokio::select! {
                biased;
                status = child.wait() => break status.ok().and_then(|s| s.code()),
                n = stdout.read(&mut buf), if out_open => match n {
                    // EOF is NOT completion (children may hold pipes):
                    // stop polling stdout and wait on the process instead.
                    Ok(0) | Err(_) => out_open = false,
                    Ok(n) => ingest_output(&mut ring, &mut spill, &mut spilled, &buf[..n]),
                },
                () = tokio::time::sleep_until(deadline) => {
                    timed_out = true;
                    if let Some(pid) = child.id() {
                        let _ = tokio::task::spawn_blocking(move || process::kill_tree(pid)).await;
                    }
                    let _ = child.start_kill();
                    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
                    break None;
                }
            }
        };
        if out_open {
            drain_stdout(&mut stdout, &mut ring, &mut spill, &mut spilled).await;
        }
        drop(spill);
        // Let the stderr drain land (bounded like stdout) before composing.
        let _ = tokio::time::timeout(DRAIN_FOR, &mut err_task).await;
        err_task.abort();
        let mut text = String::from_utf8_lossy(&ring).into_owned();
        {
            let err = err_keep.lock().unwrap_or_else(PoisonError::into_inner);
            if !err.is_empty() {
                text.push_str("\n[stderr]\n");
                text.push_str(&String::from_utf8_lossy(&err));
            }
        }
        if !keep_tail(&mut text, OUTPUT_CAP) {
            let _ = std::fs::remove_file(&spill_path);
            return Ok(ExecResult {
                exit,
                timed_out,
                text,
                truncated: false,
                spill: None,
            });
        }
        text = format!(
            "…[earlier output cut; full log: {}]\n{text}",
            spill_path.display()
        );
        Ok(ExecResult {
            exit,
            timed_out,
            text,
            truncated: true,
            spill: Some(spill_path),
        })
    }

    /// Start a background shell; returns its id immediately.
    pub fn start(&self, cmd: &str, workdir: &std::path::Path) -> std::io::Result<String> {
        let dir = self.dir()?;
        // Claim the slot before anything runs: a refused command never starts.
        let id = {
            let mut reg = self.reg();
            let running = reg.shells.values().filter(|s| s.exit.is_none()).count();
            if running >= MAX_BACKGROUND_PER_SESSION {
                return Err(std::io::Error::other(format!(
                    "already running {running} background shells; kill one first"
                )));
            }
            reg.seq += 1;
            let id = format!("sh-{}", reg.seq);
            reg.shells.insert(id.clone(), LiveShell::reserved());
            id
        };
        let spill = std::fs::File::create(dir.join(format!("{id}.log"))).ok();
        let spawned = Self::spawn_shell(cmd, workdir);
        let mut reg = self.reg();
        let (child, stdout, stderr) = match spawned {
            Ok(parts) => parts,
            Err(e) => {
                reg.shells.remove(&id);
                return Err(e);
            }
        };
        match reg.shells.get_mut(&id) {
            Some(sh) if sh.exit.is_none() => {
                sh.child = Some(child);
                sh.spill = spill;
            }
            // Killed while starting (session deleted): never let it run on.
            _ => {
                drop(reg);
                reap(child);
                return Err(std::io::Error::other(
                    "the shell was stopped while starting",
                ));
            }
        }
        drop(reg);
        tokio::spawn(pump(self.inner.clone(), id.clone(), stdout, stderr));
        Ok(id)
    }

    fn with<F, T>(&self, id: &str, f: F) -> Option<T>
    where
        F: FnOnce(&mut LiveShell) -> T,
    {
        let (out, expired) = {
            let mut reg = self.reg();
            let sh = reg.shells.get_mut(id)?;
            let (_, expired) = sh.enforce();
            (f(sh), expired)
        };
        if let Some(child) = expired {
            reap(child);
        }
        Some(out)
    }

    /// Bytes appended since `offset` (clamped to the ring), plus the new
    /// cursor, liveness, and exit code when done.
    pub fn logs(&self, id: &str, offset: u64, tail: usize) -> Option<LogTail> {
        let (bytes, total, exit) = self.with(id, |sh| {
            let buf_start = sh.total.saturating_sub(sh.buf.len() as u64);
            let from = offset.min(sh.total).max(buf_start);
            let at = usize::try_from(from - buf_start)
                .unwrap_or(usize::MAX)
                .min(sh.buf.len());
            (sh.buf[at..].to_vec(), sh.total, sh.settled())
        })?;
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        keep_tail(&mut text, tail);
        Some(LogTail {
            text,
            next_offset: total,
            running: exit.is_none(),
            exit,
        })
    }

    pub fn kill(&self, id: &str) -> bool {
        match self.with(id, |sh| sh.exit.is_none().then(|| sh.kill())) {
            Some(Some(child)) => {
                if let Some(child) = child {
                    reap(child);
                }
                true
            }
            _ => false,
        }
    }

    /// Kill every live shell in this registry. Used when the owning
    /// session is deleted or purged so nothing outlives it.
    pub fn kill_all(&self) {
        let doomed: Vec<Child> = self
            .reg()
            .shells
            .values_mut()
            .filter(|s| s.exit.is_none())
            .filter_map(LiveShell::kill)
            .collect();
        for child in doomed {
            reap(child);
        }
    }
}

/// Pump a background shell: stream stdout and stderr into ring + spill
/// until the PROCESS exits (never pipe EOF — children inherit stdio),
/// then drain the remainder briefly.
async fn pump(
    inner: Arc<Mutex<Registry>>,
    id: String,
    mut stdout: ChildStdout,
    stderr: ChildStderr,
) {
    let (err_inner, err_id) = (inner.clone(), id.clone());
    let mut err_task = drain_stderr(stderr, move |chunk| append(&err_inner, &err_id, chunk));
    let mut buf = [0u8; 8192];
    let mut out_open = true;
    loop {
        tokio::select! {
            biased;
            n = stdout.read(&mut buf), if out_open => match n {
                // Pipe EOF: only process exit counts, so stop reading and keep checking.
                Ok(0) | Err(_) => out_open = false,
                Ok(n) => append(&inner, &id, &buf[..n]),
            },
            // Wake to re-check exit even when silent.
            () = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
        let (live, expired) = lock(&inner)
            .shells
            .get_mut(&id)
            .map_or((false, None), LiveShell::enforce);
        if let Some(child) = expired {
            reap(child);
        }
        if !live {
            break;
        }
    }
    if out_open {
        let until = tokio::time::Instant::now() + DRAIN_FOR;
        while let Ok(Ok(n)) = tokio::time::timeout_at(until, stdout.read(&mut buf)).await {
            if n == 0 {
                break;
            }
            append(&inner, &id, &buf[..n]);
        }
    }
    let _ = tokio::time::timeout(DRAIN_FOR, &mut err_task).await;
    err_task.abort();
    if let Some(sh) = lock(&inner).shells.get_mut(&id) {
        sh.child.take();
        sh.drained = true;
        sh.spill = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd_echo() -> &'static str {
        "echo hello-shell"
    }

    fn cmd_sleep(secs: u64) -> String {
        if cfg!(windows) {
            format!("Start-Sleep -Seconds {secs}")
        } else {
            format!("sleep {secs}")
        }
    }

    fn tmp() -> PathBuf {
        std::env::temp_dir()
    }

    fn scrub(sid: &str) {
        if let Ok(dir) = parzi_core::paths::sessions_dir() {
            let _ = std::fs::remove_dir_all(dir.join(sid));
        }
    }

    #[tokio::test]
    async fn foreground_echo_returns_output() {
        let reg = ShellRegistry::new("test-echo");
        let out = reg
            .exec(cmd_echo(), &tmp(), Duration::from_secs(30))
            .await
            .unwrap();
        assert!(!out.timed_out);
        assert!(out.text.contains("hello-shell"), "{}", out.text);
        assert!(!out.truncated);
        assert!(out.spill.is_none());
        assert_eq!(out.exit, Some(0));
        scrub("test-echo");
    }

    #[tokio::test]
    async fn foreground_timeout_kills() {
        let reg = ShellRegistry::new("test-timeout");
        let out = reg
            .exec(&cmd_sleep(30), &tmp(), Duration::from_millis(400))
            .await
            .unwrap();
        assert!(out.timed_out, "a 30s sleep must trip a 400ms deadline");
        scrub("test-timeout");
    }

    #[tokio::test]
    async fn foreground_chatty_command_still_times_out() {
        // Steady output used to re-arm the deadline on every chunk.
        let reg = ShellRegistry::new("test-chatty");
        let chatty = if cfg!(windows) {
            "while ($true) { 'tick'; Start-Sleep -Milliseconds 20 }"
        } else {
            "while true; do echo tick; sleep 0.02; done"
        };
        let t0 = std::time::Instant::now();
        let out = tokio::time::timeout(
            Duration::from_secs(60),
            reg.exec(chatty, &tmp(), Duration::from_secs(4)),
        )
        .await
        .expect("a chatty command must not outlive its deadline")
        .unwrap();
        assert!(out.timed_out);
        assert!(t0.elapsed() < Duration::from_secs(20), "{:?}", t0.elapsed());
        assert!(out.text.contains("tick"), "{}", out.text);
        scrub("test-chatty");
    }

    #[test]
    fn tails_cut_on_char_boundaries() {
        let mut s = "é".repeat(10);
        assert!(keep_tail(&mut s, 5));
        assert_eq!(s, "éé");
        let mut short = String::from("ok");
        assert!(!keep_tail(&mut short, 5));
        assert_eq!(short, "ok");
    }

    #[test]
    fn logs_tail_never_splits_a_char() {
        let reg = ShellRegistry::new("test-utf8");
        let mut sh = LiveShell::reserved();
        sh.ingest("é".repeat(100).as_bytes());
        sh.exit = Some(0);
        sh.drained = true;
        reg.reg().shells.insert("sh-1".into(), sh);
        for tail in [1, 7, 33] {
            let t = reg.logs("sh-1", 0, tail).unwrap();
            assert!(t.text.len() <= tail, "{tail}: {:?}", t.text);
            assert!(t.text.chars().all(|c| c == 'é'), "{tail}: {:?}", t.text);
        }
        assert!(!reg.inner.is_poisoned());
    }

    #[tokio::test]
    async fn background_lifecycle() {
        let reg = ShellRegistry::new("test-bg");
        let id = reg.start(&cmd_sleep(30), &tmp()).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let tail = reg.logs(&id, 0, 4096).unwrap();
        assert!(tail.running);
        assert!(reg.kill(&id));
        for _ in 0..50 {
            if reg.logs(&id, 0, 16).is_some_and(|t| !t.running) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let tail = reg.logs(&id, tail.next_offset, 4096).unwrap();
        assert!(!tail.running);
        scrub("test-bg");
    }

    #[tokio::test]
    async fn background_cap_refuses_the_ninth_before_it_runs() {
        let reg = ShellRegistry::new("test-cap");
        for _ in 0..MAX_BACKGROUND_PER_SESSION {
            reg.start(&cmd_sleep(30), &tmp()).unwrap();
        }
        let dir = tmp().join(format!("parzi-cap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("ran.txt");
        let _ = std::fs::remove_file(&marker);
        assert!(reg.start("echo ran > ran.txt", &dir).is_err());
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(!marker.exists(), "a refused command must never start");
        reg.kill_all();
        let _ = std::fs::remove_dir_all(&dir);
        scrub("test-cap");
    }

    #[tokio::test]
    async fn logs_offset_walks_forward() {
        let reg = ShellRegistry::new("test-offset");
        let id = reg.start(cmd_echo(), &tmp()).unwrap();
        let mut off = 0;
        for _ in 0..50 {
            if let Some(t) = reg.logs(&id, off, 4096) {
                off = t.next_offset;
                if !t.running {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let tail = reg.logs(&id, 0, 4096).unwrap();
        assert!(tail.text.contains("hello-shell"), "{:?}", tail.text);
        reg.kill(&id);
        scrub("test-offset");
    }

    #[tokio::test]
    async fn background_stderr_reaches_the_log() {
        let reg = ShellRegistry::new("test-stderr");
        let cmd = if cfg!(windows) {
            "[Console]::Error.WriteLine('err-marker')"
        } else {
            "echo err-marker 1>&2"
        };
        let id = reg.start(cmd, &tmp()).unwrap();
        let mut seen = String::new();
        for _ in 0..100 {
            let t = reg.logs(&id, 0, 4096).unwrap();
            seen = t.text;
            if !t.running && seen.contains("err-marker") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(seen.contains("err-marker"), "{seen:?}");
        scrub("test-stderr");
    }
}
