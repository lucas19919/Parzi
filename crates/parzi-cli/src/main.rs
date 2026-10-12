use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{ArgGroup, Parser, Subcommand};
use parzi_core::config::ParziConfig;
use parzi_core::paths;
use parzi_core::store::SessionStore;
use parzi_providers::State;
use parzi_runtime::desk::{self, normalize_url, DeskError, Probe};
use parzi_runtime::tools::{Approval, Approver, AutoApprover, ToolCallInfo};
use parzi_runtime::{handler::RunEvent, Orchestrator};
use serde_json::{json, Value};

mod window;

use window::{page, page_body, print_json, short};

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
    #[command(about = "Health: config, agents, the window, parzi serve")]
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
    #[command(about = "Tabs in the open window, and the pages in them")]
    Tab {
        #[command(subcommand)]
        action: TabCmd,
    },
    #[command(about = "Show something in the open window")]
    Open {
        #[command(subcommand)]
        what: OpenCmd,
        #[arg(long, global = true, help = "Open it without switching to it")]
        background: bool,
        #[arg(long, global = true, help = "Also bring the window to the front")]
        raise: bool,
    },
    #[command(about = "A notification in the open window")]
    Notify {
        #[arg(required = true)]
        text: Vec<String>,
        #[arg(long, help = "The session it is about: the notice opens it")]
        session: Option<String>,
    },
    #[command(about = "What the open window shows: tabs, folder, running sessions")]
    State {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Screenshot of the open window as a JPEG")]
    Screenshot {
        #[arg(long, help = "Where to save it. Default: ~/.parzi/shots/cli/")]
        out: Option<std::path::PathBuf>,
    },
    #[command(about = "Follow runs, approvals and tabs as they happen. Ctrl-C stops")]
    Watch {
        #[arg(long, help = "Only this session")]
        session: Option<String>,
        #[arg(long, help = "One JSON object per line")]
        json: bool,
    },
    #[command(about = "Run the engine with no window. Listens on 127.0.0.1")]
    Serve,
    #[command(about = "Set this machine up for ParziOS: ~/.parzi, a spec, the engine")]
    Setup {
        #[arg(
            long,
            help = "Settings and notes sent by the desktop. Deleted once applied"
        )]
        spec: Option<std::path::PathBuf>,
        #[arg(long, help = "Install and start the engine as a user service")]
        enable: bool,
        #[arg(long, help = "One JSON line per step")]
        json: bool,
    },
    #[command(about = "Relay newline JSON on stdio to parzi serve. Parzi runs this over SSH")]
    Rpc,
    #[command(about = "Tool calls waiting for a yes, in the window or on parzi serve")]
    Approval {
        #[command(subcommand)]
        action: ApprovalCmd,
    },
    #[command(about = "Install an agent or sign in to it, on this machine")]
    Agent {
        #[command(subcommand)]
        action: AgentCmd,
    },
    #[command(about = "ParziOS on another machine, over your SSH")]
    Remote {
        #[command(subcommand)]
        action: RemoteCmd,
    },
}

#[derive(Subcommand)]
enum AgentCmd {
    #[command(about = "Run the vendor's official installer")]
    Install {
        #[arg(help = "claude, codex, opencode, grok, antigravity, cursor")]
        provider: String,
    },
    #[command(about = "Sign in with the agent's own login")]
    Login { provider: String },
}

#[derive(Subcommand)]
enum RemoteCmd {
    #[command(about = "Install Parzi on a Linux machine and link to it. Needs key login")]
    Setup {
        #[arg(help = "user@host or user@host:port")]
        target: String,
        #[arg(long, help = "Keep brain notes on this machine")]
        no_notes: bool,
        #[arg(
            long,
            help = "Upload this Linux build instead of downloading the release"
        )]
        binary: Option<std::path::PathBuf>,
    },
    #[command(about = "The linked remote and whether its engine answers")]
    Status,
    #[command(about = "Send one op to the remote engine and print the reply")]
    Call {
        op: String,
        #[arg(help = "JSON object with the op's fields")]
        body: Option<String>,
    },
    #[command(about = "Sync brain notes, settings and this machine's sessions with the remote")]
    Sync,
    #[command(about = "Unlink this machine from the remote")]
    Forget,
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
    #[command(
        about = "`new` starts a session. An id continues one. Streams the reply; Ctrl-C stops the run"
    )]
    Send {
        target: String,
        #[arg(required = true)]
        message: Vec<String>,
        #[arg(
            long,
            help = "Agent and model: `claude`, `claude/opus`. Needed for a new session"
        )]
        model: Option<String>,
        #[arg(
            long,
            help = "Folder the agent works in. Default: the current directory"
        )]
        cwd: Option<String>,
        #[arg(
            long,
            help = "Approve tool calls without asking. With the window open, you confirm this there"
        )]
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
enum ApprovalCmd {
    #[command(about = "Tool calls waiting for a yes")]
    List {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Allow one. A key prefix works in the window")]
    Allow { key: String },
    #[command(about = "Deny one")]
    Deny { key: String },
}

#[derive(Subcommand)]
enum OpenCmd {
    #[command(about = "A session, in its own tab")]
    Session { id: String },
    #[command(about = "Settings: general, providers, connections, appearance or system")]
    Settings { section: Option<String> },
    #[command(about = "The brain, at a note (a path inside the vault)")]
    Brain { path: Option<String> },
    #[command(about = "History: sessions, history, bookmarks or downloads")]
    History { view: Option<String> },
    #[command(about = "The task list")]
    Tasks,
    #[command(about = "The home tab")]
    Home,
    #[command(about = "A web page")]
    Url { url: String },
}

/// Page commands work on the tab in front unless `--tab` names one. A tab
/// no agent opened is yours: the window asks you before a command reads
/// or drives it.
#[derive(Subcommand)]
enum TabCmd {
    List {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Open a URL, or switch to the tab that already has it")]
    Open {
        url: String,
        #[arg(long, help = "Open it without switching to it")]
        background: bool,
        #[arg(long, help = "Also bring the window to the front")]
        raise: bool,
    },
    Focus {
        id: String,
        #[arg(long, help = "Also bring the window to the front")]
        raise: bool,
    },
    Close {
        id: String,
    },
    #[command(about = "A page's text and controls")]
    Read {
        #[arg(long)]
        tab: Option<String>,
        #[arg(long, help = "Where the text starts, in characters")]
        offset: Option<u64>,
        #[arg(long, help = "How much text, 200 to 20000 characters (default 4000)")]
        limit: Option<u64>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Click a control by its text or a CSS selector")]
    #[command(group(ArgGroup::new("target").required(true).args(["text", "selector"])))]
    Click {
        text: Option<String>,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        tab: Option<String>,
    },
    #[command(about = "Type into a field: by its label, a CSS selector, or the focused one")]
    Type {
        text: String,
        #[arg(long)]
        field: Option<String>,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long, help = "Add to the text instead of replacing it")]
        append: bool,
        #[arg(long, help = "Then press Enter once")]
        submit: bool,
        #[arg(long)]
        tab: Option<String>,
    },
    #[command(about = "Press a key: Enter, Tab, Escape, ArrowDown, a letter, ctrl+a")]
    Press {
        key: String,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        tab: Option<String>,
    },
    #[command(about = "Choose an option in a dropdown")]
    Select {
        option: String,
        #[arg(long)]
        field: Option<String>,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        tab: Option<String>,
    },
    #[command(about = "Scroll: down, up, top or bottom, or to an element")]
    Scroll {
        #[arg(default_value = "down", value_parser = ["down", "up", "top", "bottom"])]
        to: String,
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        tab: Option<String>,
    },
    #[command(about = "Wait for a CSS selector or a text to show up (or go)")]
    #[command(group(ArgGroup::new("what").required(true).args(["selector", "text"])))]
    Wait {
        #[arg(long)]
        selector: Option<String>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long, help = "Wait until it is gone instead")]
        gone: bool,
        #[arg(long, default_value_t = 10, help = "Seconds, at most 60")]
        timeout: u64,
        #[arg(long)]
        tab: Option<String>,
    },
    #[command(about = "Screenshot a page as a JPEG")]
    Shot {
        #[arg(long)]
        tab: Option<String>,
        #[arg(long, help = "Where to save it. Default: ~/.parzi/shots/cli/")]
        out: Option<std::path::PathBuf>,
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
        Cmd::Open {
            what,
            background,
            raise,
        } => cmd_open(what, background, raise).await,
        Cmd::Notify { text, session } => {
            window::simple(
                "desk.notify",
                json!({ "text": text.join(" "), "session": session.unwrap_or_default() }),
            )
            .await
        }
        Cmd::State { json } => window::state(json).await,
        Cmd::Screenshot { out } => window::screenshot(out).await,
        Cmd::Watch { session, json } => window::watch(session, json).await,
        Cmd::Serve => cmd_serve().await,
        Cmd::Setup { spec, enable, json } => cmd_setup(spec.as_deref(), enable, json).await,
        Cmd::Rpc => {
            let exe = std::env::current_exe().context("finding the parzi binary")?;
            parzi_runtime::provision::relay(&exe)
                .await
                .context("relaying to parzi serve")
        }
        Cmd::Approval { action } => cmd_approval(action).await,
        Cmd::Agent { action } => cmd_agent(action).await,
        Cmd::Remote { action } => cmd_remote(action).await,
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

async fn cmd_serve() -> Result<()> {
    let (cfg, store) = boot()?;
    let orch = Arc::new(Orchestrator::new(cfg, store));
    let daemon = parzi_runtime::osserve::start(orch)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    eprintln!("parzi serve listening on 127.0.0.1:{}", daemon.port);
    if let Some(path) = desk::serve_path() {
        eprintln!("token file: {}", path.display());
    }
    eprintln!("this socket is local. A remote machine is reached through SSH.");
    tokio::select! {
        r = tokio::signal::ctrl_c() => r.context("waiting for ctrl-c")?,
        () = terminated() => {}
        () = daemon.stopped() => eprintln!("asked to stop"),
    }
    drop(daemon);
    eprintln!("stopped");
    Ok(())
}

/// SIGTERM, which is how systemd stops the service.
async fn terminated() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            term.recv().await;
            return;
        }
    }
    std::future::pending::<()>().await;
}

async fn cmd_setup(spec: Option<&std::path::Path>, enable: bool, json: bool) -> Result<()> {
    use parzi_runtime::provision::{self, Step};
    let report = |s: &Step| {
        if json {
            println!("{}", serde_json::to_string(s).unwrap_or_default());
        } else {
            println!(
                "{} {:<8} {}",
                if s.ok { "ok  " } else { "FAIL" },
                s.step,
                s.detail
            );
        }
    };
    let mut failed = false;
    let mut emit = |s: Step| {
        failed |= !s.ok;
        report(&s);
    };
    match cmd_init_quiet() {
        Ok(root) => emit(Step::ok("home", root.display().to_string())),
        Err(e) => emit(Step::fail("home", format!("{e:#}"))),
    }
    if let Some(path) = spec {
        let parsed = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))
            .and_then(|raw| {
                serde_json::from_str::<provision::Spec>(&raw)
                    .map_err(|e| format!("the spec is not valid: {e}"))
            });
        match parsed {
            Ok(spec) => {
                if !spec.from.is_empty() {
                    emit(Step::ok("spec", format!("from {}", spec.from)));
                }
                for step in provision::apply_spec(&spec) {
                    emit(step);
                }
                let _ = std::fs::remove_file(path);
            }
            Err(e) => emit(Step::fail("spec", e)),
        }
    }
    if enable {
        let exe = std::env::current_exe().context("finding the parzi binary")?;
        for step in provision::install_service(&exe).await {
            emit(step);
        }
        match provision::ensure_serving(&exe, Duration::from_secs(20)).await {
            Ok(v) => emit(Step::ok(
                "engine",
                format!("Parzi {v} answers on this machine"),
            )),
            Err(e) => emit(Step::fail("engine", e)),
        }
        match parzi_runtime::desk::call_serve(
            "providers",
            json!({ "refresh": true }),
            Duration::from_secs(90),
        )
        .await
        {
            Ok(v) => emit(Step::ok("agents", agents_line(&v))),
            Err(e) => emit(Step::ok("agents", format!("not checked: {e}"))),
        }
    } else if cfg!(target_os = "linux") {
        emit(Step::ok(
            "service",
            "not installed; run `parzi setup --enable` to start the engine as a user service",
        ));
    } else {
        emit(Step::ok(
            "service",
            "on this machine, start the engine with `parzi serve`",
        ));
    }
    let done = Step {
        step: "done".into(),
        ok: !failed,
        detail: String::new(),
    };
    if json {
        report(&done);
    }
    if failed {
        anyhow::bail!("setup did not finish");
    }
    Ok(())
}

/// "2 of 6 ready: Claude, Codex" from a `providers` reply.
fn agents_line(v: &Value) -> String {
    let rows = v
        .get("providers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let ready: Vec<&str> = rows
        .iter()
        .filter(|r| r.get("state").and_then(Value::as_str) == Some("ready"))
        .filter_map(|r| r.get("provider").and_then(Value::as_str))
        .map(parzi_providers::display_name)
        .collect();
    if ready.is_empty() {
        "no agent is signed in here yet. Install and sign in from Parzi, or run `parzi agent install claude`".into()
    } else {
        format!(
            "{} of {} ready: {}",
            ready.len(),
            rows.len(),
            ready.join(", ")
        )
    }
}

fn cmd_init_quiet() -> Result<std::path::PathBuf> {
    let root = paths::ensure_dirs().context("creating ~/.parzi")?;
    if !paths::config_path()?.exists() {
        ParziConfig::default().save().context("writing config")?;
    }
    let theme = parzi_core::theme::Theme::load().unwrap_or_default();
    theme.save().context("writing theme")?;
    Ok(root)
}

async fn cmd_agent(action: AgentCmd) -> Result<()> {
    let (id, install) = match &action {
        AgentCmd::Install { provider } => (provider, true),
        AgentCmd::Login { provider } => (provider, false),
    };
    let id = parzi_providers::canonical_id(id).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown agent `{id}` (one of: {})",
            parzi_providers::PROVIDERS.join(", ")
        )
    })?;
    let name = parzi_providers::display_name(id);
    let (cfg, _) = boot()?;
    let script = if install {
        parzi_providers::install_script(id)
    } else {
        parzi_providers::login_script(id, &cfg)
    };
    let Some(script) = script else {
        if install {
            anyhow::bail!("no installer is known for {name}");
        }
        eprintln!("{name} signs in through your browser. If none opens here, sign in on a machine with a browser.");
        return parzi_providers::sign_in(id, &cfg, || {
            eprintln!("waiting for the browser sign-in…")
        })
        .await
        .map_err(|e| anyhow::anyhow!(e.message));
    };
    eprintln!(
        "{}: {script}",
        if install { "installing" } else { "signing in" }
    );
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("powershell.exe");
        c.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ]);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", &script]);
        c
    };
    let status = cmd.status().context("starting the shell")?;
    if !status.success() {
        anyhow::bail!("{name}: the command failed ({status})");
    }
    Ok(())
}

async fn report_sync(link: &parzi_runtime::remote::Link, server: &str) -> Result<()> {
    let (_, store) = boot()?;
    let done = parzi_runtime::sync::everything(link, &store, server).await;
    let summary = done.summary();
    println!(
        "synced with {server}: {}",
        if summary.is_empty() {
            "already in step"
        } else {
            &summary
        }
    );
    for e in &done.errors {
        eprintln!("FAIL {e}");
    }
    if done.errors.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("sync did not finish")
    }
}

fn parse_target(raw: &str) -> Result<parzi_runtime::remote::Target> {
    let (user, host) = raw
        .split_once('@')
        .ok_or_else(|| anyhow::anyhow!("write the remote as user@host"))?;
    parzi_runtime::remote::Target::parse(user, host).map_err(|e| anyhow::anyhow!(e))
}

async fn remote_link() -> Result<(parzi_runtime::remote::Saved, parzi_runtime::remote::Link)> {
    use parzi_runtime::remote;
    let saved = remote::load_saved()
        .ok_or_else(|| anyhow::anyhow!("no remote yet: `parzi remote setup user@host`"))?;
    let link = remote::Link::open(&remote::Transport::ssh(&saved.target))
        .await
        .map_err(|e| anyhow::anyhow!("{}: {e}", saved.target.label()))?;
    Ok((saved, link))
}

async fn cmd_remote(action: RemoteCmd) -> Result<()> {
    use parzi_runtime::remote;
    match action {
        RemoteCmd::Setup {
            target,
            no_notes,
            binary,
        } => {
            let target = parse_target(&target)?;
            let opts = remote::SetupOptions {
                notes: !no_notes,
                binary,
                ..Default::default()
            };
            let print = |p: remote::Progress| {
                let mark = match p.state {
                    "ok" => "ok  ",
                    "fail" => "FAIL",
                    "run" => "... ",
                    _ => "    ",
                };
                eprintln!("{mark} {:<8} {}", p.step, p.detail);
            };
            let (saved, link) = remote::setup(target, opts, &print)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            report_sync(&link, &saved.target.label()).await
        }
        RemoteCmd::Sync => {
            let (saved, link) = remote_link().await?;
            report_sync(&link, &saved.target.label()).await
        }
        RemoteCmd::Status => {
            let (saved, link) = remote_link().await?;
            let v = link
                .call("health", json!({}), Duration::from_secs(10))
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            println!(
                "{}  Parzi {}",
                saved.target.label(),
                v.get("version").and_then(Value::as_str).unwrap_or("?")
            );
            Ok(())
        }
        RemoteCmd::Call { op, body } => {
            let body: Value = match body {
                Some(raw) => {
                    serde_json::from_str(&raw).context("the body must be a JSON object")?
                }
                None => json!({}),
            };
            let (_, link) = remote_link().await?;
            let v = link
                .call(&op, body, Duration::from_secs(120))
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            print_json(&v)
        }
        RemoteCmd::Forget => {
            remote::forget().map_err(|e| anyhow::anyhow!(e))?;
            println!("unlinked. The remote keeps running until you stop it there.");
            Ok(())
        }
    }
}

async fn cmd_approval(action: ApprovalCmd) -> Result<()> {
    match action {
        ApprovalCmd::List { json } => {
            let v = window::approvals("approval.list", json!({})).await?;
            if json {
                return print_json(&v);
            }
            let rows = v
                .get("approvals")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if rows.is_empty() {
                println!("no approvals waiting");
                return Ok(());
            }
            for row in rows {
                let key = row.get("key").and_then(Value::as_str).unwrap_or("");
                let session = row.get("session").and_then(Value::as_str).unwrap_or("");
                let label = row.get("label").and_then(Value::as_str).unwrap_or("");
                println!("{key}  {}  {label}", short(session));
            }
            Ok(())
        }
        ApprovalCmd::Allow { key } => answer_approval(&key, true).await,
        ApprovalCmd::Deny { key } => answer_approval(&key, false).await,
    }
}

async fn answer_approval(key: &str, allow: bool) -> Result<()> {
    let v = window::approvals("approval.answer", json!({ "key": key, "allow": allow })).await?;
    println!(
        "{} {}",
        if allow { "allowed" } else { "denied" },
        v.get("key").and_then(Value::as_str).unwrap_or(key)
    );
    Ok(())
}

/// Neither the window nor `parzi serve` answers: the files are ours.
fn offline(err: &str) -> bool {
    desk::is_not_open(err)
}

async fn engine(op: &str, body: Value, wait: Duration) -> Result<Value, String> {
    desk::call_desk_or_serve(op, body, wait).await
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
    match engine("session.list", json!({}), Duration::from_secs(5)).await {
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
    match engine(op, json!({ "id": id }), Duration::from_secs(5)).await {
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
    // No automatic models: a new thread is told which agent runs it.
    let model = model.map(str::trim).filter(|m| !m.is_empty());
    if target == "new" && model.is_none() {
        anyhow::bail!(
            "a new session needs a model: --model claude (or claude/opus, codex, ...). `parzi providers` lists them"
        );
    }
    let base = match cwd.filter(|c| !c.trim().is_empty()) {
        Some(c) => std::path::PathBuf::from(c),
        None => std::env::current_dir().context("reading the current directory")?,
    };
    let body = window::send_body(
        target,
        message,
        model,
        &base.display().to_string(),
        yes,
        effort,
    );
    let view = Arc::new(std::sync::Mutex::new(window::SendView::default()));
    let shown = view.clone();
    let mut on_event = move |v: &Value| {
        if let Ok(mut view) = shown.lock() {
            view.show(v);
        }
    };
    let sent = desk::request_within("session.send", body.clone(), None, &mut on_event);
    let got = tokio::select! {
        r = sent => r,
        r = tokio::signal::ctrl_c() => {
            r.context("waiting for ctrl-c")?;
            // Ctrl-C stops the run in the window too, not just this command.
            let id = view.lock().ok().and_then(|v| v.session.clone());
            if let Some(id) = id {
                let _ = desk::request("session.kill", json!({ "id": id })).await;
                eprintln!("\nstopped {id}");
            }
            std::process::exit(130);
        }
    };
    let view = view
        .lock()
        .map(|v| (v.printed, v.ended_line))
        .unwrap_or_default();
    match got {
        Ok(v) => {
            if !view.0 {
                // A window from before streaming answers with the whole text.
                if let Some(id) = v.get("id").and_then(Value::as_str) {
                    eprintln!("session {id}");
                }
                println!("{}", v.get("text").and_then(Value::as_str).unwrap_or(""));
            } else if !view.1 {
                println!();
            }
            Ok(())
        }
        Err(DeskError::Failed(e)) => {
            if view.0 && !view.1 {
                println!();
            }
            anyhow::bail!(e)
        }
        Err(DeskError::NotOpen) => {
            let mut body = body;
            body["stream"] = json!(false);
            match desk::call_serve("session.send", body, Duration::from_secs(60)).await {
                Ok(v) => {
                    if let Some(id) = v.get("id").and_then(Value::as_str) {
                        eprintln!("session {id}");
                    }
                    let status = v.get("status").and_then(Value::as_str).unwrap_or("active");
                    eprintln!("{status} on parzi serve");
                    eprintln!("parzi approval list");
                    Ok(())
                }
                Err(e) if e.contains("not running") => {
                    send_local(target, message, model, &base, yes, effort).await
                }
                Err(e) => anyhow::bail!(e),
            }
        }
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
    let (id, mut rx) = if let Some(id) = continued {
        let rx = orch
            .send_to(
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
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        (id, rx)
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
        (meta.id, rx)
    };
    loop {
        let ev = tokio::select! {
            ev = rx.recv() => ev,
            r = tokio::signal::ctrl_c() => {
                r.context("waiting for ctrl-c")?;
                // The run and its agent process stop with this command.
                let _ = orch.kill(&id).await;
                eprintln!("\nstopped {id}");
                std::process::exit(130);
            }
        };
        let Some(ev) = ev else { break };
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
    match engine("session.fork", body, Duration::from_secs(5)).await {
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
    match engine("session.kill", json!({ "id": id }), Duration::from_secs(5)).await {
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
    match engine(
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
    match engine(
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

/// The op and body a tab command sends; None for the ones that print
/// something richer (list, read, shot).
fn tab_body(cmd: TabCmd) -> Option<(&'static str, Value)> {
    let opt = |v: Option<String>| v.map_or(Value::Null, Value::String);
    Some(match cmd {
        TabCmd::Open {
            url,
            background,
            raise,
        } => (
            "tab.open",
            json!({ "url": normalize_url(&url), "focus": !background, "raise": raise }),
        ),
        TabCmd::Focus { id, raise } => ("tab.focus", json!({ "id": id, "raise": raise })),
        TabCmd::Close { id } => ("tab.close", json!({ "id": id })),
        TabCmd::Click {
            text,
            selector,
            tab,
        } => (
            "tab.click",
            page_body(tab, json!({ "text": opt(text), "selector": opt(selector) })),
        ),
        TabCmd::Type {
            text,
            field,
            selector,
            append,
            submit,
            tab,
        } => (
            "tab.type",
            page_body(
                tab,
                json!({
                    "text": text,
                    "field": opt(field),
                    "selector": opt(selector),
                    "append": append,
                    "submit": submit,
                }),
            ),
        ),
        TabCmd::Press { key, selector, tab } => (
            "tab.press",
            page_body(tab, json!({ "key": key, "selector": opt(selector) })),
        ),
        TabCmd::Select {
            option,
            field,
            selector,
            tab,
        } => (
            "tab.select",
            page_body(
                tab,
                json!({ "option": option, "field": opt(field), "selector": opt(selector) }),
            ),
        ),
        TabCmd::Scroll { to, selector, tab } => (
            "tab.scroll",
            page_body(tab, json!({ "to": to, "selector": opt(selector) })),
        ),
        TabCmd::Wait {
            selector,
            text,
            gone,
            timeout,
            tab,
        } => (
            "tab.wait",
            page_body(
                tab,
                json!({
                    "selector": opt(selector),
                    "text": opt(text),
                    "gone": gone,
                    "timeout_ms": timeout.min(60) * 1000,
                }),
            ),
        ),
        TabCmd::List { .. } | TabCmd::Read { .. } | TabCmd::Shot { .. } => return None,
    })
}

async fn cmd_tab(action: TabCmd) -> Result<()> {
    match action {
        TabCmd::List { json } => window::tab_list(json).await,
        TabCmd::Read {
            tab,
            offset,
            limit,
            json,
        } => window::tab_read(tab, offset, limit, json).await,
        TabCmd::Shot { tab, out } => window::tab_shot(tab, out).await,
        other => match tab_body(other) {
            Some((op @ ("tab.open" | "tab.focus" | "tab.close"), body)) => {
                window::simple(op, body).await
            }
            Some((op, body)) => page(op, body).await,
            None => Ok(()),
        },
    }
}

fn open_body(what: OpenCmd, background: bool, raise: bool) -> Value {
    let (kind, extra) = match what {
        OpenCmd::Session { id } => ("session", json!({ "id": id })),
        OpenCmd::Settings { section } => ("settings", json!({ "section": section })),
        OpenCmd::Brain { path } => ("brain", json!({ "path": path })),
        OpenCmd::History { view } => ("history", json!({ "view": view })),
        OpenCmd::Tasks => ("tasks", json!({})),
        OpenCmd::Home => ("home", json!({})),
        OpenCmd::Url { url } => ("url", json!({ "url": normalize_url(&url) })),
    };
    let mut body = json!({ "what": kind, "focus": !background, "raise": raise });
    if let (Value::Object(b), Value::Object(e)) = (&mut body, extra) {
        b.extend(e.into_iter().filter(|(_, v)| !v.is_null()));
    }
    body
}

async fn cmd_open(what: OpenCmd, background: bool, raise: bool) -> Result<()> {
    window::simple("desk.open", open_body(what, background, raise)).await
}

/// The window, checked for real: the file, the hello, then the token.
async fn window_check() -> parzi_runtime::doctor::Check {
    let check = |ok: bool, detail: String| parzi_runtime::doctor::Check {
        name: "window".into(),
        ok,
        detail,
    };
    match desk::probe().await {
        Probe::Open(file) => match desk::request("ping", json!({})).await {
            Ok(_) => check(
                true,
                format!(
                    "Parzi {} answers on 127.0.0.1:{} (pid {})",
                    if file.version.is_empty() {
                        "(older)"
                    } else {
                        &file.version
                    },
                    file.port,
                    file.pid
                ),
            ),
            Err(DeskError::Failed(e)) if e.contains("unknown op") => check(
                true,
                format!("an older Parzi answers on 127.0.0.1:{}", file.port),
            ),
            Err(e) => check(false, format!("the window did not take its token: {e}")),
        },
        Probe::Cleared => check(
            true,
            "not open (removed a gui.json a crash left behind)".into(),
        ),
        Probe::NotOpen => check(true, "not open".into()),
        Probe::Stuck(why) => check(false, why),
    }
}

async fn serve_check() -> parzi_runtime::doctor::Check {
    let (ok, detail) = match desk::call_serve("health", json!({}), Duration::from_secs(3)).await {
        Ok(v) => (
            true,
            format!(
                "parzi serve {} answers",
                v.get("version").and_then(Value::as_str).unwrap_or("?")
            ),
        ),
        Err(e) if e.contains("not running") => (true, "not running".into()),
        Err(e) => (false, e),
    };
    parzi_runtime::doctor::Check {
        name: "serve".into(),
        ok,
        detail,
    }
}

async fn cmd_doctor(json: bool) -> Result<()> {
    let (cfg, _) = boot()?;
    let mut checks = parzi_runtime::doctor::Doctor::new(cfg).run().await;
    checks.push(window_check().await);
    checks.push(serve_check().await);
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
    // The window's board when it is open: one set of probes, the same
    // answer the window shows.
    let shown = desk::request("providers", json!({ "refresh": true, "ids": ids }))
        .await
        .ok()
        .and_then(|v| {
            serde_json::from_value::<Vec<parzi_providers::ProviderStatus>>(
                v.get("providers")?.clone(),
            )
            .ok()
        });
    let mut all = match shown {
        Some(all) => all,
        None => Orchestrator::new(cfg, store).refresh_providers(&ids).await,
    };
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

fn resolve_id(store: &SessionStore, id: &str) -> Result<String> {
    Ok(store.resolve_id(id)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("parzi").chain(args.iter().copied()))
    }

    fn tab(args: &[&str]) -> (&'static str, Value) {
        let Cmd::Tab { action } = parse(args).unwrap().cmd else {
            panic!("not a tab command: {args:?}");
        };
        tab_body(action).unwrap()
    }

    #[test]
    fn page_commands_build_their_requests() {
        let (op, body) = tab(&["tab", "click", "Sign in", "--tab", "tab-1"]);
        assert_eq!(op, "tab.click");
        assert_eq!(body["tab"], "tab-1");
        assert_eq!(body["args"]["text"], "Sign in");
        let (_, body) = tab(&["tab", "click", "--selector", "#go"]);
        assert_eq!(body["args"]["selector"], "#go");
        assert!(body.get("tab").is_none(), "default: the tab in front");
        let (op, body) = tab(&["tab", "type", "hello", "--field", "Search", "--submit"]);
        assert_eq!(op, "tab.type");
        assert_eq!(body["args"]["submit"], true);
        assert_eq!(body["args"]["append"], false);
        let (op, body) = tab(&["tab", "press", "ctrl+a"]);
        assert_eq!((op, &body["args"]["key"]), ("tab.press", &json!("ctrl+a")));
        let (op, body) = tab(&["tab", "select", "Blue", "--field", "Color"]);
        assert_eq!(
            (op, &body["args"]["option"]),
            ("tab.select", &json!("Blue"))
        );
        let (_, body) = tab(&["tab", "scroll"]);
        assert_eq!(body["args"]["to"], "down");
        let (op, body) = tab(&["tab", "wait", "--text", "Done", "--timeout", "500"]);
        assert_eq!(op, "tab.wait");
        assert_eq!(body["args"]["timeout_ms"], 60_000, "capped at a minute");
        let (op, body) = tab(&["tab", "open", "github.com", "--background"]);
        assert_eq!(op, "tab.open");
        assert_eq!(body["url"], "https://github.com");
        assert_eq!(body["focus"], false);
        let (_, body) = tab(&["tab", "open", "github.com"]);
        assert_eq!(body["focus"], true, "the CLI's own tab comes forward");
    }

    #[test]
    fn page_commands_need_a_target() {
        assert!(parse(&["tab", "click"]).is_err());
        assert!(parse(&["tab", "wait"]).is_err());
        assert!(parse(&["tab", "scroll", "sideways"]).is_err());
        assert!(parse(&["tab", "read", "--offset", "4000", "--limit", "500"]).is_ok());
        assert!(parse(&["tab", "shot", "--out", "x.jpg"]).is_ok());
    }

    #[test]
    fn open_names_what_to_show() {
        let open = |args: &[&str]| {
            let Cmd::Open {
                what,
                background,
                raise,
            } = parse(args).unwrap().cmd
            else {
                panic!("not open");
            };
            open_body(what, background, raise)
        };
        let body = open(&["open", "settings", "providers"]);
        assert_eq!(body["what"], "settings");
        assert_eq!(body["section"], "providers");
        assert_eq!(body["focus"], true);
        let body = open(&["open", "session", "abc123", "--background", "--raise"]);
        assert_eq!(body["id"], "abc123");
        assert_eq!(body["focus"], false);
        assert_eq!(body["raise"], true);
        let body = open(&["open", "brain"]);
        assert!(body.get("path").is_none());
        let body = open(&["open", "url", "localhost:1420"]);
        assert_eq!(body["url"], "http://localhost:1420");
        assert!(parse(&["open", "nowhere"]).is_err());
    }

    #[test]
    fn the_rest_of_the_window_commands_parse() {
        assert!(parse(&["watch", "--session", "abc", "--json"]).is_ok());
        assert!(parse(&["state", "--json"]).is_ok());
        assert!(parse(&["screenshot"]).is_ok());
        assert!(parse(&["notify", "build", "is", "green"]).is_ok());
        assert!(parse(&["notify"]).is_err());
        assert!(parse(&["approval", "list", "--json"]).is_ok());
        assert!(parse(&["approval", "allow", "1a2b"]).is_ok());
        assert!(
            parse(&["session", "send", "new"]).is_err(),
            "needs a message"
        );
        let Cmd::Session {
            action: SessionCmd::Send { model, yes, .. },
        } = parse(&["session", "send", "new", "hi", "--yes"])
            .unwrap()
            .cmd
        else {
            panic!("not send");
        };
        assert_eq!(model, None, "no default model, never `auto`");
        assert!(yes);
    }
}
