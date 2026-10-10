//! The desk's door to ParziOS. All the work lives in
//! `parzi_runtime::remote`; this file keeps one live link and relays the
//! few ops the remote tab needs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parzi_runtime::remote::{self, Link, Progress, SetupOptions, Target, Transport, REMOTE_BIN};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct RemoteState {
    link: Mutex<Option<Arc<Link>>>,
    busy: AtomicBool,
}

#[derive(serde::Serialize)]
pub struct RemoteInfo {
    pub label: String,
    pub user: String,
    pub host: String,
    pub version: String,
    pub linked_at: i64,
    pub alive: bool,
}

fn info(saved: &remote::Saved, alive: bool) -> RemoteInfo {
    RemoteInfo {
        label: saved.target.label(),
        user: saved.target.user.clone(),
        host: match saved.target.port {
            Some(p) => format!("{}:{p}", saved.target.host),
            None => saved.target.host.clone(),
        },
        version: saved.version.clone(),
        linked_at: saved.linked_at,
        alive,
    }
}

/// The saved remote, if any. Never opens a connection.
#[tauri::command]
pub async fn remote_info(state: State<'_, RemoteState>) -> Result<Option<RemoteInfo>, String> {
    let alive = state.link.lock().await.as_ref().is_some_and(|l| l.alive());
    Ok(remote::load_saved().map(|s| info(&s, alive)))
}

struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Set a Linux machine up and link to it. Progress arrives as
/// `parzi://remote-setup` events. The password is used for this call only
/// and is never stored.
#[tauri::command]
pub async fn remote_setup(
    app: AppHandle,
    state: State<'_, RemoteState>,
    user: String,
    host: String,
    password: Option<String>,
    notes: bool,
) -> Result<RemoteInfo, String> {
    if state.busy.swap(true, Ordering::AcqRel) {
        return Err("a setup is already running".into());
    }
    let _busy = Busy(&state.busy);
    let target = Target::parse(&user, &host)?;
    let opts = SetupOptions {
        password: password.filter(|p| !p.is_empty()),
        notes,
        ..Default::default()
    };
    let emit = |p: Progress| {
        let _ = app.emit("parzi://remote-setup", &p);
    };
    let (saved, link) = remote::setup(target, opts, &emit).await?;
    *state.link.lock().await = Some(Arc::new(link));
    Ok(info(&saved, true))
}

async fn live_link(state: &RemoteState) -> Result<Arc<Link>, String> {
    let mut slot = state.link.lock().await;
    if let Some(link) = slot.as_ref().filter(|l| l.alive()) {
        return Ok(link.clone());
    }
    let saved = remote::load_saved().ok_or("no remote is set up")?;
    let link = Arc::new(Link::open(&Transport::ssh(&saved.target)).await?);
    *slot = Some(link.clone());
    Ok(link)
}

fn wait_for(op: &str) -> Duration {
    match op {
        "providers" | "doctor" => Duration::from_secs(100),
        "session.send" | "session.kill" | "session.delete" => Duration::from_secs(70),
        _ => Duration::from_secs(30),
    }
}

/// Relay one op to the remote engine. Only the ops the remote tab uses
/// pass; the engine checks everything again on its side.
#[tauri::command]
pub async fn remote_call(
    state: State<'_, RemoteState>,
    op: String,
    body: Option<Value>,
) -> Result<Value, String> {
    if !remote::OPS.contains(&op.as_str()) {
        return Err(format!("`{op}` is not a remote op"));
    }
    let link = live_link(&state).await?;
    link.call(&op, body.unwrap_or(Value::Null), wait_for(&op))
        .await
}

/// Install an agent on the remote, or sign in to it, in a terminal the
/// user watches: vendor installers and logins are interactive.
#[tauri::command]
pub async fn remote_agent(provider: String, action: String) -> Result<(), String> {
    let id = parzi_providers::canonical_id(&provider)
        .ok_or_else(|| format!("unknown agent {provider}"))?;
    let verb = match action.as_str() {
        "install" => "install",
        "login" => "login",
        other => return Err(format!("unknown action {other}")),
    };
    let saved = remote::load_saved().ok_or("no remote is set up")?;
    let args = Transport::ssh(&saved.target)
        .terminal_args(&format!("{REMOTE_BIN} agent {verb} {id}"))
        .ok_or("this remote has no terminal")?;
    let line = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    let script = if cfg!(windows) {
        format!("& {line}")
    } else {
        line
    };
    let name = parzi_providers::display_name(id);
    let title = if verb == "install" {
        format!("Install {name} on {}", saved.target.label())
    } else {
        format!("Sign in to {name} on {}", saved.target.label())
    };
    crate::onboard::open_terminal(&title, &script)
}

fn quote(arg: &str) -> String {
    if cfg!(windows) {
        let mut out = String::from("'");
        for c in arg.chars() {
            if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
                out.push(c);
            }
            out.push(c);
        }
        out.push('\'');
        out
    } else {
        remote::sh_quote(arg)
    }
}

/// Unlink: close the link and drop the saved remote. The engine on the
/// other machine keeps running.
#[tauri::command]
pub async fn remote_forget(state: State<'_, RemoteState>) -> Result<(), String> {
    *state.link.lock().await = None;
    remote::forget()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_arguments_are_quoted_for_the_local_shell() {
        if cfg!(windows) {
            assert_eq!(quote("it's"), "'it''s'");
            assert_eq!(quote("D\u{2019}Angelo"), "'D\u{2019}\u{2019}Angelo'");
            assert_eq!(
                quote("~/.local/bin/parzi agent login claude"),
                "'~/.local/bin/parzi agent login claude'"
            );
        } else {
            assert_eq!(quote("it's"), r"'it'\''s'");
        }
    }
}
