//! `parzi mcp`: Parzi as a tool for other agents. MCP (JSON-RPC 2.0 over
//! newline-delimited stdio) exposing sessions, workspaces, deck projects,
//! knowledge, plans, diagnostics and agent self-reports.
//!
//! stdout carries protocol messages only — every log, transcript stream and
//! progress line goes to stderr, or one stray print corrupts the framing.
//! Tool results are JSON text the calling agent parses; failures come back
//! with `isError: true`, never as protocol errors.

use std::fmt::Write as _;
use std::sync::Arc;

use anyhow::{Context, Result};
use parzi_core::store::SessionStore;
use parzi_runtime::tools::{Approver, AutoApprover};
use parzi_runtime::{handler::RunEvent, Orchestrator};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL_VERSION: &str = "2024-11-05";
/// Transcripts longer than this are tail-cut in `session_send` results.
const TRANSCRIPT_CAP: usize = 12_000;

pub async fn run() -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut server = Server {
        client: String::new(),
    };
    while let Some(line) = lines.next_line().await.context("reading stdin")? {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                // No id to answer to: log and carry on.
                eprintln!("parzi mcp: bad frame: {e}");
                continue;
            }
        };
        if let Some(reply) = server.handle(&msg).await {
            let mut out = serde_json::to_string(&reply).context("encoding reply")?;
            out.push('\n');
            tokio::io::stdout()
                .write_all(out.as_bytes())
                .await
                .context("writing stdout")?;
            tokio::io::stdout()
                .flush()
                .await
                .context("flushing stdout")?;
        }
    }
    Ok(())
}

struct Server {
    client: String,
}

impl Server {
    async fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        // Notifications carry no id and get no reply.
        match method {
            "initialize" => {
                self.client = params
                    .get("clientInfo")
                    .and_then(|c| c.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                Some(ok(
                    id,
                    &json!({
                        "protocolVersion": PROTOCOL_VERSION,
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "parzi", "version": env!("CARGO_PKG_VERSION") },
                    }),
                ))
            }
            "ping" => Some(ok(id, &json!({}))),
            "tools/list" => Some(ok(id, &json!({ "tools": tool_defs() }))),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(Value::Null);
                let client = self.client.clone();
                Some(call_result(id, dispatch(name, args, &client).await))
            }
            "" => None,
            _ if id.is_none() => None,
            _ => Some(err(
                id.as_ref(),
                -32601,
                &format!("unknown method: {method}"),
            )),
        }
    }
}

fn ok(id: Option<Value>, result: &Value) -> Value {
    match id {
        Some(i) => json!({ "jsonrpc": "2.0", "id": i, "result": result }),
        None => json!({ "jsonrpc": "2.0", "result": result }),
    }
}

fn err(id: Option<&Value>, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn call_result(id: Option<Value>, r: Result<Value, String>) -> Value {
    match r {
        Ok(v) => ok(
            id,
            &json!({ "content": [{ "type": "text", "text": v.to_string() }] }),
        ),
        Err(e) => ok(
            id,
            &json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
        ),
    }
}

/// One line per tool: name, what it does, and the one sharp edge the caller
/// must respect. Agents read these cold, so they carry their own manual.
/// A static table: long by nature, split would only scatter it.
#[allow(clippy::too_many_lines)]
fn tool_defs() -> Value {
    let t = |name: &str, desc: &str, schema: Value| json!({ "name": name, "description": desc, "inputSchema": schema });
    let obj = |props: Value, required: &[&str]| json!({ "type": "object", "properties": props, "required": required });
    let s = |desc: &str| json!({ "type": "string", "description": desc });
    let b =
        |desc: &str, def: bool| json!({ "type": "boolean", "description": desc, "default": def });
    let strlist =
        |desc: &str| json!({ "type": "array", "items": { "type": "string" }, "description": desc });
    let int = |desc: &str| json!({ "type": "integer", "description": desc });
    json!([
        t("session_send",
            "Send a message to Parzi. target \"new\" starts a session, an id continues one. Runs to completion and returns the transcript tail plus usage. Long runs block — prefer small, verifiable steps.",
            obj(json!({
                "target": s("Session id, id prefix, or \"new\""),
                "message": s("The prompt"),
                "project": s("Chat key: a workspace name or \"default\""),
                "lane": s("Lane within the project (default \"\")"),
                "model": s("Agent and model: claude, claude/opus, codex/gpt-5.5, opencode/<provider>/<model>… or \"auto\" (default) for Smart Auto"),
                "effort": s("low | medium | high | extra | ultra, or the agent's own word (default \"medium\")"),
                "cwd": s("The folder the agent works in (default: where this server runs; a continued session keeps its own)"),
                "attach": strlist("Workspace-relative files to attach as context"),
                "auto_approve": b("Approve tool calls without asking (default true; the transport is non-interactive, false is refused)", true),
                "mode": s("Run mode floor for this send: \"auto\" (default) or \"deny\" for a read-only run. \"ask\" is refused: nothing can answer. Workspace policy still floors both."),
            }), &["target", "message"])),
        t("session_list", "List sessions, newest first.",
            obj(json!({}), &[])),
        t("session_show",
            "Read one session: metadata plus transcript markdown (tail-cut past 20000 chars). Accepts a full id or prefix.",
            obj(json!({ "id": s("Session id or prefix") }), &["id"])),
        t("session_fork",
            "Fork a session transcript into a new session.",
            obj(json!({
                "id": s("Session id or prefix"),
                "at": int("Message index to fork at (default the end)"),
            }), &["id"])),
        t("session_kill", "Cancel a live run and mark it killed.",
            obj(json!({ "id": s("Session id or prefix") }), &["id"])),
        t("workspace_list", "Hub workspace names.",
            obj(json!({}), &[])),
        t("workspace_create",
            "Create a hub workspace. Names are [A-Za-z0-9_-], max 64 chars.",
            obj(json!({
                "name": s("Workspace name"),
                "kind": s("\"solo\" or \"team\" (default \"solo\")"),
            }), &["name"])),
        t("workspace_delete",
            "Delete a hub workspace, its deck projects' role sessions and its chats. Irreversible. Refuses \"default\".",
            obj(json!({ "name": s("Workspace name") }), &["name"])),
        t("workspace_migrate",
            "Move a legacy ~/.parzi/projects entry into a hub workspace of the same name. Chat keys survive; only the directory moves.",
            obj(json!({ "name": s("Legacy project name") }), &["name"])),
        t("workspace_sync",
            "Sync a hub workspace through its git remote (commit + pull + push). Run it on each machine that shares the workspace — or pair it with a remote parzi over SSH and let each side sync itself.",
            obj(json!({ "workspace": s("Workspace name") }), &["workspace"])),
        t("workspace_remote",
            "Read the workspace's sync remote, or point it at a git URL. Without a remote a sync only commits locally — set one before expecting work to reach another machine.",
            obj(json!({
                "workspace": s("Workspace name"),
                "url": s("Git URL for origin; omit to just read the current one"),
            }), &["workspace"])),
        t("workspace_clone",
            "Join a workspace that already exists on a git remote. Use this on a second machine (a VM, a teammate's box) instead of workspace_create — two separate creates hold unrelated histories and can never be merged.",
            obj(json!({
                "url": s("Git URL of the workspace repo"),
                "name": s("Local workspace name (default: the repo's own)"),
            }), &["url"])),
        t("project_list", "Deck projects of one workspace.",
            obj(json!({ "workspace": s("Workspace name") }), &["workspace"])),
        t("project_create", "Create a deck project (starts Drafting).",
            obj(json!({
                "workspace": s("Workspace name"),
                "title": s("Human title; the slug derives from it"),
                "repos": strlist("Repo names mapped in the workspace"),
                "header": s("Roster model for the header role (default \"\")"),
                "orchestrator": s("Roster model for the orchestrator role (default \"\")"),
                "coder": s("Roster model for the coder role (default \"\")"),
                "budget_usd": json!({ "type": ["number", "null"], "description": "Budget cap (default null)" }),
            }), &["workspace", "title"])),
        t("project_delete",
            "Delete a deck project and its role sessions. Irreversible.",
            obj(json!({
                "workspace": s("Workspace name"),
                "slug": s("Project slug"),
            }), &["workspace", "slug"])),
        t("project_rename",
            "Rename a deck project's title. The slug never moves, so role sessions keep working.",
            obj(json!({
                "workspace": s("Workspace name"),
                "slug": s("Project slug"),
                "title": s("New title"),
            }), &["workspace", "slug", "title"])),
        t("knowledge",
            "Read a project's cumulative KNOWLEDGE.md, or append one note to it.",
            obj(json!({
                "project": s("Project key"),
                "note": s("Note to append; omit to just read"),
            }), &["project"])),
        t("plan_status",
            "The living plan markdown of a legacy project (plus parsed tasks when it parses).",
            obj(json!({ "project": s("Project key") }), &["project"])),
        t("doctor", "Health checks: config, agents, Smart Auto order, MCP.",
            obj(json!({}), &[])),
        t("providers",
            "Where each agent stands, asked of its own program: installed, signed in, plan usage, models with their effort levels. Spends no quota.",
            obj(json!({ "provider": s("One of claude, codex, opencode, grok, antigravity, cursor; omit for all") }), &[])),
        t("report_issue",
            "File an issue on Parzi itself (lucas19919/Parzi, labelled agent-report). ONLY for Parzi bugs, annoyances or feature requests you actually hit — never for the user's own project. Confirm with the user first unless they asked you to file. One issue per distinct problem, with a real title (10+ chars) and what-happened plus what-you-expected (40+ chars). Needs GitHub auth: the stored token or gh's.",
            obj(json!({
                "title": s("Issue title"),
                "body": s("What happened, what you expected, steps to reproduce"),
            }), &["title", "body"])),
    ])
}

fn s_arg(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(ToString::to_string)
}

fn req(v: &Value, key: &str) -> Result<String, String> {
    let s = s_arg(v, key).unwrap_or_default();
    if s.trim().is_empty() {
        return Err(format!("missing required argument: {key}"));
    }
    Ok(s)
}

fn b_arg(v: &Value, key: &str, def: bool) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(def)
}

fn arr_arg(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn boot() -> Result<(parzi_core::config::ParziConfig, SessionStore), String> {
    parzi_core::paths::ensure_dirs().map_err(|e| e.to_string())?;
    let cfg = parzi_core::config::ParziConfig::load().map_err(|e| e.to_string())?;
    let store = SessionStore::open().map_err(|e| e.to_string())?;
    Ok((cfg, store))
}

/// Full id or 8-char prefix, like the CLI.
fn resolve_id(store: &SessionStore, id: &str) -> Result<String, String> {
    if id.len() >= 36 {
        return Ok(id.to_string());
    }
    for m in store.list().map_err(|e| e.to_string())? {
        if m.id.starts_with(id) {
            return Ok(m.id);
        }
    }
    Err(format!("no session matching `{id}`"))
}

async fn dispatch(name: &str, args: Value, client: &str) -> Result<Value, String> {
    if let Some(r) = dispatch_session(name, &args).await {
        return r;
    }
    if let Some(r) = dispatch_workspace(name, &args).await {
        return r;
    }
    if let Some(r) = dispatch_project(name, &args).await {
        return r;
    }
    if let Some(r) = dispatch_misc(name, &args, client).await {
        return r;
    }
    match name {
        "" => Err("missing tool name".into()),
        other => Err(format!("unknown tool: {other}")),
    }
}

/// Session tools. Returns `None` when `name` is not one of ours.
async fn dispatch_session(name: &str, args: &Value) -> Option<Result<Value, String>> {
    match name {
        "session_send" => Some(tool_send(args.clone()).await),
        "session_list" => Some((|| {
            let (_, store) = boot()?;
            let all = store.list().map_err(|e| e.to_string())?;
            Ok(json!(all))
        })()),
        "session_show" => Some((|| {
            let (_, store) = boot()?;
            let id = resolve_id(&store, &req(args, "id")?)?;
            let meta = store.get(&id).map_err(|e| e.to_string())?;
            let md = std::fs::read_to_string(
                parzi_core::paths::sessions_dir()
                    .map_err(|e| e.to_string())?
                    .join(&id)
                    .join("session.md"),
            )
            .map_err(|e| e.to_string())?;
            Ok(json!({ "meta": meta, "transcript": tail(&md, 20_000) }))
        })()),
        "session_fork" => Some((|| {
            let (_, store) = boot()?;
            let id = resolve_id(&store, &req(args, "id")?)?;
            let at = args
                .get("at")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok());
            let meta = store.fork(&id, at).map_err(|e| e.to_string())?;
            Ok(json!({ "id": meta.id }))
        })()),
        "session_kill" => Some(tool_kill(args.clone()).await),
        _ => None,
    }
}

async fn tool_kill(args: Value) -> Result<Value, String> {
    let (cfg, store) = boot()?;
    let id = resolve_id(&store, &req(&args, "id")?)?;
    Orchestrator::new(cfg, store)
        .kill(&id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "killed": id }))
}

/// Workspace tools. Returns `None` when `name` is not one of ours.
async fn dispatch_workspace(name: &str, args: &Value) -> Option<Result<Value, String>> {
    match name {
        "workspace_list" => Some(Ok(json!(parzi_core::workspace::list()))),
        "workspace_create" => Some((|| {
            let name = req(args, "name")?;
            let kind = match s_arg(args, "kind")
                .unwrap_or_else(|| "solo".into())
                .as_str()
            {
                "team" => parzi_core::workspace::Kind::Team,
                "solo" => parzi_core::workspace::Kind::Solo,
                other => return Err(format!("kind must be solo|team, got `{other}`")),
            };
            let user = std::env::var("PARZI_USER")
                .or_else(|_| std::env::var("USERNAME"))
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "me".into());
            let ws =
                parzi_core::workspace::create_for(&name, kind, &user).map_err(|e| e.to_string())?;
            Ok(json!(ws))
        })()),
        "workspace_delete" => Some(tool_workspace_delete(args.clone()).await),
        "workspace_sync" => Some(tool_workspace_sync(args.clone()).await),
        "workspace_remote" => Some(tool_workspace_remote(args)),
        "workspace_clone" => Some(tool_workspace_clone(args)),
        "workspace_migrate" => Some((|| {
            let ws = parzi_core::workspace::import_legacy(&req(args, "name")?)
                .map_err(|e| e.to_string())?;
            Ok(json!(ws))
        })()),
        _ => None,
    }
}

/// Deck project tools. Returns `None` when `name` is not one of ours.
async fn dispatch_project(name: &str, args: &Value) -> Option<Result<Value, String>> {
    match name {
        "project_list" => Some((|| {
            let ws = req(args, "workspace")?;
            let out: Vec<_> = parzi_core::project::list(&ws)
                .iter()
                .filter_map(|slug| parzi_core::project::load(&ws, slug).ok())
                .collect();
            Ok(json!(out))
        })()),
        "project_create" => Some((|| {
            let ws = req(args, "workspace")?;
            let title = req(args, "title")?;
            let p = parzi_core::project::create(
                &ws,
                &title,
                arr_arg(args, "repos"),
                parzi_core::project::Roster {
                    header: s_arg(args, "header").unwrap_or_default(),
                    orchestrator: s_arg(args, "orchestrator").unwrap_or_default(),
                    coder: s_arg(args, "coder").unwrap_or_default(),
                },
                args.get("budget_usd").and_then(Value::as_f64),
            )
            .map_err(|e| e.to_string())?;
            Ok(json!(p))
        })()),
        "project_delete" => Some(tool_project_delete(args.clone()).await),
        "project_rename" => Some((|| {
            let p = parzi_core::project::retitle(
                &req(args, "workspace")?,
                &req(args, "slug")?,
                &req(args, "title")?,
            )
            .map_err(|e| e.to_string())?;
            Ok(json!(p))
        })()),
        _ => None,
    }
}

/// Knowledge, plans, diagnostics and self-reports.
async fn dispatch_misc(name: &str, args: &Value, client: &str) -> Option<Result<Value, String>> {
    match name {
        "knowledge" => Some((|| {
            let project = req(args, "project")?;
            if let Some(note) = s_arg(args, "note") {
                parzi_core::lanes::append_knowledge(&project, &note).map_err(|e| e.to_string())?;
                Ok(json!({ "recorded": true, "project": project }))
            } else {
                Ok(json!({
                    "project": project,
                    "knowledge": parzi_core::lanes::read_knowledge(&project),
                }))
            }
        })()),
        "plan_status" => Some(
            (|| match parzi_core::plan::read_plan(&req(args, "project")?) {
                Ok(md) => {
                    let parsed = parzi_core::plan::parse_plan(&md);
                    Ok(json!({ "exists": true, "markdown": tail(&md, 20_000), "tasks": parsed }))
                }
                Err(_) => Ok(json!({ "exists": false })),
            })(),
        ),
        "doctor" => Some(tool_doctor().await),
        "providers" => Some(tool_providers(args).await),
        "report_issue" => Some(tool_report_issue(args.clone(), client).await),
        _ => None,
    }
}

async fn tool_report_issue(args: Value, client: &str) -> Result<Value, String> {
    let url =
        parzi_providers::github::report_issue(&req(&args, "title")?, &req(&args, "body")?, client)
            .await?;
    Ok(json!({ "url": url }))
}

async fn tool_workspace_sync(args: Value) -> Result<Value, String> {
    let report = parzi_runtime::sync_timer::sync_workspace(&req(&args, "workspace")?)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!(report))
}

/// Setting the remote goes through `sync::init`, which is idempotent and
/// never pushes: the first push stays an explicit `workspace_sync`.
fn tool_workspace_remote(args: &Value) -> Result<Value, String> {
    let name = req(args, "workspace")?;
    let ws = parzi_core::workspace::load(&name).map_err(|e| e.to_string())?;
    if let Some(url) = s_arg(args, "url").filter(|u| !u.trim().is_empty()) {
        parzi_core::workspace::sync::init(&ws, Some(url)).map_err(|e| e.to_string())?;
    }
    let remote = parzi_core::workspace::sync::remote_url(&parzi_core::workspace::dir(&name));
    Ok(json!({ "workspace": name, "remote": remote }))
}

fn tool_workspace_clone(args: &Value) -> Result<Value, String> {
    let url = req(args, "url")?;
    let name = s_arg(args, "name")
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| {
            url.trim_end_matches('/')
                .rsplit(['/', ':', '\\'])
                .next()
                .unwrap_or(&url)
                .trim_end_matches(".git")
                .to_string()
        });
    parzi_core::workspace::sync::clone_into(&url, &name).map_err(|e| e.to_string())?;
    let ws = parzi_core::workspace::load(&name).map_err(|e| e.to_string())?;
    Ok(json!(ws))
}

async fn tool_doctor() -> Result<Value, String> {
    let (cfg, _) = boot()?;
    let checks = parzi_runtime::doctor::Doctor::new(cfg).run().await;
    Ok(json!(checks))
}

/// Each agent's own program, asked now. Spends no quota.
async fn tool_providers(args: &Value) -> Result<Value, String> {
    let (cfg, store) = boot()?;
    let ids = match s_arg(args, "provider") {
        Some(p) => vec![parzi_providers::canonical_id(&p)
            .ok_or_else(|| format!("unknown provider `{p}`"))?
            .to_string()],
        None => vec![],
    };
    let mut all = Orchestrator::new(cfg, store).refresh_providers(&ids).await;
    all.retain(|s| ids.is_empty() || ids.contains(&s.provider));
    Ok(json!(all))
}

/// stdout stays silent in here: the transcript accumulates and goes home in
/// the result object.
async fn tool_send(args: Value) -> Result<Value, String> {
    if !b_arg(&args, "auto_approve", true) {
        return Err("non-interactive transport: pass auto_approve true (or omit it)".into());
    }
    let target = req(&args, "target")?;
    let message = req(&args, "message")?;
    let (mut cfg, store) = boot()?;
    // One-shot like the CLI: a busy harness gets a loud error, not a queue.
    cfg.orchestrator.queue_when_busy = false;
    let orch = Orchestrator::new(cfg.clone(), store);
    orch.recover().ok();
    let approver: Arc<dyn Approver> = Arc::new(AutoApprover);
    // A new thread works where this server runs; a continued one keeps its
    // own folder. `cwd` overrides both.
    let explicit = s_arg(&args, "cwd").filter(|c| !c.trim().is_empty() && c != ".");
    let base = match &explicit {
        Some(c) => std::path::PathBuf::from(c),
        None => {
            std::env::current_dir().map_err(|e| format!("reading the current directory: {e}"))?
        }
    };
    let attachments = parzi_core::context::read_attachments(&base, &arr_arg(&args, "attach"));
    let effort = s_arg(&args, "effort").unwrap_or_else(|| "medium".into());
    let model = s_arg(&args, "model").unwrap_or_else(|| "auto".into());
    let mode_override = match s_arg(&args, "mode")
        .unwrap_or_else(|| "auto".into())
        .as_str()
    {
        "auto" => None,
        "deny" => Some("deny".to_string()),
        other => {
            return Err(format!(
                "mode must be \"auto\" or \"deny\" over MCP (\"ask\" needs a human); got `{other}`"
            ))
        }
    };
    let (id, mut rx) = if target == "new" {
        let (meta, rx) = orch
            .spawn(
                &s_arg(&args, "project").unwrap_or_else(|| "default".into()),
                &s_arg(&args, "lane").unwrap_or_default(),
                &model,
                &message,
                Some(approver),
                &base.display().to_string(),
                &effort,
                attachments,
                mode_override.clone(),
            )
            .await
            .map_err(|e| e.to_string())?;
        let id = meta.id.clone();
        (id, rx)
    } else {
        let id = target.clone();
        let rx = orch
            .send_to(
                &id,
                &message,
                Some(approver),
                explicit.as_deref().unwrap_or(""),
                &effort,
                attachments,
                None,
                mode_override,
            )
            .await
            .map_err(|e| e.to_string())?;
        (id, rx)
    };
    let outcome = drain_run(&mut rx).await?;
    // E8/R-2: a one-shot command must not leave connectors running behind it.
    orch.mcp().shutdown().await;
    let truncated = outcome.text.len() > TRANSCRIPT_CAP;
    Ok(json!({
        "session_id": id,
        "turns": outcome.turns,
        "tokens_in": outcome.tokens_in,
        "tokens_out": outcome.tokens_out,
        "cost_usd": (outcome.cost_usd * 10_000.0).round() / 10_000.0,
        "truncated": truncated,
        "transcript": tail(&outcome.text, TRANSCRIPT_CAP),
    }))
}

/// What one drained run said: transcript, turns and usage.
struct RunOutcome {
    text: String,
    turns: u32,
    tokens_in: u64,
    tokens_out: u64,
    cost_usd: f64,
}

async fn drain_run(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<RunEvent>,
) -> Result<RunOutcome, String> {
    let mut out = RunOutcome {
        text: String::new(),
        turns: 0,
        tokens_in: 0,
        tokens_out: 0,
        cost_usd: 0.0,
    };
    while let Some(ev) = rx.recv().await {
        match ev {
            RunEvent::Text(t) => out.text.push_str(&t),
            RunEvent::ToolCall { name, label, .. } => {
                let _ = writeln!(out.text, "\n[tool] {name} — {label}");
            }
            RunEvent::ToolResult { name, ok, ms, .. } => {
                let _ = writeln!(out.text, "[result] {name} ok={ok} {ms}ms");
            }
            RunEvent::Usage {
                tokens_in: i,
                tokens_out: o,
                cost_usd: c,
            } => {
                out.tokens_in += i;
                out.tokens_out += o;
                out.cost_usd += c;
            }
            RunEvent::Done { turns } => {
                out.turns = turns;
                break;
            }
            RunEvent::Error(e) => {
                // The run died: hand back what it said plus the error.
                return Err(format!(
                    "run failed: {e}\n--- partial transcript ---\n{}",
                    tail(&out.text, TRANSCRIPT_CAP)
                ));
            }
            _ => {}
        }
    }
    Ok(out)
}

async fn tool_workspace_delete(args: Value) -> Result<Value, String> {
    let target = req(&args, "name")?;
    if target == "default" {
        return Err("the Inbox can't be deleted".into());
    }
    let mut keys = vec![target.clone()];
    keys.extend(parzi_core::project::list(&target));
    let (cfg, store) = boot()?;
    let orch = Orchestrator::new(cfg, store.clone());
    let ids: Vec<String> = store
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|m| keys.iter().any(|k| k == &m.project))
        .map(|m| m.id)
        .collect();
    for id in &ids {
        let _ = orch.kill(id).await;
    }
    let mut n = 0;
    for key in &keys {
        n += store
            .delete_project_threads(key)
            .map_err(|e| e.to_string())?;
    }
    parzi_core::workspace::remove(&target).map_err(|e| e.to_string())?;
    Ok(json!({ "deleted": target, "deleted_sessions": n }))
}

async fn tool_project_delete(args: Value) -> Result<Value, String> {
    let ws = req(&args, "workspace")?;
    let slug = req(&args, "slug")?;
    // Confirm the slug belongs to this workspace first: slugs are only
    // unique per workspace, and sessions file under the slug.
    parzi_core::project::load(&ws, &slug).map_err(|e| e.to_string())?;
    let (cfg, store) = boot()?;
    let orch = Orchestrator::new(cfg, store.clone());
    let ids: Vec<String> = store
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|m| m.project == slug)
        .map(|m| m.id)
        .collect();
    for id in &ids {
        let _ = orch.kill(id).await;
    }
    let n = store
        .delete_project_threads(&slug)
        .map_err(|e| e.to_string())?;
    parzi_core::project::remove(&ws, &slug).map_err(|e| e.to_string())?;
    Ok(json!({ "deleted": slug, "deleted_sessions": n }))
}

/// Keep the tail; note when the head was cut so callers ask for the rest.
fn tail(s: &str, cap: usize) -> String {
    if s.len() <= cap {
        return s.to_string();
    }
    let cut = s.len() - cap;
    // Cut on a char boundary, then on a line boundary when one is near.
    let mut start = cut;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    let rest = &s[start..];
    let rest = rest.find('\n').map_or(rest, |i| &rest[i + 1..]);
    format!("[…{cut} chars truncated — use session_show for the full transcript…]\n{rest}")
}
