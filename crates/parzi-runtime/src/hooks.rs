//! Workspace event hooks, executed: user scripts watching tool calls.
//!
//! `pre_tool` hooks run before the approval gate — automation first. Exit 0
//! allows, exit 2 denies (stderr becomes the reason, capped), anything else
//! or a timeout allows with a warning notice. `post_tool` hooks are
//! notify-only: failures surface as notices, never as tool errors.
//!
//! Children get the H-6 scrubbed environment (PATH and system vars only —
//! never provider keys) plus `PARZI_SESSION`, `PARZI_PROJECT`, `PARZI_TOOL`,
//! `PARZI_EVENT`, and the event JSON on stdin. The working directory is the
//! workspace dir, the legacy project dir, or the Parzi home for global-only
//! runs.

use std::process::Stdio;
use std::time::Duration;

use parzi_core::store::SessionStore;
use serde_json::Value;

/// Cap for stderr reasons surfaced to the transcript.
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
    // H-6, same posture as shell.exec and MCP children.
    cmd.env_clear();
    for (k, v) in [
        ("PATH", std::env::var("PATH").unwrap_or_default()),
        (
            "SYSTEMROOT",
            std::env::var("SYSTEMROOT").unwrap_or_default(),
        ),
        ("TEMP", std::env::var("TEMP").unwrap_or_default()),
        ("TMP", std::env::var("TMP").unwrap_or_default()),
        ("TMPDIR", std::env::var("TMPDIR").unwrap_or_default()),
        ("HOME", std::env::var("HOME").unwrap_or_default()),
        ("APPDATA", std::env::var("APPDATA").unwrap_or_default()),
        (
            "USERPROFILE",
            std::env::var("USERPROFILE").unwrap_or_default(),
        ),
        ("LANG", std::env::var("LANG").unwrap_or_default()),
        ("LC_ALL", std::env::var("LC_ALL").unwrap_or_default()),
    ] {
        if !v.is_empty() {
            cmd.env(k, v);
        }
    }
    cmd.env("PARZI_SESSION", session);
    cmd.env("PARZI_PROJECT", project);
    cmd.env("PARZI_TOOL", tool);
    cmd.env("PARZI_EVENT", event);
}

async fn run_hook(
    def: &parzi_core::hooks::HookDef,
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
    let body = payload.to_string();
    let work = async {
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        if let Some(stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt as _;
            let mut stdin = stdin;
            let _ = stdin.write_all(body.as_bytes()).await;
            let _ = stdin.shutdown().await;
        }
        child.wait_with_output().await.map_err(|e| e.to_string())
    };
    match tokio::time::timeout(Duration::from_secs(def.timeout()), work).await {
        Err(_) => HookRun {
            code: None,
            stderr: String::new(),
            timed_out: true,
        },
        Ok(Err(e)) => HookRun {
            code: None,
            stderr: e,
            timed_out: false,
        },
        Ok(Ok(out)) => HookRun {
            code: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            timed_out: false,
        },
    }
}

fn reason(mut s: String) -> String {
    if s.len() > REASON_CAP {
        s.truncate(REASON_CAP);
        s.push('…');
    }
    if s.trim().is_empty() {
        "blocked by a workspace hook".into()
    } else {
        s
    }
}

fn scope_of(store: &SessionStore, session_id: &str) -> (String, std::path::PathBuf) {
    let project = store.get(session_id).map(|m| m.project).unwrap_or_default();
    let cwd = if parzi_core::workspace::load(&project).is_ok() {
        parzi_core::workspace::dir(&project)
    } else if parzi_core::lanes::safe_name(&project).is_ok() {
        parzi_core::paths::projects_dir()
            .map(|root| root.join(&project))
            .unwrap_or_else(|_| std::env::temp_dir())
    } else {
        parzi_core::paths::parzi_dir().unwrap_or_else(|_| std::env::temp_dir())
    };
    (project, cwd)
}

/// Run matching `pre_tool` hooks in file order. Returns the denial reason
/// when one fires, plus warnings (timeouts, crashes) for the transcript.
pub async fn pre_tool(
    store: &SessionStore,
    session_id: &str,
    tool: &str,
    args: &Value,
) -> (Option<String>, Vec<String>) {
    let (project, cwd) = scope_of(store, session_id);
    let set = parzi_core::hooks::for_scope(&project);
    let payload = serde_json::json!({
        "session": session_id,
        "project": project,
        "tool": tool,
        "args": args,
    });
    let mut warnings = Vec::new();
    for def in set.pre_tool.iter().filter(|d| d.matches(tool)) {
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

/// Run matching `post_tool` hooks. Returns notices for failed hooks.
pub async fn post_tool(
    store: &SessionStore,
    session_id: &str,
    tool: &str,
    args: &Value,
    ok: bool,
    output: &str,
) -> Vec<String> {
    let (project, cwd) = scope_of(store, session_id);
    let set = parzi_core::hooks::for_scope(&project);
    let payload = serde_json::json!({
        "session": session_id,
        "project": project,
        "tool": tool,
        "args": args,
        "ok": ok,
        "output": output.chars().take(4000).collect::<String>(),
    });
    let mut notices = Vec::new();
    for def in set.post_tool.iter().filter(|d| d.matches(tool)) {
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
