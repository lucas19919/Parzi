use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::types::{ErrorClass, ProviderError};

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const STDERR_KEEP: usize = 16 * 1024;
const UNPACK_MIN_AGE: Duration = Duration::from_secs(10 * 60);

static ADOPT: OnceLock<fn(u32)> = OnceLock::new();

pub fn on_spawn(adopt: fn(u32)) {
    let _ = ADOPT.set(adopt);
}

pub fn adopt(child: &Child) {
    if let (Some(adopt), Some(pid)) = (ADOPT.get(), child.id()) {
        adopt(pid);
    }
}

/// Kill a process and its whole descendant tree (best effort).
/// Windows uses taskkill /T /F; unix walks `ps` and kills descendants.
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // Full path: a `taskkill.exe` planted next to the app or on PATH
        // must never run in its place.
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let taskkill = std::path::Path::new(&root)
            .join("System32")
            .join("taskkill.exe");
        let _ = std::process::Command::new(taskkill)
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        let tree = descendants(pid);
        if !tree.is_empty() {
            let _ = std::process::Command::new("kill")
                .arg("-KILL")
                .args(tree.iter().map(u32::to_string))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

// The root too: on unix kill_tree leaves it to the caller.
fn kill_all(pid: u32) {
    kill_tree(pid);
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// The harness shell: pwsh when present, else powershell, else sh.
/// Returns the program plus the argument prefix placed before the command.
pub fn shell_program() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        if resolve("pwsh").is_some() {
            return (
                "pwsh".into(),
                ["-NoProfile", "-NonInteractive", "-Command"]
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
            );
        }
        (
            "powershell".into(),
            ["-NoProfile", "-NonInteractive", "-Command"]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        )
    }
    #[cfg(not(windows))]
    {
        ("sh".into(), vec!["-c".into()])
    }
}

pub(crate) fn private_temp(agent: &str) -> Option<PathBuf> {
    let dir = parzi_core::paths::parzi_dir().ok()?.join("tmp").join(agent);
    std::fs::create_dir_all(&dir).ok()?;
    sweep_unpack_dirs(&dir, UNPACK_MIN_AGE);
    Some(dir)
}

pub(crate) fn existing_folder(cwd: &Path) -> Result<(), ProviderError> {
    if cwd.is_dir() {
        return Ok(());
    }
    Err(ProviderError::new(
        ErrorClass::BadRequest,
        format!("the folder {} does not exist", cwd.display()),
    ))
}

// A young folder may belong to an agent still unpacking itself.
fn sweep_unpack_dirs(dir: &Path, min_age: Duration) {
    if !cfg!(windows) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("_MEI") || !e.path().is_dir() {
            continue;
        }
        let age = e
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| now.duration_since(t).unwrap_or(Duration::ZERO));
        if !age.is_ok_and(|a| a >= min_age) {
            continue;
        }
        let dead = dir.join(format!("_gone{}", &name[4..]));
        if std::fs::rename(e.path(), &dead).is_ok() {
            let _ = std::fs::remove_dir_all(&dead);
        }
    }
}

pub fn resolve(program: &str) -> Option<PathBuf> {
    let program = program.trim();
    if program.is_empty() {
        return None;
    }
    let found = if program.contains('/') || program.contains('\\') {
        let p = expand_home(program);
        p.is_file().then_some(p)
    } else {
        find_on_path(program)
    }?;
    Some(follow_npm_shim(&found).unwrap_or(found))
}

fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        Some(rest) => dirs::home_dir().map_or_else(|| PathBuf::from(p), |h| h.join(rest)),
        None => PathBuf::from(p),
    }
}

fn install_dirs() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    let mut out = vec![
        home.join(".local").join("bin"),
        home.join(".grok").join("bin"),
    ];
    if cfg!(windows) {
        if let Some(roaming) = dirs::data_dir() {
            out.push(roaming.join("npm"));
        }
        if let Some(local) = dirs::data_local_dir() {
            out.push(local.join("cursor-agent"));
        }
    } else {
        out.push(PathBuf::from("/opt/homebrew/bin"));
        out.push(PathBuf::from("/usr/local/bin"));
    }
    out
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let search: Vec<PathBuf> = std::env::split_paths(&path).chain(install_dirs()).collect();
    let exts: Vec<String> = if cfg!(windows) {
        let pathext =
            std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        std::iter::once(String::new())
            .chain(pathext.split(';').map(|e| e.to_ascii_lowercase()))
            .filter(|e| e.is_empty() || e.starts_with('.'))
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in search {
        for ext in &exts {
            if cfg!(windows) && ext.is_empty() && Path::new(name).extension().is_none() {
                continue;
            }
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn follow_npm_shim(path: &Path) -> Option<PathBuf> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    if ext != "cmd" && ext != "bat" {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let dir = path.parent()?;
    for marker in ["\"%dp0%\\", "\"%~dp0\\"] {
        for (i, _) in text.match_indices(marker) {
            let rest = &text[i + marker.len()..];
            let Some(end) = rest.find('"') else { continue };
            let rel = &rest[..end];
            if rel.to_ascii_lowercase().ends_with(".exe") {
                let exe = dir.join(rel.replace('\\', std::path::MAIN_SEPARATOR_STR));
                if exe.is_file() {
                    return Some(exe);
                }
            }
        }
    }
    None
}

pub(crate) struct Proc {
    pub child: Child,
    stderr: Arc<Mutex<String>>,
}

impl Proc {
    pub fn spawn(
        program: &Path,
        args: &[String],
        cwd: &Path,
        env: &[(String, String)],
    ) -> std::io::Result<Self> {
        let mut cmd = Command::new(program);
        // Drop kills root and tree off-thread; tokio killing the root first
        // would cut the tree walk short.
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false);
        for (k, v) in env {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        let mut child = cmd.spawn()?;
        adopt(&child);
        let stderr = Arc::new(Mutex::new(String::new()));
        if let Some(mut pipe) = child.stderr.take() {
            let sink = stderr.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    if let Ok(mut s) = sink.lock() {
                        s.push_str(&String::from_utf8_lossy(&buf[..n]));
                        if s.len() > STDERR_KEEP {
                            let mut cut = s.len() - STDERR_KEEP;
                            while !s.is_char_boundary(cut) {
                                cut += 1;
                            }
                            s.drain(..cut);
                        }
                    }
                }
            });
        }
        Ok(Self { child, stderr })
    }

    pub fn stderr(&self) -> String {
        self.stderr.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub async fn kill(&mut self) {
        if let Some(pid) = self.child.id() {
            let _ = tokio::task::spawn_blocking(move || kill_tree(pid)).await;
        }
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let Some(pid) = self.child.id() else {
            return;
        };
        // taskkill and ps are slow; never block whoever drops this.
        let spawned = std::thread::Builder::new()
            .name("parzi-kill".into())
            .spawn(move || kill_all(pid));
        if spawned.is_err() {
            kill_all(pid);
        }
    }
}

#[cfg(unix)]
fn descendants(root: u32) -> Vec<u32> {
    let Ok(out) = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid="])
        .stderr(Stdio::null())
        .output()
    else {
        return vec![];
    };
    let mut children: std::collections::HashMap<u32, Vec<u32>> = Default::default();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut it = line.split_whitespace().map(str::parse::<u32>);
        if let (Some(Ok(pid)), Some(Ok(ppid))) = (it.next(), it.next()) {
            children.entry(ppid).or_default().push(pid);
        }
    }
    let mut tree = vec![];
    let mut next = vec![root];
    while let Some(p) = next.pop() {
        for &c in children.get(&p).into_iter().flatten() {
            if c != root && !tree.contains(&c) {
                tree.push(c);
                next.push(c);
            }
        }
    }
    tree
}

pub const STOP_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

// Stdout and stderr of a short command; on timeout its whole tree dies,
// not just the direct child.
async fn probe(program: &Path, args: &[&str], secs: u64) -> Option<(String, String)> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().ok()?;
    adopt(&child);
    let pid = child.id();
    let (mut stdout, mut stderr) = (child.stdout.take()?, child.stderr.take()?);
    let run = async {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let _ = tokio::join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err));
        let _ = child.wait().await;
        (out, err)
    };
    if let Ok((out, err)) = tokio::time::timeout(Duration::from_secs(secs), run).await {
        return Some((
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        ));
    }
    if let Some(pid) = pid {
        let _ = tokio::task::spawn_blocking(move || kill_probe(pid)).await;
    }
    let _ = child.start_kill();
    None
}

fn kill_probe(pid: u32) {
    #[cfg(windows)]
    kill_tree(pid);
    // The probe leads its own group, so one signal reaches every descendant.
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

pub(crate) async fn first_line(program: &Path, args: &[&str]) -> Option<String> {
    let (out, _) = probe(program, args, 15).await?;
    out.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

pub(crate) async fn output(program: &Path, args: &[&str], secs: u64) -> Option<String> {
    let (mut out, err) = probe(program, args, secs).await?;
    out.push_str(&err);
    Some(out)
}

pub(crate) async fn stdout(program: &Path, args: &[&str], secs: u64) -> Option<String> {
    probe(program, args, secs).await.map(|(out, _)| out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_shim_leads_to_the_native_binary() {
        let dir = std::env::temp_dir().join(format!("parzi-shim-{}", std::process::id()));
        let bin = dir.join("node_modules").join("tool").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("tool.exe"), b"").unwrap();
        let shim = dir.join("tool.cmd");
        std::fs::write(
            &shim,
            "@ECHO off\r\n:start\r\n\"%dp0%\\node_modules\\tool\\bin\\tool.exe\"   %*\r\n",
        )
        .unwrap();
        let got = follow_npm_shim(&shim).expect("shim followed");
        assert!(got.ends_with(Path::new("node_modules/tool/bin/tool.exe")));
        std::fs::write(
            &shim,
            "\"%_prog%\" \"%dp0%\\node_modules\\tool\\cli.js\" %*\r\n",
        )
        .unwrap();
        assert!(follow_npm_shim(&shim).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_program_is_not_installed() {
        assert!(resolve("parzi-no-such-program-xyz").is_none());
        assert!(resolve("").is_none());
    }

    #[test]
    fn a_missing_folder_is_named() {
        let gone = std::env::temp_dir().join(format!("parzi-gone-{}", uuid::Uuid::new_v4()));
        let err = existing_folder(&gone).unwrap_err();
        assert_eq!(err.class, ErrorClass::BadRequest);
        assert!(
            err.message.starts_with("the folder") && err.message.ends_with("does not exist"),
            "{err}"
        );
        assert!(existing_folder(&std::env::temp_dir()).is_ok());
    }

    #[tokio::test]
    async fn dropping_a_program_kills_what_it_started() {
        let dir = std::env::temp_dir().join(format!("parzi-tree-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("child.pid");
        #[cfg(windows)]
        let (program, script) = (
            PathBuf::from("powershell"),
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                format!(
                    "$p = Start-Process -PassThru -WindowStyle Hidden ping -ArgumentList '-n','60','127.0.0.1'; \
                     Set-Content -Path '{}' -Value $p.Id; Wait-Process -Id $p.Id",
                    pidfile.display()
                ),
            ],
        );
        #[cfg(unix)]
        let (program, script) = (
            PathBuf::from("sh"),
            vec![
                "-c".to_string(),
                format!("sleep 60 & echo $! > '{}'; wait", pidfile.display()),
            ],
        );
        let proc = Proc::spawn(&program, &script, &dir, &[]).unwrap();
        let mut pid = None;
        for _ in 0..300 {
            pid = std::fs::read_to_string(&pidfile)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok());
            if pid.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let pid = pid.expect("the program started its own child");
        assert!(alive(pid), "the child runs while its parent does");
        drop(proc);
        let mut gone = false;
        for _ in 0..50 {
            if !alive(pid) {
                gone = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(gone, "process {pid} outlived the program that started it");
    }

    #[cfg(windows)]
    #[test]
    fn sweep_removes_abandoned_unpack_dirs_and_keeps_held_ones() {
        use std::os::windows::fs::OpenOptionsExt as _;
        let dir = std::env::temp_dir().join(format!("parzi-sweep-{}", uuid::Uuid::new_v4()));
        let gone = dir.join("_MEI1111");
        let held = dir.join("_MEI2222");
        let other = dir.join("keep-me");
        for d in [&gone, &held, &other] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("x.pyd"), b"x").unwrap();
        }
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(held.join("x.pyd"))
            .unwrap();
        sweep_unpack_dirs(&dir, UNPACK_MIN_AGE);
        assert!(gone.exists(), "a folder still being unpacked was swept");
        sweep_unpack_dirs(&dir, Duration::ZERO);
        assert!(!gone.exists(), "abandoned unpack folder was kept");
        assert!(held.join("x.pyd").exists(), "a folder in use was touched");
        assert!(
            other.exists(),
            "a folder that is not an unpack folder was touched"
        );
        drop(lock);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn alive(pid: u32) -> bool {
        #[cfg(windows)]
        {
            let out = std::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            out.contains(&format!("\"{pid}\""))
        }
        #[cfg(unix)]
        {
            std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        }
    }
}
