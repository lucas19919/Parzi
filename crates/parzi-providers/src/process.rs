//! Finding and starting a vendor program. An npm install leaves a `.cmd`
//! launcher on PATH on Windows; where it points at a native binary, Parzi
//! runs that binary directly, so the process tree is one process and no
//! console window flashes.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Bytes of stderr kept for a failure message.
const STDERR_KEEP: usize = 16 * 1024;

/// The program to run for `program` (a bare name searched on PATH, or a
/// path, `~` allowed). `None` = not installed.
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
            // A bare extensionless file on Windows is the sh launcher npm
            // writes for Git Bash; the `.cmd` next to it is the real one.
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

/// npm's `cmd-shim` writes `"%dp0%\node_modules\…\bin\x.exe" %*` for
/// packages that ship a native binary. Returns that binary when it exists.
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

/// A running vendor program with piped stdio and the tail of its stderr.
pub struct Proc {
    pub child: Child,
    stderr: Arc<Mutex<String>>,
}

impl Proc {
    /// Start `program` in `cwd`. stdin/stdout are for the protocol; stderr
    /// is drained in the background so a chatty vendor never blocks.
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

    /// What the program printed on stderr so far (last 16 KiB).
    pub fn stderr(&self) -> String {
        self.stderr.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Kill the program and everything it started.
    pub async fn kill(&mut self) {
        #[cfg(windows)]
        if let Some(pid) = self.child.id() {
            use std::os::windows::process::CommandExt as _;
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), self.child.wait()).await;
    }
}

/// First line of `program args…` (a `--version` call), within 15 s.
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

/// All of stdout+stderr of `program args…`, within `secs`.
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
        // A shim that runs node has no native target: the shim itself runs.
        std::fs::write(&shim, "\"%_prog%\" \"%dp0%\\node_modules\\tool\\cli.js\" %*\r\n").unwrap();
        assert!(follow_npm_shim(&shim).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_program_is_not_installed() {
        assert!(resolve("parzi-no-such-program-xyz").is_none());
        assert!(resolve("").is_none());
    }
}
