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
const OUTPUT_CAP: usize = 12 * 1024;
const RING_KEEP: usize = 64 * 1024;
const SPILL_CAP: u64 = 8 * 1024 * 1024;
const STDERR_KEEP: usize = 16 * 1024;
const DRAIN_FOR: Duration = Duration::from_secs(2);
pub const MAX_BACKGROUND_PER_SESSION: usize = 8;
const BACKGROUND_MAX_LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);
const MAX_JOB_URLS: usize = 4;

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HostJob {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    pub cmd: String,
    pub workdir: String,
    pub urls: Vec<String>,
    pub running: bool,
    pub exit: Option<i32>,
    pub started: i64,
    pub ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SavedHost {
    id: String,
    cmd: String,
    workdir: String,
    urls: Vec<String>,
    updated: i64,
    #[serde(skip)]
    job_id: Option<String>,
}

const FG_KEEP: usize = 30;
const MAX_SAVED_HOSTS: usize = 16;

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogTail {
    pub text: String,
    pub next_offset: u64,
    pub running: bool,
    pub exit: Option<i32>,
}

struct LiveShell {
    cmd: String,
    workdir: PathBuf,
    host_id: Option<String>,
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
    urls: Vec<String>,
}

impl LiveShell {
    fn reserved(cmd: &str, workdir: PathBuf, host_id: Option<String>) -> Self {
        Self {
            cmd: cmd.to_string(),
            workdir,
            host_id,
            started_at: parzi_core::now_ms_i64(),
            child: None,
            buf: Vec::new(),
            total: 0,
            spill: None,
            spilled: 0,
            started: Instant::now(),
            exit: None,
            exited_at: None,
            drained: false,
            urls: Vec::new(),
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

    fn ingest(&mut self, chunk: &[u8]) -> bool {
        ingest_output(&mut self.buf, &mut self.spill, &mut self.spilled, chunk);
        self.total += chunk.len() as u64;
        let mut changed = false;
        if self.urls.len() < MAX_JOB_URLS {
            let text = String::from_utf8_lossy(chunk);
            for url in extract_loopback_urls(&text) {
                if !self.urls.contains(&url) {
                    self.urls.push(url);
                    changed = true;
                    if self.urls.len() >= MAX_JOB_URLS {
                        break;
                    }
                }
            }
        }
        changed
    }
}

#[derive(Default)]
struct Registry {
    shells: HashMap<String, LiveShell>,
    seq: u64,
    host_seq: u64,
    hosts: Vec<SavedHost>,
    foreground: Vec<ShellJob>,
    /// The session's tool server, for `parzi connector` in its commands.
    link: Option<String>,
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

fn lock(inner: &Mutex<Registry>) -> MutexGuard<'_, Registry> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

fn normalize_workdir_key(path: &str) -> String {
    let trimmed = path.trim().replace('\\', "/");
    let trimmed = trimmed.trim_end_matches('/');
    if cfg!(windows) {
        trimmed.to_ascii_lowercase()
    } else {
        trimmed.to_string()
    }
}

fn hosts_path(sid: &str) -> Option<PathBuf> {
    if sid.is_empty() {
        return None;
    }
    parzi_core::paths::sessions_dir()
        .ok()
        .map(|d| d.join(sid).join("shell").join("hosts.json"))
}

fn load_saved_hosts(sid: &str) -> Vec<SavedHost> {
    let Some(path) = hosts_path(sid) else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(mut list) = serde_json::from_str::<Vec<SavedHost>>(&raw) else {
        return Vec::new();
    };
    list.retain(|h| !h.id.is_empty() && !h.cmd.trim().is_empty() && !h.urls.is_empty());
    list
}

fn save_hosts_file(sid: &str, hosts: &[SavedHost]) {
    let Some(path) = hosts_path(sid) else {
        return;
    };
    if hosts.is_empty() {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(hosts) {
        let _ = std::fs::write(&path, json);
    }
}

fn next_host_seq(hosts: &[SavedHost]) -> u64 {
    hosts
        .iter()
        .filter_map(|h| h.id.strip_prefix("host-")?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
}

fn append(inner: &Mutex<Registry>, sid: &str, id: &str, chunk: &[u8]) {
    let snapshot = {
        let mut reg = lock(inner);
        let meta = {
            let Some(sh) = reg.shells.get_mut(id) else {
                return;
            };
            if !sh.ingest(chunk) || sh.urls.is_empty() {
                None
            } else {
                Some((
                    sh.cmd.clone(),
                    sh.workdir.to_string_lossy().into_owned(),
                    sh.urls.clone(),
                    sh.host_id.clone(),
                ))
            }
        };
        let Some((cmd, workdir, urls, existing_hid)) = meta else {
            return;
        };
        let now = parzi_core::now_ms_i64();
        let cmd_trim = cmd.trim();
        let wd_key = normalize_workdir_key(&workdir);
        let found_idx = existing_hid
            .as_deref()
            .and_then(|hid| reg.hosts.iter().position(|h| h.id == hid))
            .or_else(|| {
                reg.hosts.iter().position(|h| {
                    h.cmd.trim() == cmd_trim && normalize_workdir_key(&h.workdir) == wd_key
                })
            });
        let hid = if let Some(idx) = found_idx {
            let h = &mut reg.hosts[idx];
            h.urls = urls;
            h.updated = now;
            h.job_id = Some(id.to_string());
            h.id.clone()
        } else {
            reg.host_seq += 1;
            let hid = format!("host-{}", reg.host_seq);
            reg.hosts.push(SavedHost {
                id: hid.clone(),
                cmd,
                workdir,
                urls,
                updated: now,
                job_id: Some(id.to_string()),
            });
            hid
        };
        if let Some(sh) = reg.shells.get_mut(id) {
            sh.host_id = Some(hid);
        }
        while reg.hosts.len() > MAX_SAVED_HOSTS {
            let drop_idx = reg
                .hosts
                .iter()
                .position(|h| {
                    h.job_id
                        .as_deref()
                        .and_then(|jid| reg.shells.get(jid))
                        .is_none_or(|sh| sh.exit.is_some())
                })
                .unwrap_or(0);
            reg.hosts.remove(drop_idx);
        }
        Some(reg.hosts.clone())
    };
    if let Some(hosts) = snapshot {
        save_hosts_file(sid, &hosts);
    }
}

#[derive(Clone, Default)]
pub struct ShellRegistry {
    inner: Arc<Mutex<Registry>>,
    sid: String,
}

impl ShellRegistry {
    pub fn new(sid: &str) -> Self {
        let hosts = load_saved_hosts(sid);
        let host_seq = next_host_seq(&hosts);
        Self {
            inner: Arc::new(Mutex::new(Registry {
                hosts,
                host_seq,
                ..Registry::default()
            })),
            sid: sid.to_string(),
        }
    }

    /// Reads persisted hosts for a session without creating a registry or
    /// touching the filesystem when no hosts file exists.
    pub fn hosts_saved(sid: &str) -> Vec<HostJob> {
        load_saved_hosts(sid)
            .into_iter()
            .map(|h| HostJob {
                id: h.id,
                job_id: None,
                cmd: h.cmd,
                workdir: h.workdir,
                urls: h.urls,
                running: false,
                exit: None,
                started: h.updated,
                ms: 0,
            })
            .collect()
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

    /// Points this session's commands at its tool server, so `parzi
    /// connector` in them reaches the session's connectors.
    pub fn set_link(&self, url: Option<String>) {
        self.reg().link = url;
    }

    /// What a command gets on top of the usual environment: the session's
    /// tool server, and Parzi's own `bin` folder (where the app leaves a
    /// `parzi` for `parzi connector` when no CLI is installed) on its PATH.
    fn session_env(&self) -> HashMap<String, String> {
        let mut env = crate::mcp::child_env(&HashMap::new());
        if let Some(url) = self.reg().link.clone() {
            env.insert(crate::connector_cli::URL_VAR.into(), url);
        }
        if let Some(bin) = parzi_core::paths::parzi_dir()
            .ok()
            .map(|d| d.join("bin"))
            .filter(|d| d.is_dir())
        {
            let key = env
                .keys()
                .find(|k| k.eq_ignore_ascii_case("PATH"))
                .cloned()
                .unwrap_or_else(|| "PATH".into());
            let sep = if cfg!(windows) { ";" } else { ":" };
            let rest = env.get(&key).cloned().unwrap_or_default();
            env.insert(key, format!("{}{sep}{rest}", bin.display()));
        }
        env
    }

    fn spawn_shell(
        &self,
        cmd: &str,
        workdir: &std::path::Path,
    ) -> std::io::Result<(Child, ChildStdout, ChildStderr)> {
        let (program, mut args) = process::shell_program();
        args.push(cmd.to_string());
        let mut c = Command::new(program);
        c.args(&args)
            .env_clear()
            .envs(self.session_env())
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
        let (child, mut stdout, stderr) = self.spawn_shell(cmd, workdir)?;
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
                started: parzi_core::now_ms_i64(),
                ms: 0,
                urls: Vec::new(),
                host_id: None,
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
        self.start_linked(cmd, workdir, None)
    }

    fn start_linked(
        &self,
        cmd: &str,
        workdir: &std::path::Path,
        explicit_host_id: Option<String>,
    ) -> std::io::Result<String> {
        let dir = self.dir()?;
        let wd_str = workdir.to_string_lossy().into_owned();
        let wd_key = normalize_workdir_key(&wd_str);
        let cmd_trim = cmd.trim();
        let (id, linked_host, prev_job) = {
            let mut reg = self.reg();
            let running = reg.shells.values().filter(|s| s.exit.is_none()).count();
            if running >= MAX_BACKGROUND_PER_SESSION {
                return Err(std::io::Error::other(format!(
                    "already running {running} background shells; kill one first"
                )));
            }
            let linked = explicit_host_id.or_else(|| {
                reg.hosts
                    .iter()
                    .find(|h| {
                        h.cmd.trim() == cmd_trim
                            && normalize_workdir_key(&h.workdir) == wd_key
                            && h.job_id
                                .as_deref()
                                .and_then(|jid| reg.shells.get(jid))
                                .is_none_or(|sh| sh.exit.is_some())
                    })
                    .map(|h| h.id.clone())
            });
            reg.seq += 1;
            let id = format!("sh-{}", reg.seq);
            reg.shells.insert(
                id.clone(),
                LiveShell::reserved(cmd, workdir.to_path_buf(), linked.clone()),
            );
            let prev_job = linked.as_deref().and_then(|hid| {
                reg.hosts.iter_mut().find(|h| h.id == hid).map(|h| {
                    h.updated = parzi_core::now_ms_i64();
                    h.job_id.replace(id.clone())
                })
            });
            (id, linked, prev_job.flatten())
        };
        let spill = std::fs::File::create(dir.join(format!("{id}.log"))).ok();
        let spawned = self.spawn_shell(cmd, workdir);
        let mut reg = self.reg();
        let (child, stdout, stderr) = match spawned {
            Ok(parts) => parts,
            Err(e) => {
                reg.shells.remove(&id);
                if let Some(hid) = linked_host.as_deref() {
                    if let Some(h) = reg.hosts.iter_mut().find(|h| h.id == hid) {
                        if h.job_id.as_deref() == Some(id.as_str()) {
                            h.job_id = prev_job;
                        }
                    }
                }
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
        tokio::spawn(pump(
            self.inner.clone(),
            self.sid.clone(),
            id.clone(),
            stdout,
            stderr,
        ));
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
        let now = parzi_core::now_ms_i64();
        // Foreground first: the sort is stable, and a command that finished and
        // the one started right after it can share a millisecond.
        let mut out: Vec<ShellJob> = reg
            .foreground
            .iter()
            .cloned()
            .map(|mut j| {
                if j.running {
                    j.ms = u64::try_from(now - j.started).unwrap_or(0);
                }
                j
            })
            .collect();
        out.extend(reg.shells.iter_mut().map(|(id, sh)| {
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
                urls: sh.urls.clone(),
                host_id: sh.host_id.clone(),
            }
        }));
        drop(reg);
        for child in expired {
            reap(child);
        }
        out.sort_by_key(|j| j.started);
        out
    }

    pub fn hosts(&self) -> Vec<HostJob> {
        let mut reg = self.reg();
        let mut expired = vec![];
        let Registry { hosts, shells, .. } = &mut *reg;
        let out: Vec<HostJob> = hosts
            .iter()
            .map(|h| {
                if let Some(sh) = h.job_id.as_deref().and_then(|jid| shells.get_mut(jid)) {
                    let (running, gone) = sh.enforce();
                    expired.extend(gone);
                    let ms = sh.exited_at.unwrap_or_else(Instant::now) - sh.started;
                    let urls = if sh.urls.is_empty() {
                        h.urls.clone()
                    } else {
                        sh.urls.clone()
                    };
                    HostJob {
                        id: h.id.clone(),
                        job_id: h.job_id.clone(),
                        cmd: h.cmd.clone(),
                        workdir: h.workdir.clone(),
                        urls,
                        running,
                        exit: sh.exit,
                        started: sh.started_at,
                        ms: u64::try_from(ms.as_millis()).unwrap_or(u64::MAX),
                    }
                } else {
                    HostJob {
                        id: h.id.clone(),
                        job_id: None,
                        cmd: h.cmd.clone(),
                        workdir: h.workdir.clone(),
                        urls: h.urls.clone(),
                        running: false,
                        exit: None,
                        started: h.updated,
                        ms: 0,
                    }
                }
            })
            .collect();
        drop(reg);
        for child in expired {
            reap(child);
        }
        out
    }

    /// Starts a stopped host or cleanly restarts a running one in its saved
    /// working directory. Waits briefly for an existing process tree to exit
    /// so the port is freed before relaunching.
    pub async fn restart_host(&self, host_id: &str) -> std::io::Result<HostJob> {
        let (cmd, workdir, old_child) = {
            let mut reg = self.reg();
            let Registry { hosts, shells, .. } = &mut *reg;
            let Some(h) = hosts.iter().find(|h| h.id == host_id) else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "host not found",
                ));
            };
            let cmd = h.cmd.clone();
            let workdir = h.workdir.clone();
            let old_child = h
                .job_id
                .as_deref()
                .and_then(|jid| shells.get_mut(jid))
                .and_then(|sh| {
                    let (running, expired) = sh.enforce();
                    if let Some(child) = expired {
                        Some(child)
                    } else if running {
                        sh.kill()
                    } else {
                        None
                    }
                });
            (cmd, workdir, old_child)
        };
        let wd = PathBuf::from(&workdir);
        if !wd.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("folder no longer exists: {workdir}"),
            ));
        }
        if let Some(mut child) = old_child {
            if let Some(pid) = child.id() {
                let _ = tokio::task::spawn_blocking(move || process::kill_tree(pid)).await;
            }
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
        }
        self.start_linked(&cmd, &wd, Some(host_id.to_string()))?;
        self.hosts()
            .into_iter()
            .find(|h| h.id == host_id)
            .ok_or_else(|| std::io::Error::other("host disappeared while starting"))
    }

    pub fn remove_host(&self, host_id: &str) -> bool {
        let (removed, killed, snapshot) = {
            let mut reg = self.reg();
            let Registry { hosts, shells, .. } = &mut *reg;
            let Some(idx) = hosts.iter().position(|h| h.id == host_id) else {
                return false;
            };
            let removed_host = hosts.remove(idx);
            let killed = removed_host
                .job_id
                .as_deref()
                .and_then(|jid| shells.get_mut(jid))
                .and_then(|sh| sh.exit.is_none().then(|| sh.kill()).flatten());
            for sh in shells.values_mut() {
                if sh.host_id.as_deref() == Some(host_id) {
                    sh.host_id = None;
                }
            }
            (true, killed, hosts.clone())
        };
        if let Some(child) = killed {
            reap(child);
        }
        save_hosts_file(&self.sid, &snapshot);
        removed
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
    sid: String,
    id: String,
    mut stdout: ChildStdout,
    stderr: ChildStderr,
) {
    let (err_inner, err_session, err_id) = (inner.clone(), sid.clone(), id.clone());
    let mut err_task = drain_stderr(stderr, move |chunk| {
        append(&err_inner, &err_session, &err_id, chunk);
    });
    let mut buf = [0u8; 8192];
    let mut out_open = true;
    loop {
        tokio::select! {
            biased;
            n = stdout.read(&mut buf), if out_open => match n {
                Ok(0) | Err(_) => out_open = false,
                Ok(n) => append(&inner, &sid, &id, &buf[..n]),
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
            append(&inner, &sid, &id, &buf[..n]);
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

/// Strips ANSI escape sequences (CSI/OSC) so dev server banners like Vite/Next
/// yield clean URLs.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                for n in chars.by_ref() {
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            } else if matches!(chars.peek(), Some(']')) {
                chars.next();
                while let Some(n) = chars.next() {
                    if n == '\x07' || (n == '\x1b' && matches!(chars.peek(), Some('\\'))) {
                        if n == '\x1b' {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Extracts loopback http(s) URLs printed by local dev servers (localhost,
/// *.localhost, 127.x.x.x, [::1], and wildcard binds 0.0.0.0 / [::] normalized
/// to localhost so they open cleanly in a browser).
fn extract_loopback_urls(chunk: &str) -> Vec<String> {
    if !chunk.contains("://") {
        return Vec::new();
    }
    let clean = strip_ansi(chunk);
    let mut out = Vec::new();
    for raw in clean.split_whitespace() {
        let Some(pos) = raw.find("http://").or_else(|| raw.find("https://")) else {
            continue;
        };
        let candidate = raw[pos..].trim_end_matches(|c: char| {
            matches!(
                c,
                ',' | '.' | ';' | ')' | ']' | '}' | '"' | '\'' | '`' | '>'
            )
        });
        if let Some(url) = normalize_loopback_url(candidate) {
            if !out.contains(&url) {
                out.push(url);
            }
        }
    }
    out
}

fn normalize_loopback_url(raw: &str) -> Option<String> {
    let (scheme, rest) = raw.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let sep = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..sep];
    let tail = &rest[sep..];
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        let (h, after) = inner.split_once(']')?;
        let p = match after {
            "" => None,
            s => Some(s.strip_prefix(':')?),
        };
        (h, p)
    } else if authority.matches(':').count() == 1 {
        let (h, p) = authority.rsplit_once(':')?;
        (h, Some(p))
    } else if authority.contains(':') {
        return None;
    } else {
        (authority, None)
    };
    if let Some(p) = port {
        if p.is_empty() || p.parse::<u16>().is_err() {
            return None;
        }
    }
    let host_lc = host.to_ascii_lowercase();
    let normalized_host = if host_lc == "0.0.0.0" || host_lc == "::" {
        "localhost".to_string()
    } else if host_lc == "localhost"
        || host_lc.ends_with(".localhost")
        || host_lc == "::1"
        || host_lc
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.octets()[0] == 127)
    {
        if host_lc == "::1" {
            "[::1]".to_string()
        } else {
            host_lc
        }
    } else {
        return None;
    };
    let tail = if tail == "/" { "" } else { tail };
    Some(match port {
        Some(p) => format!("{scheme}://{normalized_host}:{p}{tail}"),
        None => format!("{scheme}://{normalized_host}{tail}"),
    })
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

    #[test]
    fn extracts_loopback_urls_from_server_output() {
        let vite = "\x1b[32m  ➜  Local:   \x1b[36mhttp://localhost:5173/\x1b[0m\n  ➜  Network: http://192.168.1.10:5173/\n";
        assert_eq!(extract_loopback_urls(vite), vec!["http://localhost:5173"]);

        let py = "Serving HTTP on http://0.0.0.0:8000/ and https://127.0.0.1:8443/app, ignore https://example.com";
        assert_eq!(
            extract_loopback_urls(py),
            vec!["http://localhost:8000", "https://127.0.0.1:8443/app"]
        );

        let v6 = "Listening on http://[::1]:3000/ and http://[::]:4000/";
        assert_eq!(
            extract_loopback_urls(v6),
            vec!["http://[::1]:3000", "http://localhost:4000"]
        );
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
        let mut sh = LiveShell::reserved("test", tmp(), None);
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

    #[tokio::test]
    async fn hosts_persist_restart_and_remove() {
        let sid = format!("test-hosts-{}", std::process::id());
        scrub(&sid);
        let cmd = if cfg!(windows) {
            "Write-Output 'Local: http://localhost:5199/'; Start-Sleep -Seconds 30"
        } else {
            "echo 'Local: http://localhost:5199/'; sleep 30"
        };
        let reg = ShellRegistry::new(&sid);
        let first_job = reg.start(cmd, &tmp()).unwrap();
        let mut found = Vec::new();
        for _ in 0..80 {
            found = reg.hosts();
            if !found.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(
            found.len(),
            1,
            "host should be discovered from server banner"
        );
        assert!(found[0].running);
        assert_eq!(found[0].urls, vec!["http://localhost:5199"]);
        assert_eq!(found[0].job_id.as_deref(), Some(first_job.as_str()));
        let host_id = found[0].id.clone();

        // Stopping keeps the host entry available for restart.
        assert!(reg.kill(&first_job));
        let stopped = reg.hosts();
        assert_eq!(stopped.len(), 1);
        assert!(!stopped[0].running);

        // Fresh registry (simulating Parzi restart) loads the persisted host.
        let saved = ShellRegistry::hosts_saved(&sid);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].id, host_id);
        assert!(!saved[0].running);

        let reg2 = ShellRegistry::new(&sid);
        let restarted = reg2.restart_host(&host_id).await.unwrap();
        assert!(restarted.running);
        assert_eq!(restarted.urls, vec!["http://localhost:5199"]);
        assert!(restarted.job_id.is_some());

        // Removing stops the live process and deletes the persisted host.
        assert!(reg2.remove_host(&host_id));
        assert!(reg2.hosts().is_empty());
        assert!(ShellRegistry::hosts_saved(&sid).is_empty());
        reg2.kill_all();
        scrub(&sid);
    }
}
