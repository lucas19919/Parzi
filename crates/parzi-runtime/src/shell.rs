use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

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
/// Concurrent background shells per session before we refuse.
pub const MAX_BACKGROUND_PER_SESSION: usize = 8;
/// Background shells are killed past this age; servers are restarted, not squatted on.
pub const BACKGROUND_MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);

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
    cmd: String,
    child: Option<Child>,
    /// Last RING_KEEP bytes; `total` counts everything ever appended.
    buf: Vec<u8>,
    total: u64,
    spill: PathBuf,
    spill_capped: bool,
    started: Instant,
    exit: Option<i32>,
    killed: bool,
}

#[derive(Default)]
struct Registry {
    shells: HashMap<String, LiveShell>,
    seq: u64,
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
            inner: Arc::new(Mutex::new(Registry {
                shells: HashMap::new(),
                seq: 0,
            })),
            sid: sid.to_string(),
        }
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
        &self,
        cmd: &str,
        workdir: &std::path::Path,
    ) -> std::io::Result<(
        Child,
        tokio::process::ChildStdout,
        tokio::process::ChildStderr,
    )> {
        let (program, mut args) = process::shell_program();
        args.push(cmd.to_string());
        let mut c = Command::new(program);
        c.args(&args)
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
        let (mut child, mut stdout, mut stderr) = self.spawn_shell(cmd, workdir)?;
        let pid = child.id();
        let dir = self.dir()?;
        let spill_path = dir.join(format!(
            "fg-{}.log",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        ));
        let mut spill = std::fs::File::create(&spill_path).ok();
        let mut ring: Vec<u8> = Vec::new();
        let mut spilled: u64 = 0;
        // Stderr drains in the background so a chatty child can never
        // wedge the stdout reader (kept tail only, like Proc).
        let err_keep = Arc::new(Mutex::new(Vec::<u8>::new()));
        let err_sink = err_keep.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                match stderr.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if let Ok(mut s) = err_sink.lock() {
                            s.extend_from_slice(&buf[..n]);
                            if s.len() > 16 * 1024 {
                                let cut = s.len() - 16 * 1024;
                                s.drain(..cut);
                            }
                        }
                    }
                }
            }
        });
        let mut timed_out = false;
        let exit = loop {
            let mut buf = [0u8; 8192];
            tokio::select! {
                biased;
                status = child.wait() => break status.ok().and_then(|s| s.code()),
                n = stdout.read(&mut buf) => match n {
                    Ok(0) => {
                        // EOF is NOT completion (children may hold pipes);
                        // keep waiting for real process exit.
                        match child.try_wait() {
                            Ok(Some(status)) => break status.code(),
                            _ => continue,
                        }
                    }
                    Ok(n) => {
                        use std::io::Write as _;
                        let room = usize::try_from(
                            (SPILL_CAP - spilled).min(u64::try_from(n).unwrap_or(u64::MAX)),
                        )
                        .unwrap_or(usize::MAX);
                            if let Some(f) = spill.as_mut() {
                                if f.write_all(&buf[..room]).is_ok() {
                                    spilled += room as u64;
                                }
                            }
                        ring.extend_from_slice(&buf[..n]);
                        if ring.len() > RING_KEEP {
                            let cut = ring.len() - RING_KEEP;
                            ring.drain(..cut);
                        }
                        continue;
                    }
                    Err(_) => continue,
                },
                () = tokio::time::sleep(timeout) => {
                    timed_out = true;
                    if let Some(pid) = pid {
                        process::kill_tree(pid);
                    }
                    let _ = child.start_kill();
                    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
                    break None;
                }
            }
        };
        drop(spill);
        let mut text = String::from_utf8_lossy(&ring).into_owned();
        if let Ok(err) = err_keep.lock() {
            if !err.is_empty() {
                text.push_str("\n[stderr]\n");
                text.push_str(&String::from_utf8_lossy(&err));
            }
        }
        let truncated = text.len() > OUTPUT_CAP;
        if truncated {
            let cut = text.len() - OUTPUT_CAP;
            text.drain(..cut);
            text = format!(
                "…[earlier output cut; full log: {}]\n{text}",
                spill_path.display()
            );
        } else {
            let _ = std::fs::remove_file(&spill_path);
            return Ok(ExecResult {
                exit,
                timed_out,
                text,
                truncated: false,
                spill: None,
            });
        }
        Ok(ExecResult {
            exit,
            timed_out,
            text,
            truncated: true,
            spill: Some(spill_path),
        })
    }

    /// Start a background shell; returns its id immediately.
    pub fn start(
        &self,
        cmd: &str,
        workdir: &std::path::Path,
        title: &str,
    ) -> std::io::Result<String> {
        let (child, stdout, stderr) = self.spawn_shell(cmd, workdir)?;
        let dir = self.dir()?;
        let mut reg = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::other("shell registry poisoned"))?;
        let running = reg.shells.values().filter(|s| s.exit.is_none()).count();
        if running >= MAX_BACKGROUND_PER_SESSION {
            return Err(std::io::Error::other(format!(
                "already running {running} background shells; kill one first"
            )));
        }
        reg.seq += 1;
        let id = format!("sh-{}", reg.seq);
        let spill = dir.join(format!("{id}.log"));
        let _ = std::fs::File::create(&spill);
        reg.shells.insert(
            id.clone(),
            LiveShell {
                cmd: if title.trim().is_empty() {
                    cmd.to_string()
                } else {
                    title.to_string()
                },
                child: Some(child),
                buf: Vec::new(),
                total: 0,
                spill,
                spill_capped: false,
                started: Instant::now(),
                exit: None,
                killed: false,
            },
        );
        drop(reg);
        let inner = self.inner.clone();
        let sid = id.clone();
        tokio::spawn(async move {
            pump(inner, sid, stdout, stderr).await;
        });
        Ok(id)
    }

    fn with<F, T>(&self, id: &str, f: F) -> Option<T>
    where
        F: FnOnce(&mut LiveShell) -> T,
    {
        self.inner.lock().ok().and_then(|mut reg| {
            // Lifetime + completion are enforced lazily on every touch.
            if let Some(sh) = reg.shells.get_mut(id) {
                if sh.exit.is_none() {
                    if sh.started.elapsed() > BACKGROUND_MAX_LIFETIME {
                        if let Some(child) = sh.child.as_mut() {
                            if let Some(pid) = child.id() {
                                process::kill_tree(pid);
                            }
                            let _ = child.start_kill();
                        }
                        sh.exit = Some(-1);
                        sh.killed = true;
                    } else if let Some(child) = sh.child.as_mut() {
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                sh.exit = status.code();
                            }
                            Ok(None) => {}
                            Err(_) => {
                                sh.exit = Some(-1);
                            }
                        }
                    }
                }
                Some(f(sh))
            } else {
                None
            }
        })
    }

    /// Bytes appended since `offset` (clamped to the ring), plus the new
    /// cursor, liveness, and exit code when done.
    pub fn logs(&self, id: &str, offset: u64, tail: usize) -> Option<LogTail> {
        self.with(id, |sh| {
            let start = offset.min(sh.total);
            let buf_start = sh.total.saturating_sub(sh.buf.len() as u64);
            let from = start.max(buf_start);
            let mut text = {
                let at = usize::try_from(from - buf_start).unwrap_or(usize::MAX);
                String::from_utf8_lossy(&sh.buf[at.min(sh.buf.len())..]).into_owned()
            };
            if text.len() > tail {
                let cut = text.len() - tail;
                text.drain(..cut);
            }
            LogTail {
                text,
                next_offset: sh.total,
                running: sh.exit.is_none(),
                exit: sh.exit,
            }
        })
    }

    pub fn kill(&self, id: &str) -> bool {
        self.with(id, |sh| {
            if sh.exit.is_some() {
                return false;
            }
            if let Some(child) = sh.child.as_mut() {
                if let Some(pid) = child.id() {
                    process::kill_tree(pid);
                }
                let _ = child.start_kill();
            }
            sh.exit = Some(-1);
            sh.killed = true;
            true
        })
        .unwrap_or(false)
    }

    /// Kill every live shell in this registry. Used when the owning
    /// session is deleted or purged so nothing outlives it.
    pub fn kill_all(&self) {
        let Ok(mut reg) = self.inner.lock() else {
            return;
        };
        for sh in reg.shells.values_mut() {
            if sh.exit.is_some() {
                continue;
            }
            if let Some(child) = sh.child.as_mut() {
                if let Some(pid) = child.id() {
                    process::kill_tree(pid);
                }
                let _ = child.start_kill();
            }
            sh.exit = Some(-1);
            sh.killed = true;
        }
    }

    pub fn describe(&self, id: &str) -> Option<(String, bool, Option<i32>)> {
        self.with(id, |sh| (sh.cmd.clone(), sh.exit.is_none(), sh.exit))
    }
}

/// Pump a background shell: stream stdout into ring + spill until the
/// PROCESS exits (never pipe EOF — children inherit stdio), then drain
/// the remainder and record the exit code.
async fn pump(
    inner: Arc<Mutex<Registry>>,
    id: String,
    mut stdout: tokio::process::ChildStdout,
    mut stderr: tokio::process::ChildStderr,
) {
    let err_keep = Arc::new(Mutex::new(Vec::<u8>::new()));
    let err_sink = err_keep.clone();
    tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut s) = err_sink.lock() {
                        s.extend_from_slice(&buf[..n]);
                        if s.len() > 16 * 1024 {
                            let cut = s.len() - 16 * 1024;
                            s.drain(..cut);
                        }
                    }
                }
            }
        }
    });
    let mut buf = [0u8; 8192];
    // Wait for process exit; keep draining in the meantime. try_wait is
    // polled cheaply because reads also wake us.
    let exit: Option<i32> = loop {
        // Lifetime cap, checked without needing output.
        let over = inner.lock().map(|reg| {
            reg.shells.get(&id).is_some_and(|sh| {
                sh.exit.is_none() && sh.started.elapsed() > BACKGROUND_MAX_LIFETIME
            })
        });
        if over.unwrap_or(false) {
            let _ = inner.lock().map(|mut reg| {
                if let Some(sh) = reg.shells.get_mut(&id) {
                    if let Some(child) = sh.child.as_mut() {
                        if let Some(pid) = child.id() {
                            process::kill_tree(pid);
                        }
                        let _ = child.start_kill();
                    }
                    sh.exit = Some(-1);
                    sh.killed = true;
                }
            });
            break Some(-1);
        }
        tokio::select! {
            biased;
            n = stdout.read(&mut buf) => match n {
                Ok(0) => {
                    // Pipe EOF: someone still alive out there, or the
                    // process exited. Only process exit counts.
                    let done: Option<i32> = inner
                        .lock()
                        .map(|mut reg| {
                            let mut code = None;
                            if let Some(sh) = reg.shells.get_mut(&id) {
                                if let Some(child) = sh.child.as_mut() {
                                    if let Ok(Some(status)) = child.try_wait() {
                                        code = status.code();
                                        sh.exit = code;
                                    }
                                } else if sh.exit.is_some() {
                                    code = sh.exit;
                                }
                            }
                            code
                        })
                        .unwrap_or(None);
                    match done {
                        Some(code) => break Some(code),
                        _ => {
                            // Nobody home on stdout but the process lives
                            // (children holding pipes). Park briefly.
                            tokio::time::sleep(Duration::from_millis(200)).await;
                            continue;
                        }
                    }
                }
                Ok(n) => {
                    append(&inner, &id, &buf[..n]);
                    continue;
                }
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            },
            () = tokio::time::sleep(Duration::from_millis(500)) => {
                // Wake periodically to re-check exit even when silent.
                let done: Option<i32> = inner
                    .lock()
                    .map(|mut reg| {
                        let mut code = None;
                        if let Some(sh) = reg.shells.get_mut(&id) {
                            if let Some(child) = sh.child.as_mut() {
                                if let Ok(Some(status)) = child.try_wait() {
                                    code = status.code();
                                    sh.exit = code;
                                }
                            } else if sh.exit.is_some() {
                                code = sh.exit;
                            }
                        }
                        code
                    })
                    .unwrap_or(None);
                if let Some(code) = done {
                    break Some(code);
                }
                continue;
            }
        }
    };
    // Drain whatever the pipes still hold, then close out.
    loop {
        match tokio::time::timeout(Duration::from_millis(300), stdout.read(&mut buf)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(n)) => append(&inner, &id, &buf[..n]),
        }
    }
    if let Ok(mut reg) = inner.lock() {
        if let Some(sh) = reg.shells.get_mut(&id) {
            sh.child.take();
            if sh.exit.is_none() {
                sh.exit = exit;
            }
        }
    }
}

fn append(inner: &Arc<Mutex<Registry>>, id: &str, chunk: &[u8]) {
    use std::io::Write as _;
    let Ok(mut reg) = inner.lock() else { return };
    let Some(sh) = reg.shells.get_mut(id) else {
        return;
    };
    if !sh.spill_capped {
        let append_ok = std::fs::OpenOptions::new()
            .append(true)
            .open(&sh.spill)
            .map(|mut f| f.write_all(chunk).is_ok())
            .unwrap_or(false);
        let capped = std::fs::metadata(&sh.spill)
            .map(|m| m.len() > SPILL_CAP)
            .unwrap_or(false);
        if !append_ok || capped {
            sh.spill_capped = true;
        }
    }
    sh.buf.extend_from_slice(chunk);
    sh.total += chunk.len() as u64;
    if sh.buf.len() > RING_KEEP {
        let cut = sh.buf.len() - RING_KEEP;
        sh.buf.drain(..cut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd_echo() -> &'static str {
        if cfg!(windows) {
            "echo hello-shell"
        } else {
            "echo hello-shell"
        }
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
    async fn background_lifecycle() {
        let reg = ShellRegistry::new("test-bg");
        let id = reg.start(&cmd_sleep(30), &tmp(), "sleeper").unwrap();
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
    async fn background_cap_refuses_the_ninth() {
        let reg = ShellRegistry::new("test-cap");
        for _ in 0..MAX_BACKGROUND_PER_SESSION {
            reg.start(&cmd_sleep(30), &tmp(), "sleeper").unwrap();
        }
        assert!(reg.start(&cmd_sleep(30), &tmp(), "one-more").is_err());
        // Reap everything so temp shells don't linger past the test.
        let ids: Vec<String> = reg.inner.lock().unwrap().shells.keys().cloned().collect();
        for id in ids {
            reg.kill(&id);
        }
        scrub("test-cap");
    }

    #[tokio::test]
    async fn logs_offset_walks_forward() {
        let reg = ShellRegistry::new("test-offset");
        let id = reg.start(cmd_echo(), &tmp(), "echo").unwrap();
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
}
