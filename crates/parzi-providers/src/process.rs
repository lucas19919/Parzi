use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const STDERR_KEEP: usize = 16 * 1024;

pub fn private_temp(agent: &str) -> Option<PathBuf> {
    let dir = parzi_core::paths::parzi_dir().ok()?.join("tmp").join(agent);
    std::fs::create_dir_all(&dir).ok()?;
    sweep_unpack_dirs(&dir);
    Some(dir)
}

pub fn sweep_unpack_dirs(dir: &Path) {
    if !cfg!(windows) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("_MEI") || !e.path().is_dir() {
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

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
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
    for dir in std::env::split_paths(&path) {
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

pub struct Proc {
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
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in env {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        let mut child = cmd.spawn()?;
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
        self.kill_tree();
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), self.child.wait()).await;
    }

    fn kill_tree(&mut self) {
        let Some(pid) = self.child.id() else {
            return;
        };
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            let _ = std::process::Command::new("taskkill")
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
}

impl Drop for Proc {
    fn drop(&mut self) {
        self.kill_tree();
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

pub async fn first_line(program: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = tokio::time::timeout(std::time::Duration::from_secs(15), cmd.output())
        .await
        .ok()?
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

pub async fn output(program: &Path, args: &[&str], secs: u64) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = tokio::time::timeout(std::time::Duration::from_secs(secs), cmd.output())
        .await
        .ok()?
        .ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(text)
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
        for _ in 0..150 {
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
        sweep_unpack_dirs(&dir);
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
