//! ACP agents (Agent Client Protocol): OpenCode, Grok, Antigravity and
//! Cursor each ship an agent that speaks it on stdio. One driver serves all
//! four; an [`Agent`] says how to start each one and how to check its
//! sign-in without opening a session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::jsonrpc::{Incoming, Peer, RpcError};
use crate::process::{self, Proc};
use crate::types::{
    tail, Access, ErrorClass, EventTx, ModelInfo, PermissionDecision, PermissionGate,
    PermissionRequest, Provider, ProviderError, ProviderEvent, ProviderStatus, State, TurnEnd,
    TurnSpec,
};

/// ACP's "authentication required" error code.
const AUTH_REQUIRED: i64 = -32000;

/// How a roster agent is started and checked.
#[derive(Clone, Copy)]
pub struct Agent {
    pub id: &'static str,
    pub name: &'static str,
    /// Program name on PATH when no path is configured.
    program: &'static str,
    args: fn(Access) -> Vec<String>,
    /// Extra environment, from the resolved program's location.
    env: fn(&Path) -> Vec<(String, String)>,
    /// Where the program lives when it is not on PATH.
    locate: fn() -> Option<PathBuf>,
    install_hint: &'static str,
    login_hint: &'static str,
    probe: Probe,
}

#[derive(Clone, Copy)]
enum Probe {
    /// `opencode models`: one `provider/model` per line, empty when no
    /// provider is signed in.
    OpencodeModels,
    /// `grok models`: says whether you are logged in, then the models.
    GrokModels,
    /// The agent offers no check short of a session: the first turn tells.
    None,
}

fn no_env(_: &Path) -> Vec<(String, String)> {
    vec![]
}

fn nowhere() -> Option<PathBuf> {
    None
}

pub fn agent(id: &str) -> Option<Agent> {
    Some(match id {
        "opencode" => Agent {
            id: "opencode",
            name: "OpenCode",
            program: "opencode",
            args: |_| vec!["acp".into()],
            env: no_env,
            locate: nowhere,
            install_hint: "Install OpenCode (`npm i -g opencode-ai`) or set its path in Settings.",
            login_hint: "Run `opencode auth login` in a terminal, then check again.",
            probe: Probe::OpencodeModels,
        },
        "grok" => Agent {
            id: "grok",
            name: "Grok",
            program: "grok",
            // Grok's own modes; every action outside them asks Parzi.
            args: |access| {
                let mode = if access == Access::Edits { "acceptEdits" } else { "default" };
                vec!["--permission-mode".into(), mode.into(), "agent".into(), "stdio".into()]
            },
            env: no_env,
            locate: nowhere,
            install_hint: "Install the Grok CLI or set its path in Settings.",
            login_hint: "Sign in with the Grok CLI (`grok`) in a terminal, then check again.",
            probe: Probe::GrokModels,
        },
        "antigravity" => Agent {
            id: "antigravity",
            name: "Antigravity",
            program: "agy_acp_server",
            args: |_| vec![],
            env: |program| {
                let harness = program.with_file_name(if cfg!(windows) {
                    "localharness_external.exe"
                } else {
                    "localharness_external"
                });
                vec![(
                    "ANTIGRAVITY_HARNESS_PATH".into(),
                    harness.display().to_string(),
                )]
            },
            locate: t3_antigravity,
            install_hint: "Install Google's Antigravity agent (T3 Code downloads it) or set the path to agy_acp_server in Settings.",
            // The agent keeps its Google sign-in under ~/.gemini, whichever
            // client started it.
            login_hint: "Sign in with Google in T3 Code's Antigravity settings, then check again.",
            probe: Probe::None,
        },
        "cursor" => Agent {
            id: "cursor",
            name: "Cursor",
            program: "cursor-agent",
            args: |_| vec!["acp".into()],
            env: no_env,
            locate: nowhere,
            install_hint: "Install Cursor's CLI (`cursor-agent`) or set its path in Settings.",
            login_hint: "Run `cursor-agent login` in a terminal, then check again.",
            probe: Probe::None,
        },
        _ => return None,
    })
}

/// Google's agent as T3 Code installs it: `~/.t3/tools/antigravity-acp/
/// <platform>/versions/<release>/agy_acp_server`, `active.json` naming the
/// release in use.
fn t3_antigravity() -> Option<PathBuf> {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", _) => "win32-x64",
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", _) => "darwin-x64",
        ("linux", "aarch64") => "linux-arm64",
        _ => "linux-x64",
    };
    let base = dirs::home_dir()?
        .join(".t3")
        .join("tools")
        .join("antigravity-acp")
        .join(platform);
    let active: Value = serde_json::from_str(&std::fs::read_to_string(base.join("active.json")).ok()?).ok()?;
    let release = active.get("releaseId")?.as_str()?;
    let exe = if cfg!(windows) { "agy_acp_server.exe" } else { "agy_acp_server" };
    let path = base.join("versions").join(release).join(exe);
    path.is_file().then_some(path)
}

pub struct Acp {
    agent: Agent,
    binary: String,
}

impl Acp {
    pub fn new(agent: Agent, binary: &str) -> Self {
        Self {
            agent,
            binary: binary.to_string(),
        }
    }

    fn program(&self) -> Option<PathBuf> {
        if !self.binary.trim().is_empty() {
            return process::resolve(&self.binary);
        }
        process::resolve(self.agent.program).or_else(self.agent.locate)
    }

    async fn open(&self, program: &Path, access: Access, cwd: &Path) -> Result<Conn, ProviderError> {
        let args = (self.agent.args)(access);
        let env = (self.agent.env)(program);
        let mut proc = Proc::spawn(program, &args, cwd, &env)
            .map_err(|e| ProviderError::process(format!("could not start {}: {e}", self.agent.name)))?;
        let (Some(stdin), Some(stdout)) = (proc.child.stdin.take(), proc.child.stdout.take()) else {
            proc.kill().await;
            return Err(ProviderError::process(format!("{} started without stdio", self.agent.name)));
        };
        let (peer, incoming) = Peer::start(stdout, stdin);
        let init = peer
            .request_within(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    // Parzi offers no file system or terminal of its own: the
                    // agent works with its own tools and asks before acting.
                    "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                    "clientInfo": {"name": "parzi", "title": "Parzi", "version": env!("CARGO_PKG_VERSION")},
                }),
                Duration::from_secs(60),
            )
            .await;
        match init {
            Ok(init) => Ok(Conn {
                proc,
                peer,
                incoming,
                init,
            }),
            Err(e) => {
                let err = ProviderError::process(format!(
                    "{} did not start: {e} {}",
                    self.agent.name,
                    tail(&proc.stderr(), 400)
                ));
                proc.kill().await;
                Err(err)
            }
        }
    }
}

struct Conn {
    proc: Proc,
    peer: Arc<Peer>,
    incoming: mpsc::UnboundedReceiver<Incoming>,
    init: Value,
}

#[async_trait::async_trait]
impl Provider for Acp {
    fn id(&self) -> &'static str {
        self.agent.id
    }

    async fn status(&self) -> ProviderStatus {
        let id = self.agent.id;
        let Some(program) = self.program() else {
            return ProviderStatus::new(id, State::NotInstalled, self.agent.install_hint);
        };
        let mut status = match self.agent.probe {
            Probe::OpencodeModels => opencode_status(&program, self.agent).await,
            Probe::GrokModels => grok_status(&program, self.agent).await,
            Probe::None => ProviderStatus::new(
                id,
                State::Unchecked,
                "Installed. Sign-in is checked when a thread starts.",
            ),
        };
        // The agent's own version, from a handshake that opens no session.
        if let Ok(mut conn) = self.open(&program, Access::Ask, &std::env::temp_dir()).await {
            status.version = conn
                .init
                .pointer("/agentInfo/version")
                .or_else(|| conn.init.pointer("/_meta/agentVersion"))
                .and_then(Value::as_str)
                .map(str::to_string);
            conn.proc.kill().await;
        }
        status
    }

    async fn run_turn(
        &self,
        spec: TurnSpec,
        gate: Arc<dyn PermissionGate>,
        events: EventTx,
        cancel: CancellationToken,
    ) -> Result<TurnEnd, ProviderError> {
        let program = self.program().ok_or_else(|| {
            ProviderError::process(format!(
                "{} is not installed. {}",
                self.agent.name, self.agent.install_hint
            ))
        })?;
        let mut conn = self.open(&program, spec.access, &spec.cwd).await?;
        let outcome = drive(
            &conn.peer,
            &mut conn.incoming,
            &conn.init,
            self.agent,
            &spec,
            gate,
            &events,
            &cancel,
        )
        .await;
        let outcome = match outcome {
            Err(e) if e.class == ErrorClass::Process => Err(ProviderError::process(format!(
                "{} {}",
                e.message,
                tail(&conn.proc.stderr(), 600)
            ))),
            other => other,
        };
        conn.proc.kill().await;
        outcome
    }
}

async fn opencode_status(program: &Path, agent: Agent) -> ProviderStatus {
    let Some(out) = process::output(program, &["models"], 60).await else {
        return ProviderStatus::new(agent.id, State::Error, "`opencode models` gave no answer.");
    };
    let models: Vec<ModelInfo> = out
        .lines()
        .map(str::trim)
        .filter(|l| l.contains('/') && !l.contains(' '))
        .map(|id| ModelInfo {
            id: id.to_string(),
            name: id.to_string(),
            is_default: false,
            efforts: vec![],
        })
        .collect();
    if models.is_empty() {
        return ProviderStatus::new(agent.id, State::SignedOut, agent.login_hint);
    }
    let mut providers: Vec<&str> = models.iter().filter_map(|m| m.id.split('/').next()).collect();
    providers.dedup();
    let mut s = ProviderStatus::new(agent.id, State::Ready, "");
    s.account = Some(format!(
        "{} provider{} connected",
        providers.len(),
        if providers.len() == 1 { "" } else { "s" }
    ));
    s.models = models;
    s
}

async fn grok_status(program: &Path, agent: Agent) -> ProviderStatus {
    match process::output(program, &["models"], 60).await {
        Some(out) => grok_read(&out, agent),
        None => ProviderStatus::new(agent.id, State::Error, "`grok models` gave no answer."),
    }
}

/// What `grok models` said. Listed models or "you are logged in" is ready —
/// checked first, because a sign-in refresh can print a "not logged in"
/// line on its way to succeeding. Anything unrecognised is an error in
/// Grok's own words, never a guess.
fn grok_read(out: &str, agent: Agent) -> ProviderStatus {
    let lower = out.to_lowercase();
    let mut s = ProviderStatus::new(agent.id, State::Ready, "");
    if lower.contains("you are logged in") {
        s.account = Some("Grok account".into());
    }
    for line in out.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix("* ").or_else(|| line.strip_prefix("- ")) else {
            continue;
        };
        let is_default = rest.contains("(default)");
        let id = rest.split_whitespace().next().unwrap_or("").to_string();
        if !id.is_empty() {
            s.models.push(ModelInfo {
                name: id.clone(),
                id,
                is_default,
                efforts: vec![],
            });
        }
    }
    if s.account.is_some() || !s.models.is_empty() {
        return s;
    }
    if lower.contains("not logged in") || lower.contains("not authenticated") {
        return ProviderStatus::new(agent.id, State::SignedOut, agent.login_hint);
    }
    ProviderStatus::new(
        agent.id,
        State::Error,
        format!("`grok models` said: {}", tail(out.trim(), 300)),
    )
}

/// Pick the option that carries a decision.
fn option_for(options: &[Value], decision: &PermissionDecision) -> Option<String> {
    let prefer: &[&str] = match decision {
        PermissionDecision::Allow => &["allow_once", "allow_always"],
        PermissionDecision::AllowAlways => &["allow_always", "allow_once"],
        PermissionDecision::Deny(_) => &["reject_once", "reject_always"],
    };
    prefer.iter().find_map(|kind| {
        options
            .iter()
            .find(|o| o.get("kind").and_then(Value::as_str) == Some(kind))
            .and_then(|o| o.get("optionId").and_then(Value::as_str))
            .map(str::to_string)
    })
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().map(text_of).collect::<Vec<_>>().join("\n"),
        Value::Object(o) => {
            if let Some(t) = o.get("text").and_then(Value::as_str) {
                return t.to_string();
            }
            o.get("content").map(text_of).unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// Words the transcript keeps between chunks.
#[derive(Default)]
struct Buffers {
    message: String,
    thought: String,
}

impl Buffers {
    fn flush(&mut self, events: &EventTx) {
        let thought = std::mem::take(&mut self.thought);
        if !thought.trim().is_empty() {
            let _ = events.send(ProviderEvent::Reasoning(thought));
        }
        let message = std::mem::take(&mut self.message);
        if !message.trim().is_empty() {
            let _ = events.send(ProviderEvent::Message(message));
        }
    }
}

fn rpc_failure(agent: Agent, e: RpcError) -> ProviderError {
    match e.code {
        AUTH_REQUIRED => ProviderError::new(
            ErrorClass::Auth,
            format!("{} is not signed in. {}", agent.name, agent.login_hint),
        ),
        RpcError::CLOSED | RpcError::TIMEOUT => ProviderError::process(e.message),
        _ => ProviderError::new(ErrorClass::Unknown, format!("{}: {}", agent.name, e.message)),
    }
}

/// Open (or resume) a session on an initialized agent and run one prompt.
#[allow(clippy::too_many_arguments)]
async fn drive(
    peer: &Arc<Peer>,
    incoming: &mut mpsc::UnboundedReceiver<Incoming>,
    init: &Value,
    agent: Agent,
    spec: &TurnSpec,
    gate: Arc<dyn PermissionGate>,
    events: &EventTx,
    cancel: &CancellationToken,
) -> Result<TurnEnd, ProviderError> {
    let caps = init.get("agentCapabilities").unwrap_or(&Value::Null);
    let http_mcp = caps.pointer("/mcpCapabilities/http").and_then(Value::as_bool) == Some(true);
    let can_resume = caps.pointer("/sessionCapabilities/resume").is_some_and(|v| !v.is_null());
    let images_ok = caps.pointer("/promptCapabilities/image").and_then(Value::as_bool) == Some(true);
    let mcp_servers = match &spec.tools {
        Some(t) if http_mcp => json!([{
            "type": "http",
            "name": t.name,
            "url": t.url,
            "headers": [{"name": "Authorization", "value": format!("Bearer {}", t.token)}],
        }]),
        Some(_) => {
            let _ = events.send(ProviderEvent::Notice(format!(
                "{} cannot reach Parzi's tools (it does not take HTTP tool servers)",
                agent.name
            )));
            json!([])
        }
        None => json!([]),
    };
    let cwd = spec.cwd.display().to_string();
    let limit = Duration::from_secs(120);
    let earlier = spec
        .resume
        .as_ref()
        .and_then(|r| r.get("session_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut fresh = true;
    let mut session = Value::Null;
    let mut sid = String::new();
    if let Some(id) = earlier {
        if can_resume {
            match peer
                .request_within(
                    "session/resume",
                    json!({"sessionId": id, "cwd": cwd, "mcpServers": mcp_servers}),
                    limit,
                )
                .await
            {
                Ok(v) => {
                    session = v;
                    sid = id;
                    fresh = false;
                }
                Err(e) if e.code == AUTH_REQUIRED => return Err(rpc_failure(agent, e)),
                Err(e) => {
                    let _ = events.send(ProviderEvent::Notice(format!(
                        "{} could not resume the earlier conversation ({e}); starting a new one",
                        agent.name
                    )));
                }
            }
        } else {
            let _ = events.send(ProviderEvent::Notice(format!(
                "{} cannot resume conversations; starting a new one",
                agent.name
            )));
        }
    }
    if fresh {
        session = peer
            .request_within("session/new", json!({"cwd": cwd, "mcpServers": mcp_servers}), limit)
            .await
            .map_err(|e| rpc_failure(agent, e))?;
        sid = session
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| ProviderError::new(ErrorClass::Unknown, format!("{} opened a session without an id", agent.name)))?;
    }
    let _ = events.send(ProviderEvent::Session {
        resume: json!({"session_id": sid}),
    });
    if let Some(model) = spec.model.as_deref().filter(|m| !m.is_empty()) {
        select_model(peer, &session, &sid, model, agent, events).await;
    }
    // ACP has no system prompt: a new session gets Parzi's instructions as
    // the head of its first prompt; a resumed one already has them.
    let mut text = spec.prompt.clone();
    if fresh {
        if let Some(i) = spec.instructions.as_deref().filter(|i| !i.trim().is_empty()) {
            text = format!("<instructions>\n{i}\n</instructions>\n\n{text}");
        }
    }
    let mut prompt = vec![json!({"type": "text", "text": text})];
    if images_ok {
        for p in &spec.images {
            if let Some((mime, data)) = crate::image_base64(p) {
                prompt.push(json!({"type": "image", "mimeType": mime, "data": data}));
            }
        }
    } else if !spec.images.is_empty() {
        let _ = events.send(ProviderEvent::Notice(format!("{} takes no images; sent text only", agent.name)));
    }
    let (done_tx, mut done_rx) = tokio::sync::oneshot::channel();
    {
        let (peer, params) = (peer.clone(), json!({"sessionId": sid, "prompt": prompt}));
        tokio::spawn(async move {
            let _ = done_tx.send(peer.request("session/prompt", params).await);
        });
    }
    let mut buffers = Buffers::default();
    let mut tool_names: HashMap<String, String> = HashMap::new();
    // ACP reports the session's running cost. A new session starts at zero;
    // a resumed one at the total its cursor carried from the last turn.
    let mut last_cost: Option<f64> = if fresh {
        Some(0.0)
    } else {
        spec.resume
            .as_ref()
            .and_then(|r| r.get("cost"))
            .and_then(Value::as_f64)
    };
    let mut interrupting = false;
    let mut stop_by: Option<tokio::time::Instant> = None;
    let mut open = true;
    loop {
        // Biased: every update queued before the prompt's answer is read
        // first, so the last chunks of a turn are never dropped. A closed
        // stream is switched off, or it would win every round.
        tokio::select! {
            biased;
            () = cancel.cancelled(), if !interrupting => {
                interrupting = true;
                stop_by = Some(tokio::time::Instant::now() + Duration::from_secs(10));
                let _ = peer.notify("session/cancel", json!({"sessionId": sid})).await;
            }
            () = sleep_until(stop_by) => {
                buffers.flush(events);
                return Ok(TurnEnd::Interrupted);
            }
            msg = incoming.recv(), if open => {
                let Some(msg) = msg else {
                    // The reader ended; the prompt answer (an error) is next.
                    open = false;
                    continue;
                };
                match msg {
                    Incoming::Notification { method, params } if method == "session/update" => {
                        if params.get("sessionId").and_then(Value::as_str).is_some_and(|s| s != sid) {
                            continue;
                        }
                        let u = params.get("update").unwrap_or(&Value::Null);
                        update(u, &sid, &mut buffers, &mut tool_names, &mut last_cost, events);
                    }
                    Incoming::Request { id, method, params } => {
                        if method == "session/request_permission" {
                            let (peer, gate) = (peer.clone(), gate.clone());
                            tokio::spawn(async move { permission(&peer, id, &params, gate).await });
                        } else {
                            let _ = peer.respond_error(id, -32601, &format!("Parzi does not offer {method}")).await;
                        }
                    }
                    _ => {}
                }
            }
            done = &mut done_rx => {
                buffers.flush(events);
                let answer = done.unwrap_or_else(|_| Err(RpcError { code: RpcError::CLOSED, message: format!("{} exited before the turn finished", agent.name), data: None }));
                let answer = match answer {
                    Ok(v) => v,
                    Err(_) if interrupting => return Ok(TurnEnd::Interrupted),
                    Err(e) => return Err(rpc_failure(agent, e)),
                };
                if let Some(u) = answer.get("usage").filter(|u| !u.is_null()) {
                    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                    let _ = events.send(ProviderEvent::Usage { input: n("inputTokens"), output: n("outputTokens"), cost_usd: None });
                }
                return match answer.get("stopReason").and_then(Value::as_str).unwrap_or("end_turn") {
                    "cancelled" => Ok(TurnEnd::Interrupted),
                    "refusal" => Err(ProviderError::new(ErrorClass::BadRequest, format!("{} refused to continue", agent.name))),
                    "max_tokens" => {
                        let _ = events.send(ProviderEvent::Notice(format!("{} hit its output limit", agent.name)));
                        Ok(TurnEnd::Completed)
                    }
                    "max_turn_requests" => {
                        let _ = events.send(ProviderEvent::Notice(format!("{} hit its step limit", agent.name)));
                        Ok(TurnEnd::Completed)
                    }
                    _ if interrupting => Ok(TurnEnd::Interrupted),
                    _ => Ok(TurnEnd::Completed),
                };
            }        }
    }
}

async fn select_model(peer: &Peer, session: &Value, sid: &str, model: &str, agent: Agent, events: &EventTx) {
    let limit = Duration::from_secs(30);
    let has_model_option = session
        .get("configOptions")
        .and_then(Value::as_array)
        .is_some_and(|opts| opts.iter().any(|o| o.get("id").and_then(Value::as_str) == Some("model")));
    let result = if has_model_option {
        peer.request_within(
            "session/set_config_option",
            json!({"sessionId": sid, "configId": "model", "value": model}),
            limit,
        )
        .await
        .map(|_| ())
    } else if session.get("models").is_some_and(|m| !m.is_null()) {
        peer.request_within("session/set_model", json!({"sessionId": sid, "modelId": model}), limit)
            .await
            .map(|_| ())
    } else {
        Ok(())
    };
    if let Err(e) = result {
        let _ = events.send(ProviderEvent::Notice(format!(
            "{} kept its own model: {model} was refused ({e})",
            agent.name
        )));
    }
}

fn update(
    u: &Value,
    sid: &str,
    buffers: &mut Buffers,
    tool_names: &mut HashMap<String, String>,
    last_cost: &mut Option<f64>,
    events: &EventTx,
) {
    let chunk = || u.get("content").map(text_of).unwrap_or_default();
    match u.get("sessionUpdate").and_then(Value::as_str).unwrap_or("") {
        "agent_message_chunk" => {
            let t = chunk();
            buffers.message.push_str(&t);
            let _ = events.send(ProviderEvent::TextDelta(t));
        }
        "agent_thought_chunk" => {
            let t = chunk();
            buffers.thought.push_str(&t);
            let _ = events.send(ProviderEvent::ReasoningDelta(t));
        }
        "tool_call" => {
            buffers.flush(events);
            let id = u.get("toolCallId").and_then(Value::as_str).unwrap_or("").to_string();
            let name = u
                .get("title")
                .or_else(|| u.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            tool_names.insert(id.clone(), name.clone());
            let _ = events.send(ProviderEvent::ToolStarted {
                id: id.clone(),
                name: name.clone(),
                input: u.get("rawInput").cloned().unwrap_or(Value::Null),
            });
            finish_tool(u, &id, &name, events);
        }
        "tool_call_update" => {
            let id = u.get("toolCallId").and_then(Value::as_str).unwrap_or("").to_string();
            let name = tool_names.get(&id).cloned().unwrap_or_else(|| "tool".into());
            finish_tool(u, &id, &name, events);
        }
        "usage_update" => {
            if let (Some(used), Some(limit)) = (
                u.get("used").and_then(Value::as_u64),
                u.get("size").and_then(Value::as_u64),
            ) {
                let _ = events.send(ProviderEvent::Context { used, limit });
            }
            // The cost is the session's running total: bill the increase, and
            // keep the total in the cursor so the next turn starts from it.
            if let Some(total) = u.pointer("/cost/amount").and_then(Value::as_f64) {
                let delta = total - last_cost.unwrap_or(total);
                *last_cost = Some(total);
                if delta > 0.0 {
                    let _ = events.send(ProviderEvent::Usage { input: 0, output: 0, cost_usd: Some(delta) });
                }
                let _ = events.send(ProviderEvent::Session {
                    resume: json!({"session_id": sid, "cost": total}),
                });
            }
        }
        _ => {}
    }
}

fn finish_tool(u: &Value, id: &str, name: &str, events: &EventTx) {
    let status = u.get("status").and_then(Value::as_str).unwrap_or("");
    if status != "completed" && status != "failed" {
        return;
    }
    let mut output = u.get("content").map(text_of).unwrap_or_default();
    if output.trim().is_empty() {
        output = u
            .get("rawOutput")
            .map(|o| o.as_str().map_or_else(|| o.to_string(), str::to_string))
            .unwrap_or_default();
    }
    let _ = events.send(ProviderEvent::ToolFinished {
        id: id.to_string(),
        name: name.to_string(),
        ok: status == "completed",
        output,
    });
}

async fn permission(peer: &Peer, id: Value, params: &Value, gate: Arc<dyn PermissionGate>) {
    let call = params.get("toolCall").unwrap_or(&Value::Null);
    let kind = call.get("kind").and_then(Value::as_str).unwrap_or("other");
    let paths: Vec<String> = if matches!(kind, "edit" | "delete" | "move") {
        call.get("locations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|l| l.get("path").and_then(Value::as_str).map(str::to_string))
            .collect()
    } else {
        vec![]
    };
    let request = PermissionRequest {
        id: call.get("toolCallId").and_then(Value::as_str).unwrap_or("").to_string(),
        tool: kind.to_string(),
        title: call
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(kind)
            .to_string(),
        input: call.get("rawInput").cloned().unwrap_or(Value::Null),
        paths,
    };
    let options: Vec<Value> = params
        .get("options")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let decision = gate.decide(request).await;
    let outcome = match option_for(&options, &decision) {
        Some(option_id) => json!({"outcome": {"outcome": "selected", "optionId": option_id}}),
        None => json!({"outcome": {"outcome": "cancelled"}}),
    };
    let _ = peer.respond(id, outcome).await;
}

async fn sleep_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolServer;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    struct Gate(PermissionDecision, std::sync::Mutex<Vec<PermissionRequest>>);

    #[async_trait::async_trait]
    impl PermissionGate for Gate {
        async fn decide(&self, r: PermissionRequest) -> PermissionDecision {
            self.1.lock().unwrap().push(r);
            self.0.clone()
        }
    }

    fn spec(resume: Option<Value>) -> TurnSpec {
        TurnSpec {
            session_id: "s".into(),
            cwd: std::env::temp_dir(),
            model: Some("opencode/big-pickle".into()),
            effort: None,
            access: Access::Ask,
            instructions: Some("stay in scope".into()),
            resume,
            prompt: "hi".into(),
            images: vec![],
            tools: Some(ToolServer {
                name: "parzi".into(),
                url: "http://127.0.0.1:1/mcp/t".into(),
                token: "t".into(),
            }),
        }
    }

    async fn read(lines: &mut tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>) -> Value {
        serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap()
    }

    async fn send(w: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>, v: Value) {
        w.write_all(format!("{v}\n").as_bytes()).await.unwrap();
    }

    fn init() -> Value {
        json!({"protocolVersion": 1, "agentCapabilities": {
            "loadSession": true,
            "mcpCapabilities": {"http": true},
            "promptCapabilities": {"image": true},
            "sessionCapabilities": {"resume": {}}
        }})
    }

    #[tokio::test]
    async fn a_new_session_gets_parzi_tools_instructions_and_a_gate() {
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let (r, mut w) = tokio::io::split(theirs);
            let mut lines = BufReader::new(r).lines();
            let new = read(&mut lines).await;
            assert_eq!(new["method"], "session/new");
            assert_eq!(new["params"]["mcpServers"][0]["type"], "http");
            assert_eq!(new["params"]["mcpServers"][0]["name"], "parzi");
            send(&mut w, json!({"jsonrpc": "2.0", "id": new["id"], "result": {"sessionId": "S1", "configOptions": [{"id": "model", "type": "select"}]}})).await;
            let model = read(&mut lines).await;
            assert_eq!(model["method"], "session/set_config_option");
            assert_eq!(model["params"]["value"], "opencode/big-pickle");
            send(&mut w, json!({"jsonrpc": "2.0", "id": model["id"], "result": {}})).await;
            let prompt = read(&mut lines).await;
            assert_eq!(prompt["method"], "session/prompt");
            let text = prompt["params"]["prompt"][0]["text"].as_str().unwrap();
            assert!(text.starts_with("<instructions>\nstay in scope"), "{text}");
            let update = |u: Value| json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "S1", "update": u}});
            send(&mut w, update(json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "plan"}}))).await;
            send(&mut w, update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Edit"}}))).await;
            send(&mut w, update(json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "ing"}}))).await;
            send(&mut w, update(json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "Edit a.rs", "kind": "edit", "status": "pending", "rawInput": {"path": "a.rs"}}))).await;
            send(&mut w, json!({"jsonrpc": "2.0", "id": 9, "method": "session/request_permission", "params": {
                "sessionId": "S1",
                "toolCall": {"toolCallId": "c1", "title": "Edit a.rs", "kind": "edit", "locations": [{"path": "a.rs"}]},
                "options": [{"optionId": "yes", "kind": "allow_once"}, {"optionId": "no", "kind": "reject_once"}]
            }})).await;
            let answer = read(&mut lines).await;
            assert_eq!(answer["id"], 9);
            assert_eq!(answer["result"]["outcome"]["optionId"], "no");
            send(&mut w, update(json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "failed", "content": [{"type": "content", "content": {"type": "text", "text": "denied"}}]}))).await;
            send(&mut w, update(json!({"sessionUpdate": "usage_update", "used": 900, "size": 200000, "cost": {"amount": 0.02, "currency": "USD"}}))).await;
            send(&mut w, update(json!({"sessionUpdate": "usage_update", "used": 950, "size": 200000, "cost": {"amount": 0.05, "currency": "USD"}}))).await;
            send(&mut w, json!({"jsonrpc": "2.0", "id": prompt["id"], "result": {"stopReason": "end_turn"}})).await;
        });
        let (r, w) = tokio::io::split(ours);
        let (peer, mut incoming) = Peer::start(r, w);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let gate = Arc::new(Gate(PermissionDecision::Deny("lease held".into()), std::sync::Mutex::new(vec![])));
        let end = drive(&peer, &mut incoming, &init(), agent("opencode").unwrap(), &spec(None), gate.clone(), &tx, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(end, TurnEnd::Completed);
        server.await.unwrap();
        assert_eq!(gate.1.lock().unwrap()[0].paths, vec!["a.rs".to_string()]);
        drop(tx);
        let mut got = vec![];
        while let Some(e) = rx.recv().await {
            got.push(e);
        }
        assert!(got.contains(&ProviderEvent::Session { resume: json!({"session_id": "S1"}) }));
        assert!(got.contains(&ProviderEvent::Reasoning("plan".into())));
        assert!(got.contains(&ProviderEvent::Message("Editing".into())));
        assert!(got.iter().any(|e| matches!(e, ProviderEvent::ToolFinished { id, ok: false, output, .. } if id == "c1" && output == "denied")));
        let cost: f64 = got
            .iter()
            .filter_map(|e| match e { ProviderEvent::Usage { cost_usd: Some(c), .. } => Some(*c), _ => None })
            .sum();
        assert!((cost - 0.05).abs() < 1e-9, "a new session bills from zero: {cost}");
        assert!(got.contains(&ProviderEvent::Session { resume: json!({"session_id": "S1", "cost": 0.05}) }));
    }

    #[tokio::test]
    async fn a_resumed_session_skips_the_instructions() {
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let (r, mut w) = tokio::io::split(theirs);
            let mut lines = BufReader::new(r).lines();
            let resume = read(&mut lines).await;
            assert_eq!(resume["method"], "session/resume");
            assert_eq!(resume["params"]["sessionId"], "S0");
            send(&mut w, json!({"jsonrpc": "2.0", "id": resume["id"], "result": {}})).await;
            let prompt = read(&mut lines).await;
            assert_eq!(prompt["params"]["sessionId"], "S0");
            assert_eq!(prompt["params"]["prompt"][0]["text"], "hi");
            send(&mut w, json!({"jsonrpc": "2.0", "id": prompt["id"], "result": {"stopReason": "cancelled"}})).await;
        });
        let (r, w) = tokio::io::split(ours);
        let (peer, mut incoming) = Peer::start(r, w);
        let (tx, _rx) = mpsc::unbounded_channel();
        let gate = Arc::new(Gate(PermissionDecision::Allow, std::sync::Mutex::new(vec![])));
        let mut s = spec(Some(json!({"session_id": "S0"})));
        s.model = None;
        let end = drive(&peer, &mut incoming, &init(), agent("opencode").unwrap(), &s, gate, &tx, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(end, TurnEnd::Interrupted);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn auth_required_names_the_way_in() {
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let (r, mut w) = tokio::io::split(theirs);
            let mut lines = BufReader::new(r).lines();
            let new = read(&mut lines).await;
            send(&mut w, json!({"jsonrpc": "2.0", "id": new["id"], "error": {"code": -32000, "message": "Authentication required"}})).await;
        });
        let (r, w) = tokio::io::split(ours);
        let (peer, mut incoming) = Peer::start(r, w);
        let (tx, _rx) = mpsc::unbounded_channel();
        let gate = Arc::new(Gate(PermissionDecision::Allow, std::sync::Mutex::new(vec![])));
        let err = drive(&peer, &mut incoming, &init(), agent("grok").unwrap(), &spec(None), gate, &tx, &CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(err.class, ErrorClass::Auth);
        assert!(err.message.contains("Grok"), "{err}");
        server.await.unwrap();
    }

    #[test]
    fn grok_models_output_reads_sign_in_and_models() {
        let grok = agent("grok").unwrap();
        // Verbatim from grok 1.0.34.
        let out = "You are logged in with grok.com.\n\nDefault model: grok-4.6\n\nAvailable models:\n  * grok-4.6 (default)\n  - grok-4.5\n";
        let s = grok_read(out, grok);
        assert_eq!(s.state, State::Ready);
        let ids: Vec<(&str, bool)> = s.models.iter().map(|m| (m.id.as_str(), m.is_default)).collect();
        assert_eq!(ids, vec![("grok-4.6", true), ("grok-4.5", false)]);

        let refreshed = format!("Token expired, not logged in; refreshing…\n{out}");
        assert_eq!(grok_read(&refreshed, grok).state, State::Ready, "a refresh on the way is not a sign-out");
        assert_eq!(
            grok_read("You are not logged in. Run `grok login`.\n", grok).state,
            State::SignedOut
        );
        let odd = grok_read("error: network unreachable\n", grok);
        assert_eq!(odd.state, State::Error);
        assert!(odd.hint.contains("network unreachable"), "{}", odd.hint);
    }
}
