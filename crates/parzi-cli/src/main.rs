use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use parzi_core::config::ParziConfig;
use parzi_core::paths;
use parzi_core::store::SessionStore;
use parzi_providers::State;
use parzi_runtime::desk::{self, normalize_url};
use parzi_runtime::tools::{Approval, Approver, AutoApprover, ToolCallInfo};
use parzi_runtime::{handler::RunEvent, Orchestrator};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "parzi", version, about = "Lean agent harness")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(about = "Create ~/.parzi (never overwrites)")]
    Init,
    #[command(about = "Health: config, agents, the window")]
    Doctor {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Where each agent stands. Spends no quota")]
    Providers {
        #[arg(help = "claude, codex, opencode, grok, antigravity, cursor")]
        provider: Option<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Sessions")]
    Session {
        #[command(subcommand)]
        action: SessionCmd,
    },
    #[command(about = "Tabs in the open window")]
    Tab {
        #[command(subcommand)]
        action: TabCmd,
    },
}

#[derive(Subcommand)]
enum SessionCmd {
    #[command(about = "Newest first")]
    List {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Metadata and the transcript")]
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Transcript to stdout")]
    Export { id: String },
    #[command(about = "`new` starts a session. An id continues one")]
    Send {
        target: String,
        message: Vec<String>,
        #[arg(long, help = "Agent and model: `claude`, `claude/opus`")]
        model: Option<String>,
        #[arg(
            long,
            help = "Folder the agent works in. Default: the current directory"
        )]
        cwd: Option<String>,
        #[arg(long, help = "Approve tool calls without asking")]
        yes: bool,
        #[arg(
            long,
            default_value = "medium",
            help = "low | medium | high | extra | ultra, or the agent's own word"
        )]
        effort: String,
    },
    #[command(about = "Fork a transcript into a new session")]
    Fork {
        id: String,
        #[arg(long)]
        at: Option<usize>,
    },
    #[command(about = "Cancel a live run")]
    Kill { id: String },
    #[command(about = "Rename a session")]
    Rename { id: String, title: String },
    #[command(about = "Delete a session and its transcript")]
    Delete { id: String },
}

#[derive(Subcommand)]
enum TabCmd {
    List {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Open a URL, or focus the tab that already has it")]
    Open {
        url: String,
    },
    Focus {
        id: String,
    },
    Close {
        id: String,
    },
}

struct CliApprover;

#[async_trait::async_trait]
impl Approver for CliApprover {
    async fn approve(&self, call: &ToolCallInfo) -> Approval {
        eprintln!("tool `{}`", call.name);
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&call.args).unwrap_or_default()
        );
        eprintln!("allow? [y/N] ");
        use std::io::Write;
        let _ = std::io::stderr().flush();
        let ans = tokio::task::spawn_blocking(|| {
            let mut s = String::new();
            let _ = std::io::stdin().read_line(&mut s);
            s
        })
        .await
        .unwrap_or_default();
        if ans.trim().eq_ignore_ascii_case("y") {
            Approval::Allow
        } else {
            Approval::Deny
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_writer(std::io::stderr)
        .init();
    match cli.cmd {
        Cmd::Init => cmd_init(),
        Cmd::Doctor { json } => cmd_doctor(json).await,
        Cmd::Providers { provider, json } => cmd_providers(provider.as_deref(), json).await,
        Cmd::Session { action } => cmd_session(action).await,
        Cmd::Tab { action } => cmd_tab(action).await,
    }
}

fn boot() -> Result<(ParziConfig, SessionStore)> {
    paths::ensure_dirs().context("creating ~/.parzi")?;
    let cfg = ParziConfig::load().context("loading config")?;
    let store = SessionStore::open().context("opening sessions")?;
    Ok((cfg, store))
}

fn cmd_init() -> Result<()> {
    let root = paths::ensure_dirs().context("creating ~/.parzi")?;
    let cfg = ParziConfig::load().unwrap_or_default();
    cfg.save().context("writing config")?;
    let theme = parzi_core::theme::Theme::load().unwrap_or_default();
    theme.save().context("writing theme")?;
    println!("parzi home: {}", root.display());
    Ok(())
}

fn offline(err: &str) -> bool {
    err.contains("not open")
}

async fn window(op: &str, body: Value, wait: Duration) -> Result<Value, String> {
    desk::call_within(op, body, wait).await
}

fn print_json(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

async fn cmd_session(action: SessionCmd) -> Result<()> {
    match action {
        SessionCmd::List { json } => session_list(json).await,
        SessionCmd::Show { id, json } => session_show(&id, json, false).await,
        SessionCmd::Export { id } => session_show(&id, false, true).await,
        SessionCmd::Send {
            target,
            message,
            model,
            cwd,
            yes,
            effort,
        } => {
            session_send(
                &target,
                &message.join(" "),
                model.as_deref(),
                cwd.as_deref(),
                yes,
                &effort,
            )
            .await
        }
        SessionCmd::Fork { id, at } => session_fork(&id, at).await,
        SessionCmd::Kill { id } => session_kill(&id).await,
        SessionCmd::Rename { id, title } => session_rename(&id, &title).await,
        SessionCmd::Delete { id } => session_delete(&id).await,
    }
}

async fn session_list(json: bool) -> Result<()> {
    match window("session.list", json!({}), Duration::from_secs(5)).await {
        Ok(v) => {
            if json {
                return print_json(&v);
            }
            let rows = v.get("sessions").and_then(Value::as_array);
            let Some(rows) = rows else {
                println!("{v}");
                return Ok(());
            };
            if rows.is_empty() {
                println!("no sessions yet — `parzi session send new \"hello\"`");
                return Ok(());
            }
            for row in rows {
                let id = row.get("id").and_then(Value::as_str).unwrap_or("");
                let status = row.get("status").and_then(Value::as_str).unwrap_or("");
                let model = row.get("model").and_then(Value::as_str).unwrap_or("");
                let title = row.get("title").and_then(Value::as_str).unwrap_or("");
                println!("{}  {}  {}  {}", short(id), status, model, title);
            }
            Ok(())
        }
        Err(e) if offline(&e) => list_local(json),
        Err(e) => anyhow::bail!(e),
    }
}

fn list_local(json: bool) -> Result<()> {
    let (_, store) = boot()?;
    let all = store.list().context("listing sessions")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }
    if all.is_empty() {
        println!("no sessions yet — `parzi session send new \"hello\"`");
        return Ok(());
    }
    for m in all {
        println!("{}  {:?}  {}  {}", short(&m.id), m.status, m.model, m.title);
    }
    Ok(())
}

async fn session_show(id: &str, json: bool, export: bool) -> Result<()> {
    let op = if export {
        "session.export"
    } else {
        "session.show"
    };
    match window(op, json!({ "id": id }), Duration::from_secs(5)).await {
        Ok(v) => {
            if export {
                println!("{}", v.get("text").and_then(Value::as_str).unwrap_or(""));
                return Ok(());
            }
            if json {
                return print_json(&v);
            }
            println!("{}", v.get("text").and_then(Value::as_str).unwrap_or(""));
            Ok(())
        }
        Err(e) if offline(&e) => {
            let (_, store) = boot()?;
            let id = resolve_id(&store, id)?;
            if export || !json {
                let md =
                    std::fs::read_to_string(paths::sessions_dir()?.join(&id).join("session.md"))
                        .context("reading transcript")?;
                println!("{md}");
                return Ok(());
            }
            let meta = store.get(&id).context("reading session")?;
            println!("{}", serde_json::to_string_pretty(&meta)?);
            Ok(())
        }
        Err(e) => anyhow::bail!(e),
    }
}

async fn session_send(
    target: &str,
    message: &str,
    model: Option<&str>,
    cwd: Option<&str>,
    yes: bool,
    effort: &str,
) -> Result<()> {
    if message.trim().is_empty() {
        anyhow::bail!("empty message");
    }
    let base = match cwd.filter(|c| !c.trim().is_empty()) {
        Some(c) => std::path::PathBuf::from(c),
        None => std::env::current_dir().context("reading the current directory")?,
    };
    let body = json!({
        "target": target,
        "message": message,
        "model": model.unwrap_or_default(),
        "cwd": base.display().to_string(),
        "yes": yes,
        "effort": effort,
    });
    match window("session.send", body, Duration::from_secs(3600)).await {
        Ok(v) => {
            if let Some(id) = v.get("id").and_then(Value::as_str) {
                eprintln!("session {id}");
            }
            println!("{}", v.get("text").and_then(Value::as_str).unwrap_or(""));
            Ok(())
        }
        Err(e) if offline(&e) => send_local(target, message, model, &base, yes, effort).await,
        Err(e) => anyhow::bail!(e),
    }
}

async fn send_local(
    target: &str,
    message: &str,
    model: Option<&str>,
    cwd: &std::path::Path,
    yes: bool,
    effort: &str,
) -> Result<()> {
    let (mut cfg, store) = boot()?;
    let continued = if target == "new" {
        None
    } else {
        Some(resolve_id(&store, target)?)
    };
    cfg.orchestrator.queue_when_busy = false;
    let orch = Orchestrator::new(cfg, store);
    orch.recover().ok();
    let approver: Arc<dyn Approver> = if yes {
        Arc::new(AutoApprover)
    } else {
        Arc::new(CliApprover)
    };
    let cwd = cwd.display().to_string();
    let mut rx = if let Some(id) = continued {
        orch.send_to(
            &id,
            message,
            Some(approver),
            &cwd,
            effort,
            vec![],
            model.map(str::to_string),
            None,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
    } else {
        let (meta, rx) = orch
            .spawn(
                "default",
                "",
                model.unwrap_or_default(),
                message,
                Some(approver),
                &cwd,
                effort,
                vec![],
                None,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        eprintln!("session {}", meta.id);
        rx
    };
    while let Some(ev) = rx.recv().await {
        match ev {
            RunEvent::Text(t) => {
                use std::io::Write;
                print!("{t}");
                let _ = std::io::stdout().flush();
            }
            RunEvent::ToolCall { name, label, .. } => eprintln!("\n[tool] {name} — {label}"),
            RunEvent::ToolResult { name, ok, ms, .. } => {
                eprintln!("[result] {name} ok={ok} {ms}ms");
            }
            RunEvent::Usage {
                tokens_in,
                tokens_out,
                cost_usd,
            } => eprintln!("\n[usage] {tokens_in} in / {tokens_out} out (${cost_usd:.4})"),
            RunEvent::Notice { text } => eprintln!("\n[notice] {text}"),
            RunEvent::Reasoning { text } => eprint!("{text}"),
            RunEvent::Done { turns } => {
                eprintln!("\ndone in {turns} turns");
                break;
            }
            RunEvent::Error(e) => {
                eprintln!("\nerror: {e}");
                break;
            }
            _ => {}
        }
    }
    Ok(())
}

async fn session_fork(id: &str, at: Option<usize>) -> Result<()> {
    let body = json!({ "id": id, "at": at });
    match window("session.fork", body, Duration::from_secs(5)).await {
        Ok(v) => {
            println!(
                "forked -> {}",
                v.get("id").and_then(Value::as_str).unwrap_or("")
            );
            Ok(())
        }
        Err(e) if offline(&e) => {
            let (_, store) = boot()?;
            let id = resolve_id(&store, id)?;
            let meta = store.fork(&id, at).context("forking session")?;
            println!("forked -> {}", meta.id);
            Ok(())
        }
        Err(e) => anyhow::bail!(e),
    }
}

async fn session_kill(id: &str) -> Result<()> {
    match window("session.kill", json!({ "id": id }), Duration::from_secs(5)).await {
        Ok(v) => {
            println!(
                "killed {}",
                v.get("id").and_then(Value::as_str).unwrap_or(id)
            );
            Ok(())
        }
        Err(e) if offline(&e) => {
            let (cfg, store) = boot()?;
            let id = resolve_id(&store, id)?;
            Orchestrator::new(cfg, store)
                .kill(&id)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("killed {id}");
            Ok(())
        }
        Err(e) => anyhow::bail!(e),
    }
}

async fn session_rename(id: &str, title: &str) -> Result<()> {
    match window(
        "session.rename",
        json!({ "id": id, "title": title }),
        Duration::from_secs(5),
    )
    .await
    {
        Ok(_) => {
            println!("renamed {id}");
            Ok(())
        }
        Err(e) if offline(&e) => {
            let (_, store) = boot()?;
            let id = resolve_id(&store, id)?;
            store.set_title(&id, title).context("renaming")?;
            println!("renamed {id}");
            Ok(())
        }
        Err(e) => anyhow::bail!(e),
    }
}

async fn session_delete(id: &str) -> Result<()> {
    match window(
        "session.delete",
        json!({ "id": id }),
        Duration::from_secs(5),
    )
    .await
    {
        Ok(v) => {
            println!(
                "deleted {}",
                v.get("id").and_then(Value::as_str).unwrap_or(id)
            );
            Ok(())
        }
        Err(e) if offline(&e) => {
            let (_, store) = boot()?;
            let id = resolve_id(&store, id)?;
            store.delete_thread(&id).context("deleting")?;
            println!("deleted {id}");
            Ok(())
        }
        Err(e) => anyhow::bail!(e),
    }
}

async fn cmd_tab(action: TabCmd) -> Result<()> {
    let (op, body) = match action {
        TabCmd::List { json } => {
            let v = window("tab.list", json!({}), Duration::from_secs(5))
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            if json {
                return print_json(&v);
            }
            let rows = v
                .get("tabs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let active = v.get("active").and_then(Value::as_str).unwrap_or("");
            if rows.is_empty() {
                println!("no tabs");
                return Ok(());
            }
            for row in rows {
                let id = row.get("id").and_then(Value::as_str).unwrap_or("");
                let kind = row.get("kind").and_then(Value::as_str).unwrap_or("");
                let title = row.get("title").and_then(Value::as_str).unwrap_or("");
                let url = row.get("url").and_then(Value::as_str).unwrap_or("");
                let mark = if id == active { "*" } else { " " };
                println!("{mark} {id}  {kind}  {title}  {url}");
            }
            return Ok(());
        }
        TabCmd::Open { url } => ("tab.open", json!({ "url": normalize_url(&url) })),
        TabCmd::Focus { id } => ("tab.focus", json!({ "id": id })),
        TabCmd::Close { id } => ("tab.close", json!({ "id": id })),
    };
    let v = window(op, body, Duration::from_secs(5))
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    print_json(&v)
}

async fn cmd_doctor(json: bool) -> Result<()> {
    let (cfg, _) = boot()?;
    let checks = parzi_runtime::doctor::Doctor::new(cfg).run().await;
    if json {
        println!("{}", serde_json::to_string_pretty(&checks)?);
        return Ok(());
    }
    let mut bad = 0;
    for c in &checks {
        println!(
            "{} {} — {}",
            if c.ok { "ok  " } else { "FAIL" },
            c.name,
            c.detail
        );
        if !c.ok {
            bad += 1;
        }
    }
    if bad > 0 {
        anyhow::bail!("{bad} failing checks");
    }
    Ok(())
}

async fn cmd_providers(only: Option<&str>, json: bool) -> Result<()> {
    let (cfg, store) = boot()?;
    let ids = match only {
        Some(p) => {
            let id = parzi_providers::canonical_id(p).ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown provider `{p}` (one of: {})",
                    parzi_providers::PROVIDERS.join(", ")
                )
            })?;
            vec![id.to_string()]
        }
        None => vec![],
    };
    let mut all = Orchestrator::new(cfg, store).refresh_providers(&ids).await;
    all.retain(|s| ids.is_empty() || ids.contains(&s.provider));
    all.sort_by_key(|s| {
        parzi_providers::PROVIDERS
            .iter()
            .position(|p| *p == s.provider)
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }
    for s in &all {
        let state = match s.state {
            State::Ready => "ready",
            State::SignedOut => "signed out",
            State::NotInstalled => "not installed",
            State::Disabled => "switched off",
            State::Error => "error",
            State::Unchecked => "installed, sign-in unchecked",
        };
        println!("{:<12} {state}", parzi_providers::display_name(&s.provider));
        if !s.hint.is_empty() {
            println!("             {}", s.hint);
        }
        for m in &s.models {
            let default = if m.is_default { "  (default)" } else { "" };
            println!("             {}/{}{default}", s.provider, m.id);
        }
    }
    Ok(())
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

fn resolve_id(store: &SessionStore, id: &str) -> Result<String> {
    Ok(store.resolve_id(id)?)
}
