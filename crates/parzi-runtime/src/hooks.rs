use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use parzi_core::hooks::{HookDef, HookSet};
use parzi_core::store::SessionStore;
use serde_json::Value;

const REASON_CAP: usize = 500;

struct HookRun {
    code: Option<i32>,
    stderr: String,
    timed_out: bool,
}

fn shell(cmd: &str) -> tokio::process::Command {
    if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(cmd);
        #[cfg(windows)]
        c.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    }
}

fn scrubbed(
    cmd: &mut tokio::process::Command,
    session: &str,
    project: &str,
    tool: &str,
    event: &str,
) {
    cmd.env_clear();
    cmd.envs(crate::mcp::child_env(&std::collections::HashMap::new()));
    cmd.env("PARZI_SESSION", session);
    cmd.env("PARZI_PROJECT", project);
    cmd.env("PARZI_TOOL", tool);
    cmd.env("PARZI_EVENT", event);
}

async fn run_hook(
    def: &HookDef,
    cwd: &std::path::Path,
    payload: &Value,
    session: &str,
    project: &str,
    tool: &str,
    event: &str,
) -> HookRun {
    let mut cmd = shell(&def.command);
    cmd.current_dir(cwd);
    scrubbed(&mut cmd, session, project, tool, event);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    let body = payload.to_string();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return HookRun {
                code: None,
                stderr: e.to_string(),
                timed_out: false,
            }
        }
    };
    // A hook is a shell, so what it starts would outlive a plain kill of
    // the shell when it times out; the whole tree goes.
    let pid = child.id();
    let work = async {
        if let Some(stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt as _;
            let mut stdin = stdin;
            let _ = stdin.write_all(body.as_bytes()).await;
            let _ = stdin.shutdown().await;
        }
        child.wait_with_output().await.map_err(|e| e.to_string())
    };
    tokio::pin!(work);
    let done = tokio::select! {
        r = &mut work => Some(r),
        () = tokio::time::sleep(Duration::from_secs(def.timeout())) => None,
    };
    // Kill the tree while the shell still holds it: once the shell is gone
    // its children belong to no one and cannot be found.
    match done {
        None => {
            if let Some(pid) = pid {
                parzi_providers::process::kill_tree(pid);
            }
            HookRun {
                code: None,
                stderr: String::new(),
                timed_out: true,
            }
        }
        Some(Err(e)) => HookRun {
            code: None,
            stderr: e,
            timed_out: false,
        },
        Some(Ok(out)) => HookRun {
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            timed_out: false,
        },
    }
}

fn reason(mut s: String) -> String {
    if s.len() > REASON_CAP {
        let mut at = REASON_CAP;
        while !s.is_char_boundary(at) {
            at -= 1;
        }
        s.truncate(at);
        s.push('…');
    }
    if s.trim().is_empty() {
        "blocked by a workspace hook".into()
    } else {
        s
    }
}

fn hook_set() -> HookSet {
    type Cached = (PathBuf, Option<SystemTime>, u64, HookSet);
    static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
    let Ok(path) = parzi_core::paths::parzi_dir().map(|d| d.join("hooks.toml")) else {
        return HookSet::default();
    };
    let meta = std::fs::metadata(&path).ok();
    let mtime = meta.as_ref().and_then(|m| m.modified().ok());
    let len = meta.as_ref().map_or(0, std::fs::Metadata::len);
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((p, m, l, set)) = cache.as_ref() {
        if *p == path && *m == mtime && *l == len {
            return set.clone();
        }
    }
    let set = parzi_core::hooks::global();
    *cache = Some((path, mtime, len, set.clone()));
    set
}

fn scope_of(store: &SessionStore, session_id: &str) -> (String, std::path::PathBuf) {
    let meta = store.get(session_id).ok();
    let project = meta.as_ref().map(|m| m.project.clone()).unwrap_or_default();
    let cwd = meta
        .as_ref()
        .map(|m| m.cwd.trim().to_string())
        .filter(|c| !c.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| parzi_core::paths::parzi_dir().ok())
        .unwrap_or_else(|| std::env::temp_dir());
    (project, cwd)
}

pub async fn pre_tool(
    store: &SessionStore,
    session_id: &str,
    tool: &str,
    args: &Value,
) -> (Option<String>, Vec<String>) {
    let set = hook_set();
    let defs: Vec<&HookDef> = set.pre_tool.iter().filter(|d| d.matches(tool)).collect();
    if defs.is_empty() {
        return (None, vec![]);
    }
    let (project, cwd) = scope_of(store, session_id);
    let payload = serde_json::json!({
        "session": session_id,
        "project": project,
        "tool": tool,
        "args": args,
    });
    let mut warnings = Vec::new();
    for def in defs {
        let run = run_hook(def, &cwd, &payload, session_id, &project, tool, "pre_tool").await;
        if run.timed_out {
            warnings.push(format!(
                "pre-tool hook `{}` timed out after {}s — allowed",
                def.command,
                def.timeout()
            ));
            continue;
        }
        match run.code {
            Some(0) => {}
            Some(2) => return (Some(reason(run.stderr)), warnings),
            _ => warnings.push(format!(
                "pre-tool hook `{}` exited {} — allowed",
                def.command,
                run.code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "spawn failed".into())
            )),
        }
    }
    (None, warnings)
}

pub async fn post_tool(
    store: &SessionStore,
    session_id: &str,
    tool: &str,
    args: &Value,
    ok: bool,
    output: &str,
) -> Vec<String> {
    let set = hook_set();
    let defs: Vec<&HookDef> = set.post_tool.iter().filter(|d| d.matches(tool)).collect();
    if defs.is_empty() {
        return vec![];
    }
    let (project, cwd) = scope_of(store, session_id);
    let payload = serde_json::json!({
        "session": session_id,
        "project": project,
        "tool": tool,
        "args": args,
        "ok": ok,
        "output": output.chars().take(4000).collect::<String>(),
    });
    let mut notices = Vec::new();
    for def in defs {
        let run = run_hook(def, &cwd, &payload, session_id, &project, tool, "post_tool").await;
        if run.timed_out {
            notices.push(format!(
                "post-tool hook `{}` timed out after {}s",
                def.command,
                def.timeout()
            ));
        } else if !matches!(run.code, Some(0)) {
            notices.push(format!(
                "post-tool hook `{}` exited {}",
                def.command,
                run.code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "spawn failed".into())
            ));
        }
    }
    notices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_reasons_cut_on_a_char_boundary() {
        let r = reason(format!("x{}", "é".repeat(400)));
        assert!(r.ends_with('…'));
        assert!(r.len() <= REASON_CAP + '…'.len_utf8());
        assert_eq!(reason("  ".into()), "blocked by a workspace hook");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_hook_that_times_out_takes_what_it_started_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("grandchild.pid");
        let def: HookDef = serde_json::from_value(serde_json::json!({
            "match": "*",
            "command": format!("sleep 30 & echo $! > {}; wait", marker.display()),
            "timeout_secs": 1,
        }))
        .unwrap();
        let run = run_hook(
            &def,
            dir.path(),
            &serde_json::json!({}),
            "s",
            "p",
            "t",
            "pre",
        )
        .await;
        assert!(run.timed_out);
        let pid = std::fs::read_to_string(&marker).unwrap();
        // A killed process lingers as a zombie until it is reaped, which can
        // take a moment on a busy machine; a zombie runs nothing.
        let running = || {
            let out = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", pid.trim()])
                .output()
                .unwrap();
            let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
            !stat.is_empty() && !stat.starts_with('Z')
        };
        let mut alive = running();
        for _ in 0..30 {
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            alive = running();
        }
        assert!(
            !alive,
            "the hook's background sleep {} survived",
            pid.trim()
        );
    }
}
