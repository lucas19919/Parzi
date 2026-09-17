//! parzi CLI: the potato path and the interop surface for other harnesses.
//! list/show/export/send/fork/kill/doctor/models/health. Zero webview deps.

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use parzi_core::config::ParziConfig;
use parzi_core::paths;
use parzi_core::store::{SessionMeta, SessionStore};
use parzi_runtime::tools::{Approval, Approver, AutoApprover, ToolCallInfo};
use parzi_runtime::{handler::RunEvent, Orchestrator};

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
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        /// Auto-approve tool calls (default prompts on a terminal).
        #[arg(long)]
        yes: bool,
        /// Attach files as context (repeatable).
        #[arg(long)]
        attach: Vec<String>,
        /// Reasoning effort: low | med | high.
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
    /// Health checks: config, keys (presence only), MCP, webview.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// List known models per provider.
    Models { provider: Option<String> },
    /// Live provider health: auth, tier, account, active cooldowns.
    Health {
        #[arg(long)]
        json: bool,
    },
    /// Clear rate-limit cooldowns (one provider, or all when omitted).
    ResetCooldowns { provider: Option<String> },
    /// Sign in with a subscription provider (antigravity: Google OAuth).
    Login { provider: String },
    /// Sign out (deletes stored tokens).
    Logout { provider: String },
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
// nothing. Blocking work (stdin prompts, keyring) already uses spawn_blocking.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .init();
    let cli = Cli::parse();
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
        Cmd::Models { provider } => cmd_models(provider.as_deref()),
        Cmd::Health { json } => cmd_health(json),
        Cmd::ResetCooldowns { provider } => cmd_reset_cooldowns(provider.as_deref()),
        Cmd::Login { provider } => cmd_login(&provider).await,
        Cmd::Logout { provider } => cmd_logout(&provider),
        Cmd::Knowledge { project, note } => cmd_knowledge(&project, note.as_deref()),
        Cmd::Plan { project, action } => cmd_plan(&project, action).await,
        Cmd::Review {
            project,
            session_id,
            lane,
            apply,
        } => cmd_review(&project, &session_id, lane.as_deref(), apply),
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
    let cwd = cwd.unwrap_or_default();
    let base = if cwd.trim().is_empty() {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        std::path::PathBuf::from(&cwd)
    };
    let attachments = parzi_core::context::read_attachments(&base, &attach);
    let mut rx = if target == "new" {
        let (meta, rx) = orch
            .spawn(
                &project.unwrap_or_else(|| "default".into()),
                &lane.unwrap_or_default(),
                &model.unwrap_or_else(|| cfg.default_provider.clone()),
                &message,
                Some(approver),
                &cwd,
                &effort,
                attachments,
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
            &cwd,
            &effort,
            attachments,
            None,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
    };
    while let Some(ev) = rx.recv().await {
        match ev {
            RunEvent::Text(t) => print!("{t}"),
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
            RunEvent::Notice { text } => eprintln!("\n[router] {text}"),
            RunEvent::RouteTransition {
                from_provider,
                to_provider,
                reason,
                ..
            } => {
                eprintln!("\n[router] {from_provider} -> {to_provider} ({reason})")
            }
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

async fn cmd_login(provider: &str) -> Result<()> {
    if provider != "antigravity" {
        anyhow::bail!("login supports: antigravity (API keys go in Settings > Models)");
    }
    eprintln!("opt-in notice: unofficial Google OAuth path; bans have been reported.");
    eprintln!("Use an account you can afford to lose. Continue? [y/N] ");
    {
        use std::io::Write;
        let _ = std::io::stderr().flush();
        let mut s = String::new();
        std::io::stdin().read_line(&mut s)?;
        if !s.trim().eq_ignore_ascii_case("y") {
            anyhow::bail!("aborted");
        }
    }
    let url = parzi_providers::antigravity_oauth::auth_url();
    println!("opening:\n{url}\n");
    parzi_providers::antigravity_oauth::open_browser(&url);
    eprintln!("waiting for Google callback on localhost:51121 (5 min)…");
    let code = parzi_providers::antigravity_oauth::wait_for_code(300)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let toks = parzi_providers::antigravity_oauth::exchange_code(&code)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    keyring::Entry::new("parzi", "antigravity")
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .set_password(&toks.access)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if let Some(r) = toks.refresh {
        keyring::Entry::new("parzi", "antigravity-refresh")
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .set_password(&r)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    println!("signed in with Google. `parzi doctor` should show auth:antigravity ok.");
    Ok(())
}

fn cmd_logout(provider: &str) -> Result<()> {
    for entry in ["", "-refresh", "-session"] {
        let name = format!("{provider}{entry}");
        if let Ok(e) = keyring::Entry::new("parzi", &name) {
            let _ = e.delete_credential();
        }
    }
    println!("signed out of {provider} (stored tokens deleted).");
    Ok(())
}

fn cmd_models(provider: Option<&str>) -> Result<()> {
    let (cfg, _) = boot()?;
    let ids: Vec<&str> = match provider {
        Some(p) => vec![p],
        None => parzi_providers::PROVIDERS.to_vec(),
    };
    for id in ids {
        match parzi_providers::provider(id, &cfg) {
            Ok(p) => {
                let status = match p.auth_status() {
                    parzi_providers::AuthStatus::Ok => "auth:ok",
                    parzi_providers::AuthStatus::Missing(_) => "auth:missing",
                    parzi_providers::AuthStatus::Expired(_) => "auth:expired",
                };
                println!("{id} [{status}]");
                for m in crate_models(id) {
                    println!(
                        "  {}  ctx={}  ${}/${} per 1M",
                        m.id, m.context_limit, m.price_in, m.price_out
                    );
                }
            }
            Err(e) => println!("{id} error: {e}"),
        }
    }
    Ok(())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cmd_health(json: bool) -> Result<()> {
    let (cfg, store) = boot()?;
    let orch = Orchestrator::new(cfg, store);
    let health = orch.provider_health();
    if json {
        println!("{}", serde_json::to_string_pretty(&health)?);
        return Ok(());
    }
    let snap = orch.config();
    println!(
        "routing: auto_failover={} keys_in_auto={} order=[{}]",
        snap.routing.auto_failover,
        snap.routing.keys_in_auto,
        parzi_providers::router::auto_order(&snap).join(",")
    );
    for h in &health {
        let cool = h.cooldown_until.map(|u| u.saturating_sub(now_secs()));
        let cool_s = cool.map(|s| format!(" cooldown={s}s")).unwrap_or_default();
        let acct = h
            .active_account
            .as_ref()
            .map(|a| format!(" acct={a}"))
            .unwrap_or_default();
        let err = h
            .last_error
            .as_ref()
            .map(|e| format!(" err={e}"))
            .unwrap_or_default();
        println!(
            "{:12} {:12} tier={:<12}{}{}{}",
            h.provider, h.status, h.tier, acct, cool_s, err
        );
    }
    Ok(())
}

fn cmd_reset_cooldowns(provider: Option<&str>) -> Result<()> {
    let (cfg, store) = boot()?;
    let orch = Orchestrator::new(cfg, store);
    orch.reset_circuit_breaker(provider);
    match provider {
        Some(p) => println!("cooldown cleared for {p}"),
        None => println!("all cooldowns cleared"),
    }
    Ok(())
}

fn crate_models(id: &str) -> Vec<parzi_providers::Model> {
    parzi_providers::catalog::for_provider(id)
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
                    .unwrap_or_else(|| cfg.default_provider.clone());

                let root = parzi_core::lanes::lane_root(project, &target_lane).unwrap_or_default();
                let mut session_cwd = root.clone();
                let approver: Arc<dyn Approver> = if yes {
                    Arc::new(AutoApprover)
                } else {
                    Arc::new(CliApprover { yes })
                };

                let (meta, mut rx) = orch
                    .spawn(
                        project,
                        &target_lane,
                        &model,
                        &prompt,
                        Some(approver),
                        &session_cwd,
                        "medium",
                        vec![],
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                if task.worktree
                    && !root.is_empty()
                    && parzi_runtime::git_worktree::is_git_repo(&root)
                {
                    if let Ok(wt) = parzi_runtime::git_worktree::create_worktree(
                        &root,
                        project,
                        &target_lane,
                        &meta.id,
                    ) {
                        session_cwd = wt.to_string_lossy().to_string();
                        let _ = store.set_cwd(&meta.id, &session_cwd);
                        println!("isolated worktree: {session_cwd}");
                    }
                }

                while let Some(event) = rx.recv().await {
                    match event {
                        RunEvent::Text(t) => print!("{t}"),
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
