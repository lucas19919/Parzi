//! parzi CLI: the potato path and the interop surface for other harnesses.
//! list/show/export/send/fork/kill/doctor/providers. Zero webview deps.

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use parzi_core::config::ParziConfig;
use parzi_core::paths;
use parzi_core::store::{SessionMeta, SessionStore};
use parzi_providers::State;
use parzi_runtime::tools::{Approval, Approver, AutoApprover, ToolCallInfo};
use parzi_runtime::{handler::RunEvent, Orchestrator};

mod mcp;

#[derive(Parser)]
#[command(name = "parzi", version, about = "Lean agent harness")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create ~/.parzi tree + default config/theme (never overwrites).
    Init,
    /// List sessions, newest first.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Print a session transcript (markdown).
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Print transcript to stdout (pipe-friendly for other harnesses).
    Export { id: String },
    /// Send a message: `new` starts a session, an id continues one.
    Send {
        target: String,
        message: Vec<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        lane: Option<String>,
        /// Agent and model: `claude`, `claude/opus`, `codex/gpt-5.5`,
        /// `opencode/<provider>/<model>`… Default: Smart Auto picks the agent.
        #[arg(long)]
        model: Option<String>,
        /// The folder the agent works in. Default: the current directory.
        #[arg(long)]
        cwd: Option<String>,
        /// Auto-approve tool calls (default prompts on a terminal).
        #[arg(long)]
        yes: bool,
        /// Attach files as context (repeatable).
        #[arg(long)]
        attach: Vec<String>,
        /// Reasoning effort: low | medium | high | extra | ultra, or the
        /// agent's own word for it (`xhigh`, `max`).
        #[arg(long, default_value = "medium")]
        effort: String,
    },
    /// Fork a session transcript into a new session.
    Fork {
        id: String,
        #[arg(long)]
        at: Option<usize>,
    },
    /// Cancel a live run and mark it killed.
    Kill { id: String },
    /// Health checks: config, agents, Smart Auto order, MCP, webview.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Where each agent stands, asked of its own program: installed, signed
    /// in, plan usage, models. Spends no quota. Sign-in happens in the
    /// agent's own program; the hint says how.
    Providers {
        /// Only this one: claude, codex, opencode, grok, antigravity, cursor.
        provider: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Manage cumulative project knowledge (KNOWLEDGE.md).
    Knowledge {
        project: String,
        /// Note to append; when omitted, displays current knowledge.
        note: Option<String>,
    },
    /// Living project plan: check status or run pending tasks headlessly.
    Plan {
        project: String,
        #[command(subcommand)]
        action: PlanAction,
    },
    /// Review isolated worktree diff for a session and optionally apply it.
    Review {
        project: String,
        session_id: String,
        #[arg(long)]
        lane: Option<String>,
        #[arg(long)]
        apply: bool,
    },
    /// Hub workspaces: list them, or sync one through its git remote.
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
    /// Serve Parzi to other agents over MCP (JSON-RPC on stdio).
    Mcp,
    /// File an issue on Parzi itself (needs GitHub auth: stored token or gh's).
    ReportIssue { title: String, body: String },
}

#[derive(Subcommand)]
enum WorkspaceAction {
    /// List workspaces with their git remote.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Commit local work, take the team's, hand ours over. Exits non-zero on
    /// conflicts so a provisioning script stops instead of syncing garbage.
    Sync {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Show the workspace's sync remote, or point it at one. Without a remote
    /// a sync only ever commits locally.
    Remote { name: String, url: Option<String> },
    /// Join a workspace that already exists on a remote. Use this on a second
    /// machine instead of `workspace_create` — two creates cannot be merged.
    Clone {
        url: String,
        /// Local name; defaults to the repo's own.
        name: Option<String>,
    },
}

#[derive(Subcommand)]
enum PlanAction {
    /// Show current living plan status and pending tasks.
    Status,
    /// Execute pending living plan tasks headlessly.
    Run {
        #[arg(long)]
        lane: Option<String>,
        #[arg(long)]
        yes: bool,
    },
}

/// Prompts y/N on a terminal; denies when piped (safe default).
struct CliApprover {
    yes: bool,
}

#[async_trait::async_trait]
impl Approver for CliApprover {
    async fn approve(&self, call: &ToolCallInfo) -> Approval {
        if self.yes {
            return Approval::Allow;
        }
        eprintln!("tool `{}` in lane `{}`", call.name, call.lane);
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&call.args).unwrap_or_default()
        );
        eprintln!("allow? [y/N] ");
        use std::io::Write;
        let _ = std::io::stderr().flush();
        let ans = tokio::task::spawn_blocking(|| {
            let mut s = String::new();
            std::io::stdin().read_line(&mut s).unwrap_or(0);
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

// E9: the CLI is one command, one run — a worker per hardware thread buys
// nothing. Blocking work (stdin prompts) already uses spawn_blocking.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    // MCP framing owns stdout: every log line must go to stderr instead,
    // or one stray print corrupts the protocol.
    if matches!(cli.cmd, Cmd::Mcp) {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .init();
    }
    match cli.cmd {
        Cmd::Init => cmd_init(),
        Cmd::List { json } => cmd_list(json),
        Cmd::Show { id, json } => cmd_show(&id, json),
        Cmd::Export { id } => cmd_export(&id),
        Cmd::Send {
            target,
            message,
            project,
            lane,
            model,
            cwd,
            yes,
            attach,
            effort,
        } => {
            cmd_send(SendParams {
                target: target.clone(),
                message: message.join(" "),
                project,
                lane,
                model,
                cwd,
                yes,
                attach,
                effort: effort.clone(),
            })
            .await
        }
        Cmd::Fork { id, at } => cmd_fork(&id, at),
        Cmd::Kill { id } => cmd_kill(&id).await,
        Cmd::Doctor { json } => cmd_doctor(json).await,
        Cmd::Providers { provider, json } => cmd_providers(provider.as_deref(), json).await,
        Cmd::Knowledge { project, note } => cmd_knowledge(&project, note.as_deref()),
        Cmd::Plan { project, action } => cmd_plan(&project, action).await,
        Cmd::Review {
            project,
            session_id,
            lane,
            apply,
        } => cmd_review(&project, &session_id, lane.as_deref(), apply),
        Cmd::Workspace { action } => cmd_workspace(action),
        Cmd::Mcp => mcp::run().await,
        Cmd::ReportIssue { title, body } => cmd_report_issue(&title, &body).await,
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
    println!("background: {}", theme.background.image);
    Ok(())
}

fn cmd_list(json: bool) -> Result<()> {
    let (_, store) = boot()?;
    let all = store.list().context("listing sessions")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&all)?);
        return Ok(());
    }
    if all.is_empty() {
        println!("no sessions yet — `parzi send new \"hello\"`");
        return Ok(());
    }
    for m in all {
        println!(
            "{}  {:?}  {}  {}  {} in/{} out",
            short(&m.id),
            m.status,
            m.model,
            lane_of(&m),
            m.tokens_in,
            m.tokens_out
        );
    }
    Ok(())
}

fn cmd_show(id: &str, json: bool) -> Result<()> {
    let (_, store) = boot()?;
    let id = resolve_id(&store, id)?;
    let meta = store.get(&id).context("reading session")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&meta)?);
        return Ok(());
    }
    let md = std::fs::read_to_string(paths::sessions_dir()?.join(&id).join("session.md"))
        .context("reading transcript")?;
    println!("{md}");
    Ok(())
}

fn cmd_export(id: &str) -> Result<()> {
    cmd_show(id, false)
}

struct SendParams {
    target: String,
    message: String,
    project: Option<String>,
    lane: Option<String>,
    model: Option<String>,
    cwd: Option<String>,
    yes: bool,
    attach: Vec<String>,
    effort: String,
}

async fn cmd_send(p: SendParams) -> Result<()> {
    let SendParams {
        target,
        message,
        project,
        lane,
        model,
        cwd,
        yes,
        attach,
        effort,
    } = p;
    if message.trim().is_empty() {
        anyhow::bail!("empty message");
    }
    let (mut cfg, store) = boot()?;
    // CLI is one-shot: a busy harness gets a loud error, not a silent queue.
    cfg.orchestrator.queue_when_busy = false;
    let orch = Orchestrator::new(cfg.clone(), store);
    orch.recover().ok();
    let approver: Arc<dyn Approver> = if yes {
        Arc::new(AutoApprover)
    } else {
        Arc::new(CliApprover { yes })
    };
    // A new thread works where the command was run; a continued one keeps
    // its own folder. `--cwd` overrides both.
    let explicit = cwd.filter(|c| !c.trim().is_empty());
    let base = match &explicit {
        Some(c) => std::path::PathBuf::from(c),
        None => std::env::current_dir().context("reading the current directory")?,
    };
    let attachments = parzi_core::context::read_attachments(&base, &attach);
    let mut rx = if target == "new" {
        let (meta, rx) = orch
            .spawn(
                &project.unwrap_or_else(|| "default".into()),
                &lane.unwrap_or_default(),
                &model.unwrap_or_else(|| "auto".into()),
                &message,
                Some(approver),
                &base.display().to_string(),
                &effort,
                attachments,
                None,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        eprintln!("session {}", meta.id);
        rx
    } else {
        let id = target.clone();
        orch.send_to(
            &id,
            &message,
            Some(approver),
            explicit.as_deref().unwrap_or(""),
            &effort,
            attachments,
            None,
            None,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
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
                eprintln!("[result] {name} ok={ok} {ms}ms")
            }
            RunEvent::Usage {
                tokens_in,
                tokens_out,
                cost_usd,
            } => {
                eprintln!("\n[usage] {tokens_in} in / {tokens_out} out (${cost_usd:.4})")
            }
            RunEvent::Context { used, limit } => {
                eprintln!("[context] {used} / {limit} tokens")
            }
            RunEvent::ApprovalRequest { .. } => {} // CliApprover prompts on stderr; event is observability only
            RunEvent::Notice { text } => eprintln!("\n[notice] {text}"),
            RunEvent::Reasoning { text } => {
                use std::io::Write;
                let _ = write!(std::io::stderr(), "{text}");
            }
            RunEvent::Done { turns } => {
                eprintln!("\ndone in {turns} turns");
                break;
            }
            RunEvent::Error(e) => {
                eprintln!("\nerror: {e}");
                break;
            }
        }
    }
    // E8/R-2: a one-shot command must not leave connectors running behind it.
    orch.mcp().shutdown().await;
    Ok(())
}

fn cmd_fork(id: &str, at: Option<usize>) -> Result<()> {
    let (_, store) = boot()?;
    let id = resolve_id(&store, id)?;
    let meta = store.fork(&id, at).context("forking session")?;
    println!("forked -> {}", meta.id);
    Ok(())
}

async fn cmd_kill(id: &str) -> Result<()> {
    let (cfg, store) = boot()?;
    let id = resolve_id(&store, id)?;
    Orchestrator::new(cfg, store)
        .kill(&id)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("killed {id}");
    Ok(())
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

/// Ask each agent's own program where it stands, now.
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
    // The board answers with every provider it knows; show what was asked.
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
        let about: Vec<&str> = [s.version.as_deref(), s.account.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        println!(
            "{:<12} {state}{}",
            parzi_providers::display_name(&s.provider),
            if about.is_empty() {
                String::new()
            } else {
                format!(" — {}", about.join(" · "))
            }
        );
        if !s.gated {
            println!(
                "             can change files without asking Parzi: leases and the folder \
                 fence cannot stop it, and read-only lanes refuse it"
            );
        }
        if !s.hint.is_empty() {
            println!("             {}", s.hint);
        }
        for w in &s.usage {
            println!("             {}: {:.0}% used", w.label, w.used_percent);
        }
        for m in &s.models {
            let efforts = if m.efforts.is_empty() {
                String::new()
            } else {
                format!("  [{}]", m.efforts.join(" "))
            };
            let default = if m.is_default { "  (default)" } else { "" };
            println!("             {}/{}{default}{efforts}", s.provider, m.id);
        }
    }
    Ok(())
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

fn lane_of(m: &SessionMeta) -> String {
    if m.lane.is_empty() {
        m.project.clone()
    } else {
        format!("{}/{}", m.project, m.lane)
    }
}

/// Accept a full id or an 8-char prefix.
fn resolve_id(store: &SessionStore, id: &str) -> Result<String> {
    if id.len() >= 36 {
        return Ok(id.to_string());
    }
    for m in store.list()? {
        if m.id.starts_with(id) {
            return Ok(m.id);
        }
    }
    anyhow::bail!("no session matching `{id}`");
}

fn cmd_knowledge(project: &str, note: Option<&str>) -> Result<()> {
    if let Some(n) = note {
        parzi_core::lanes::append_knowledge(project, n)
            .context("appending to cumulative knowledge")?;
        println!("recorded knowledge for project `{project}`");
    } else {
        match parzi_core::lanes::read_knowledge(project) {
            Some(text) => println!("{text}"),
            None => println!("no cumulative knowledge recorded yet for project `{project}`"),
        }
    }
    Ok(())
}

/// The no-MCP half of remote control: on a VM you have a shell, not a
/// harness, so syncing a workspace must be one command. Calls the plain
/// syncer rather than the runtime timer — a one-shot process has no use for
/// a 60 s background loop it would exit out from under.
fn cmd_workspace(action: WorkspaceAction) -> Result<()> {
    use parzi_core::workspace;
    match action {
        WorkspaceAction::List { json } => {
            let rows: Vec<serde_json::Value> = workspace::list()
                .into_iter()
                .map(|name| {
                    let remote = workspace::sync::remote_url(&workspace::dir(&name));
                    serde_json::json!({ "name": name, "remote": remote })
                })
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else if rows.is_empty() {
                println!("no workspaces");
            } else {
                for row in &rows {
                    let name = row["name"].as_str().unwrap_or_default();
                    let remote = row["remote"].as_str().unwrap_or("(local only)");
                    println!("{name}\t{remote}");
                }
            }
            Ok(())
        }
        WorkspaceAction::Sync { name, json } => {
            let ws = workspace::load(&name).context("loading workspace")?;
            let report = workspace::sync::sync_now(&ws, &format!("parzi: {name} sync"))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                let did = [
                    ("committed", report.committed),
                    ("pulled", report.pulled),
                    ("pushed", report.pushed),
                ]
                .into_iter()
                .filter(|(_, yes)| *yes)
                .map(|(what, _)| what)
                .collect::<Vec<_>>();
                println!(
                    "{name}: {}",
                    if did.is_empty() {
                        "nothing to do".to_string()
                    } else {
                        did.join(", ")
                    }
                );
            }
            if report.conflicts.is_empty() {
                Ok(())
            } else {
                for path in &report.conflicts {
                    eprintln!("conflict: {path}");
                }
                anyhow::bail!("{} file(s) need a human", report.conflicts.len())
            }
        }
        WorkspaceAction::Remote { name, url } => {
            let ws = workspace::load(&name).context("loading workspace")?;
            if url.is_some() {
                // `init` is the idempotent form: existing history is kept and
                // `origin` is re-pointed. It never pushes, so the first push
                // stays an explicit `workspace sync`.
                workspace::sync::init(&ws, url).map_err(|e| anyhow::anyhow!("{e}"))?;
            }
            match workspace::sync::remote_url(&workspace::dir(&name)) {
                Some(u) => println!("{u}"),
                None => println!("(local only)"),
            }
            Ok(())
        }
        WorkspaceAction::Clone { url, name } => {
            let name = name.unwrap_or_else(|| repo_basename(&url));
            let dir =
                workspace::sync::clone_into(&url, &name).map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("{name}\t{}", dir.display());
            Ok(())
        }
    }
}

/// `…/team-hub.git` → `team-hub`. Only a default; `safe_name` still judges it.
fn repo_basename(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit(['/', ':', '\\'])
        .next()
        .unwrap_or(url)
        .trim_end_matches(".git")
        .to_string()
}

async fn cmd_report_issue(title: &str, body: &str) -> Result<()> {
    let url = parzi_providers::github::report_issue(title, body, "parzi cli")
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("{url}");
    Ok(())
}

fn cmd_review(project: &str, session_id: &str, lane: Option<&str>, apply: bool) -> Result<()> {
    let (_, store) = boot()?;
    let id = resolve_id(&store, session_id)?;
    let meta = store.get(&id).context("reading session")?;
    let lane_name = lane.unwrap_or(&meta.lane);
    let clean_lane = if lane_name.trim().is_empty() {
        "default"
    } else {
        lane_name.trim()
    };
    let wt_path = paths::worktrees_dir()?
        .join(project)
        .join(clean_lane)
        .join(&id);
    if !wt_path.exists() {
        println!(
            "no isolated worktree found for session {id} at {}",
            wt_path.display()
        );
        return Ok(());
    }
    let diff =
        parzi_runtime::git_worktree::worktree_diff(&wt_path).context("reading worktree diff")?;
    if diff.trim().is_empty() {
        println!("no changes in worktree for session {id}");
    } else {
        println!("=== Worktree Diff ({id} / {clean_lane}) ===\n");
        println!("{diff}");
    }
    if apply {
        let root =
            parzi_core::lanes::lane_root(project, clean_lane).unwrap_or_else(|| meta.cwd.clone());
        if root.trim().is_empty() {
            anyhow::bail!("cannot apply: project root is not set");
        }
        let out = parzi_runtime::git_worktree::apply_worktree(&root, clean_lane, &id)
            .context("applying worktree to project root")?;
        println!("applied changes to {root}:\n{out}");
    }
    Ok(())
}

async fn cmd_plan(project: &str, action: PlanAction) -> Result<()> {
    match action {
        PlanAction::Status => {
            let raw = parzi_core::plan::read_plan(project).context("reading plan")?;
            let plan = parzi_core::plan::parse_plan(&raw);
            println!("=== Living Plan: {project} ===\n");
            for milestone in plan.milestones_grouped() {
                println!("## {}", milestone.title);
                for task in milestone.tasks {
                    let status_char = match task.status {
                        parzi_core::plan::TaskStatus::Done => 'x',
                        parzi_core::plan::TaskStatus::InProgress => '/',
                        parzi_core::plan::TaskStatus::Pending => ' ',
                    };
                    let lane_tag = task
                        .lane
                        .as_deref()
                        .map(|l| format!(" [lane:{l}]"))
                        .unwrap_or_default();
                    let wt_tag = if task.worktree { " [worktree]" } else { "" };
                    println!("- [{status_char}] {}{lane_tag}{wt_tag}", task.title);
                }
                println!();
            }
            if !plan.loose_tasks.is_empty() {
                println!("## Tasks");
                for task in plan.loose_tasks {
                    let status_char = match task.status {
                        parzi_core::plan::TaskStatus::Done => 'x',
                        parzi_core::plan::TaskStatus::InProgress => '/',
                        parzi_core::plan::TaskStatus::Pending => ' ',
                    };
                    let lane_tag = task
                        .lane
                        .as_deref()
                        .map(|l| format!(" [lane:{l}]"))
                        .unwrap_or_default();
                    let wt_tag = if task.worktree { " [worktree]" } else { "" };
                    println!("- [{status_char}] {}{lane_tag}{wt_tag}", task.title);
                }
            }
        }
        PlanAction::Run { lane, yes } => {
            let mut completed = 0;
            while let Some(task) = parzi_core::plan::next_pending_task(project) {
                let target_lane = lane
                    .clone()
                    .or(task.lane.clone())
                    .unwrap_or_else(|| "core".into());
                let wt_label = if task.worktree {
                    " (isolated worktree)"
                } else {
                    ""
                };
                println!(
                    "\n>>> Executing task: {}{wt_label} in lane `{target_lane}`",
                    task.title
                );

                let prompt = format!(
                    "Execute living plan task: {}\n\nPerform all required edits, run tests, and record any gotchas or decisions with knowledge.record.",
                    task.title
                );

                let (mut cfg, store) = boot()?;
                cfg.orchestrator.queue_when_busy = false;
                let orch = Orchestrator::new(cfg.clone(), store.clone());
                orch.recover().ok();

                let roster = parzi_core::lanes::get_project_roster(project).unwrap_or_default();
                let model = roster
                    .orchestrator
                    .model
                    .filter(|m| !m.trim().is_empty())
                    .unwrap_or_else(|| "auto".into());

                let root = parzi_core::lanes::lane_root(project, &target_lane).unwrap_or_default();
                let approver: Arc<dyn Approver> = if yes {
                    Arc::new(AutoApprover)
                } else {
                    Arc::new(CliApprover { yes })
                };

                // The session exists first, so an isolated task's worktree is
                // named for it and the agent starts inside it — an agent's
                // folder is fixed once it runs.
                let meta = store
                    .create(&task.title, project, &target_lane, &model)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let mut session_cwd = root.clone();
                if task.worktree
                    && !root.is_empty()
                    && parzi_runtime::git_worktree::is_git_repo(&root)
                {
                    let wt = parzi_runtime::git_worktree::create_worktree(
                        &root,
                        project,
                        &target_lane,
                        &meta.id,
                    )
                    .map_err(|e| anyhow::anyhow!("isolated worktree for `{}`: {e}", task.title))?;
                    session_cwd = wt.to_string_lossy().to_string();
                    println!("isolated worktree: {session_cwd}");
                }
                store
                    .set_cwd(&meta.id, &session_cwd)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let mut rx = orch
                    .send_to(
                        &meta.id,
                        &prompt,
                        Some(approver),
                        &session_cwd,
                        "medium",
                        vec![],
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                while let Some(event) = rx.recv().await {
                    match event {
                        RunEvent::Text(t) => {
                            use std::io::Write;
                            print!("{t}");
                            let _ = std::io::stdout().flush();
                        }
                        RunEvent::ToolCall { name, label, .. } => {
                            eprintln!("[tool: {name} — {label}]")
                        }
                        RunEvent::ToolResult { name, ok, ms, .. } => {
                            eprintln!("[result: {name} ok={ok} {ms}ms]")
                        }
                        RunEvent::Done { .. } => {
                            eprintln!("\n[done]");
                            break;
                        }
                        RunEvent::Error(e) => {
                            eprintln!("\n[error: {e}]");
                            break;
                        }
                        _ => {}
                    }
                }

                // E8/R-2: this loop builds one orchestrator per task; each
                // must take its MCP children with it.
                orch.mcp().shutdown().await;
                // Mark task complete in living plan
                let _ = parzi_core::plan::set_task_status(project, &task.title, true);
                completed += 1;
                println!(">>> Task marked done in PLAN.md: {}", task.title);
            }
            if completed == 0 {
                println!("no pending tasks found in living plan for project `{project}`");
            } else {
                println!("\nAll pending tasks completed ({completed} total).");
            }
        }
    }
    Ok(())
}
