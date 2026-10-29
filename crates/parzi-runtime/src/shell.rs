use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};

use parzi_providers::process;
#[cfg(windows)]
use parzi_providers::process::CREATE_NO_WINDOW;

pub const FOREGROUND_DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
pub const FOREGROUND_MAX_TIMEOUT: Duration = Duration::from_secs(600);
pub const OUTPUT_CAP: usize = 12 * 1024;
const RING_KEEP: usize = 64 * 1024;
const SPILL_CAP: u64 = 8 * 1024 * 1024;
const STDERR_KEEP: usize = 16 * 1024;
const DRAIN_FOR: Duration = Duration::from_secs(2);
pub const MAX_BACKGROUND_PER_SESSION: usize = 8;
pub const BACKGROUND_MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);

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
            *spill = None;
        }
    }
    ring.extend_from_slice(chunk);
    if ring.len() > RING_KEEP {
        let cut = ring.len() - RING_KEEP;
        ring.drain(..cut);
    }
}

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

#[derive(Debug, Clone, serde::Serialize)]
pub struct ShellJob {
    pub id: String,
    pub cmd: String,
    pub background: bool,
    pub running: bool,
    pub exit: Option<i32>,
    pub started: i64,
    pub ms: u64,
}

const FG_KEEP: usize = 30;

pub struct LogTail {
    pub text: String,
    pub next_offset: u64,
    pub running: bool,
    pub exit: Option<i32>,
}

struct LiveShell {
    cmd: String,
    started_at: i64,
    child: Option<Child>,
    buf: Vec<u8>,
    total: u64,
    spill: Option<std::fs::File>,
    spilled: u64,
    started: Instant,
    exit: Option<i32>,
    exited_at: Option<Instant>,
    drained: bool,
}

impl LiveShell {
    fn reserved(cmd: &str) -> Self {
        Self {
            cmd: cmd.to_string(),
            started_at: now_ms(),
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

    fn settled(&self) -> Option<i32> {
        let late = self.exited_at.is_some_and(|t| t.elapsed() > DRAIN_FOR * 2);
        self.exit.filter(|_| self.drained || late)
    }

    fn kill(&mut self) -> Option<Child> {
        self.mark_exit(-1);
        self.child.take()
    }

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
    foreground: Vec<ShellJob>,
}

struct FgMark {
    inner: Arc<Mutex<Registry>>,
    id: String,
    begun: Instant,
    exit: Option<i32>,
}

impl Drop for FgMark {
    fn drop(&mut self) {
        let mut reg = lock(&self.inner);
        if let Some(j) = reg.foreground.iter_mut().find(|j| j.id == self.id) {
            j.running = false;
            j.exit = Some(self.exit.unwrap_or(-1));
            j.ms = u64::try_from(self.begun.elapsed().as_millis()).unwrap_or(u64::MAX);
        }
        let extra = reg.foreground.len().saturating_sub(FG_KEEP);
        reg.foreground.drain(..extra);
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn lock(inner: &Mutex<Registry>) -> MutexGuard<'_, Registry> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

fn append(inner: &Mutex<Registry>, id: &str, chunk: &[u8]) {
    if let Some(sh) = lock(inner).shells.get_mut(id) {
        sh.ingest(chunk);
    }
}

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

    pub async fn exec(
        &self,
        cmd: &str,
        workdir: &std::path::Path,
        timeout: Duration,
    ) -> std::io::Result<ExecResult> {
        let timeout = timeout.clamp(Duration::from_secs(1), FOREGROUND_MAX_TIMEOUT);
        let deadline = tokio::time::Instant::now() + timeout;
        let spill_path = self
            .dir()?
            .join(format!("fg-{}.log", uuid::Uuid::new_v4().simple()));
        let (child, mut stdout, stderr) = Self::spawn_shell(cmd, workdir)?;
        let fg_id = {
            let mut reg = self.reg();
            reg.seq += 1;
            let id = format!("fg-{}", reg.seq);
            reg.foreground.push(ShellJob {
                id: id.clone(),
                cmd: cmd.to_string(),
                background: false,
                running: true,
                exit: None,
                started: now_ms(),
                ms: 0,
            });
            id
        };
        let mut mark = FgMark {
            inner: self.inner.clone(),
            id: fg_id,
            begun: Instant::now(),
            exit: None,
        };
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
        mark.exit = exit;
        drop(mark);
        if out_open {
            drain_stdout(&mut stdout, &mut ring, &mut spill, &mut spilled).await;
        }
        drop(spill);
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

    pub fn start(&self, cmd: &str, workdir: &std::path::Path) -> std::io::Result<String> {
        let dir = self.dir()?;
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
            reg.shells.insert(id.clone(), LiveShell::reserved(cmd));
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

    pub fn jobs(&self) -> Vec<ShellJob> {
        let mut reg = self.reg();
        let mut expired = vec![];
        let mut out: Vec<ShellJob> = reg
            .shells
            .iter_mut()
            .map(|(id, sh)| {
                let (running, gone) = sh.enforce();
                expired.extend(gone);
                let ms = sh.exited_at.unwrap_or_else(Instant::now) - sh.started;
                ShellJob {
                    id: id.clone(),
                    cmd: sh.cmd.clone(),
                    background: true,
                    running,
                    exit: sh.exit,
                    started: sh.started_at,
                    ms: u64::try_from(ms.as_millis()).unwrap_or(u64::MAX),
                }
            })
            .collect();
        let now = now_ms();
        out.extend(reg.foreground.iter().cloned().map(|mut j| {
            if j.running {
                j.ms = u64::try_from(now - j.started).unwrap_or(0);
            }
            j
        }));
        drop(reg);
        for child in expired {
            reap(child);
        }
        out.sort_by_key(|j| j.started);
        out
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
                Ok(0) | Err(_) => out_open = false,
                Ok(n) => append(&inner, &id, &buf[..n]),
            },
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
    async fn jobs_list_background_and_foreground_commands() {
        let reg = ShellRegistry::new("test-jobs");
        reg.exec(cmd_echo(), &tmp(), Duration::from_secs(30))
            .await
            .unwrap();
        let sleeper = cmd_sleep(30);
        let id = reg.start(&sleeper, &tmp()).unwrap();
        let jobs = reg.jobs();
        assert_eq!(jobs.len(), 2);
        assert!(!jobs[0].background && !jobs[0].running && jobs[0].cmd == cmd_echo());
        assert!(jobs[1].background && jobs[1].running && jobs[1].cmd == sleeper);
        assert!(reg.kill(&id));
        assert!(!reg.jobs()[1].running, "killed is no longer running");
        scrub("test-jobs");
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
        let mut sh = LiveShell::reserved("test");
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
