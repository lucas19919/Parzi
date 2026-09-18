//! Claude Code, driven the way Anthropic's own Agent SDK drives it: the
//! installed `claude` binary in stream-json mode, with approvals answered
//! over its stdio control channel (`--permission-prompt-tool stdio`). The
//! sign-in stays inside Claude Code; Parzi never reads a Claude token.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::sync::CancellationToken;

use crate::process::{self, Proc};
use crate::types::{
    tail, Access, ErrorClass, EventTx, ModelInfo, PermissionDecision, PermissionGate,
    PermissionRequest, Provider, ProviderError, ProviderEvent, ProviderStatus, State, TurnEnd,
    TurnSpec, UsageWindow,
};

pub const ID: &str = "claude";
const INSTALL_HINT: &str =
    "Install Claude Code (`npm i -g @anthropic-ai/claude-code`) or set its path in Settings.";
const LOGIN_HINT: &str = "Run `claude auth login` in a terminal, then check again.";

pub struct Claude {
    binary: String,
}

impl Claude {
    /// `binary` is the configured path, or empty for `claude` on PATH.
    pub fn new(binary: &str) -> Self {
        Self {
            binary: binary.to_string(),
        }
    }

    fn program(&self) -> Option<PathBuf> {
        process::resolve(if self.binary.trim().is_empty() {
            "claude"
        } else {
            &self.binary
        })
    }
}

#[async_trait::async_trait]
impl Provider for Claude {
    fn id(&self) -> &'static str {
        ID
    }

    async fn status(&self) -> ProviderStatus {
        let Some(program) = self.program() else {
            return ProviderStatus::new(ID, State::NotInstalled, INSTALL_HINT);
        };
        let version = process::first_line(&program, &["--version"])
            .await
            .map(|l| l.split_whitespace().next().unwrap_or(&l).to_string());
        let auth = process::output(&program, &["auth", "status"], 30)
            .await
            .and_then(|out| first_json_object(&out));
        let mut status = match &auth {
            Some(a) if a.get("loggedIn").and_then(Value::as_bool) == Some(true) => {
                let mut s = ProviderStatus::new(ID, State::Ready, "");
                s.account = Some(account_label(a));
                s
            }
            Some(_) => ProviderStatus::new(ID, State::SignedOut, LOGIN_HINT),
            None => ProviderStatus::new(
                ID,
                State::Error,
                "`claude auth status` gave no answer; check the Claude Code install.",
            ),
        };
        status.version = version;
        if status.state == State::Ready {
            match handshake(&program).await {
                Ok(init) => status.models = models_from_init(&init),
                Err(e) => status.hint = format!("Signed in, but the model list failed: {e}"),
            }
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
        let program = self
            .program()
            .ok_or_else(|| ProviderError::process(format!("Claude Code is not installed. {INSTALL_HINT}")))?;
        let scratch = TurnFiles::write(&spec)?;
        let args = turn_args(&spec, &scratch);
        let mut proc = Proc::spawn(&program, &args, &spec.cwd, &[])
            .map_err(|e| ProviderError::process(format!("could not start Claude Code: {e}")))?;
        let stdin = proc.child.stdin.take();
        let stdout = proc.child.stdout.take();
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            proc.kill().await;
            return Err(ProviderError::process("Claude Code started without stdio"));
        };
        let outcome = drive(stdout, stdin, &spec, gate, &events, &cancel).await;
        let outcome = match outcome {
            Err(e) if e.class == ErrorClass::Process => Err(ProviderError::process(format!(
                "{} {}",
                e.message,
                tail(&proc.stderr(), 600)
            ))),
            other => other,
        };
        // The CLI exits once stdin closes after a result; give it a moment,
        // then make sure nothing it started outlives the turn.
        let _ = tokio::time::timeout(Duration::from_secs(5), proc.child.wait()).await;
        proc.kill().await;
        scratch.remove();
        outcome
    }
}

/// Plan label from `claude auth status`.
fn account_label(auth: &Value) -> String {
    match auth.get("subscriptionType").and_then(Value::as_str) {
        Some("max") => "Claude Max".into(),
        Some("pro") => "Claude Pro".into(),
        Some("team") => "Claude Team".into(),
        Some("enterprise") => "Claude Enterprise".into(),
        Some(other) if !other.is_empty() => format!("Claude {other}"),
        _ => match auth.get("authMethod").and_then(Value::as_str) {
            Some(m) if m.contains("key") => "API key".into(),
            _ => "Claude account".into(),
        },
    }
}

fn first_json_object(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(text.get(start..=end)?).ok()
}

fn models_from_init(init: &Value) -> Vec<ModelInfo> {
    let Some(list) = init.get("models").and_then(Value::as_array) else {
        return vec![];
    };
    list.iter()
        .filter_map(|m| {
            let id = m.get("value").and_then(Value::as_str)?.to_string();
            let name = m
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string();
            let efforts = m
                .get("supportedEffortLevels")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            Some(ModelInfo {
                is_default: id == "default",
                id,
                name,
                efforts,
            })
        })
        .collect()
}

/// Start Claude Code, ask for its initialize answer (models, account), and
/// stop it. No prompt is written, so no request reaches Anthropic. User
/// settings are skipped so the probe never runs anyone's hooks.
async fn handshake(program: &Path) -> Result<Value, ProviderError> {
    let args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--permission-prompt-tool",
        "stdio",
        "--setting-sources=",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let cwd = std::env::temp_dir();
    let mut proc = Proc::spawn(program, &args, &cwd, &[])
        .map_err(|e| ProviderError::process(format!("could not start Claude Code: {e}")))?;
    let (Some(stdin), Some(stdout)) = (proc.child.stdin.take(), proc.child.stdout.take()) else {
        proc.kill().await;
        return Err(ProviderError::process("Claude Code started without stdio"));
    };
    let (link, mut incoming) = Link::start(stdout, stdin);
    let pump = tokio::spawn(async move {
        while let Some(msg) = incoming.recv().await {
            let _ = msg;
        }
    });
    let init = link
        .control(json!({"subtype": "initialize", "hooks": null}), Duration::from_secs(45))
        .await;
    pump.abort();
    proc.kill().await;
    init
}

/// Temporary files a turn hands to the CLI by path: long instructions and
/// the MCP config (which carries this run's secret) stay off the command
/// line, where Windows would cap them and anyone could read them.
struct TurnFiles {
    dir: PathBuf,
    instructions: Option<PathBuf>,
    mcp: Option<PathBuf>,
}

impl TurnFiles {
    fn write(spec: &TurnSpec) -> Result<Self, ProviderError> {
        let dir = std::env::temp_dir()
            .join("parzi-turns")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir)
            .map_err(|e| ProviderError::process(format!("turn scratch dir: {e}")))?;
        let mut files = Self {
            dir: dir.clone(),
            instructions: None,
            mcp: None,
        };
        if let Some(text) = spec.instructions.as_deref().filter(|t| !t.trim().is_empty()) {
            let p = dir.join("instructions.md");
            std::fs::write(&p, text).map_err(|e| ProviderError::process(e.to_string()))?;
            files.instructions = Some(p);
        }
        if let Some(t) = &spec.tools {
            let cfg = json!({"mcpServers": {t.name.clone(): {
                "type": "http",
                "url": t.url,
                "headers": {"Authorization": format!("Bearer {}", t.token)},
            }}});
            let p = dir.join("mcp.json");
            std::fs::write(&p, cfg.to_string()).map_err(|e| ProviderError::process(e.to_string()))?;
            files.mcp = Some(p);
        }
        Ok(files)
    }

    fn remove(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn permission_mode(access: Access) -> &'static str {
    match access {
        // Every change asks; Parzi's gate refuses or asks the person.
        Access::ReadOnly | Access::Ask | Access::Auto => "default",
        Access::Edits => "acceptEdits",
    }
}

fn turn_args(spec: &TurnSpec, files: &TurnFiles) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "stream-json",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
        "--permission-mode",
        permission_mode(spec.access),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(m) = spec.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
        a.push("--model".into());
        a.push(m.to_string());
    }
    if let Some(e) = spec.effort.as_deref().filter(|e| !e.is_empty()) {
        a.push("--effort".into());
        a.push(e.to_string());
    }
    match spec.resume.as_ref().and_then(resume_id) {
        Some(id) => a.push(format!("--resume={id}")),
        None => a.push(format!("--session-id={}", uuid::Uuid::new_v4())),
    }
    if let Some(p) = &files.instructions {
        a.push("--append-system-prompt-file".into());
        a.push(p.display().to_string());
    }
    if let (Some(p), Some(t)) = (&files.mcp, &spec.tools) {
        a.push("--mcp-config".into());
        a.push(p.display().to_string());
        // Parzi's own tools pass Parzi's gate when they run; Claude Code
        // asking first would make the person approve twice.
        a.push("--allowedTools".into());
        a.push(format!("mcp__{}", t.name));
    }
    a
}

fn resume_id(v: &Value) -> Option<String> {
    v.get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Stdio control channel: our control requests and their answers, plus the
/// stream of everything else the CLI prints.
struct Link {
    /// `None` once closed: dropping the pipe is what tells the CLI to exit.
    stdin: Mutex<Option<Box<dyn AsyncWrite + Send + Unpin>>>,
    waiters: std::sync::Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>,
    counter: AtomicU64,
}

impl Link {
    fn start<R, W>(reader: R, writer: W) -> (Arc<Self>, mpsc::UnboundedReceiver<Value>)
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let link = Arc::new(Self {
            stdin: Mutex::new(Some(Box::new(writer))),
            waiters: std::sync::Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let reading = link.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let text = String::from_utf8_lossy(&buf);
                let Ok(v) = serde_json::from_str::<Value>(text.trim()) else {
                    continue;
                };
                if v.get("type").and_then(Value::as_str) == Some("control_response") {
                    reading.settle(&v);
                } else if tx.send(v).is_err() {
                    break;
                }
            }
            reading.fail_all("Claude Code exited");
        });
        (link, rx)
    }

    async fn send(&self, v: &Value) -> Result<(), ProviderError> {
        let mut line = v.to_string();
        line.push('\n');
        let mut guard = self.stdin.lock().await;
        let w = guard
            .as_mut()
            .ok_or_else(|| ProviderError::process("Claude Code input is closed"))?;
        w.write_all(line.as_bytes())
            .await
            .map_err(|e| ProviderError::process(format!("writing to Claude Code: {e}")))?;
        w.flush()
            .await
            .map_err(|e| ProviderError::process(format!("writing to Claude Code: {e}")))
    }

    /// Close stdin: the CLI finishes the turn it has and exits.
    async fn close(&self) {
        if let Some(mut w) = self.stdin.lock().await.take() {
            let _ = w.shutdown().await;
        }
    }

    async fn control(&self, request: Value, limit: Duration) -> Result<Value, ProviderError> {
        let n = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        let id = format!("req_{n}_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let subtype = request
            .get("subtype")
            .and_then(Value::as_str)
            .unwrap_or("request")
            .to_string();
        let (tx, rx) = oneshot::channel();
        if let Ok(mut w) = self.waiters.lock() {
            w.insert(id.clone(), tx);
        }
        self.send(&json!({"type": "control_request", "request_id": id, "request": request}))
            .await?;
        match tokio::time::timeout(limit, rx).await {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(e))) => Err(ProviderError::new(ErrorClass::Unknown, format!("Claude Code refused {subtype}: {e}"))),
            Ok(Err(_)) => Err(ProviderError::process(format!("Claude Code exited during {subtype}"))),
            Err(_) => Err(ProviderError::process(format!(
                "Claude Code did not answer {subtype} within {}s",
                limit.as_secs()
            ))),
        }
    }

    fn settle(&self, v: &Value) {
        let Some(r) = v.get("response") else { return };
        let Some(id) = r.get("request_id").and_then(Value::as_str) else {
            return;
        };
        let Some(waiter) = self.waiters.lock().ok().and_then(|mut w| w.remove(id)) else {
            return;
        };
        let outcome = if r.get("subtype").and_then(Value::as_str) == Some("error") {
            Err(r
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_string())
        } else {
            Ok(r.get("response").cloned().unwrap_or(Value::Null))
        };
        let _ = waiter.send(outcome);
    }

    fn fail_all(&self, why: &str) {
        if let Ok(mut w) = self.waiters.lock() {
            for (_, waiter) in w.drain() {
                let _ = waiter.send(Err(why.to_string()));
            }
        }
    }

    async fn answer(&self, request_id: &str, response: Value) {
        let _ = self
            .send(&json!({
                "type": "control_response",
                "response": {"subtype": "success", "request_id": request_id, "response": response},
            }))
            .await;
    }

    async fn answer_error(&self, request_id: &str, error: &str) {
        let _ = self
            .send(&json!({
                "type": "control_response",
                "response": {"subtype": "error", "request_id": request_id, "error": error},
            }))
            .await;
    }
}

/// Everything one turn needs to keep between messages.
#[derive(Default)]
struct TurnState {
    tool_names: HashMap<String, String>,
    counted_messages: HashSet<String>,
    last_context: Option<u64>,
    interrupting: bool,
}

/// Run one turn over an already-started CLI's stdio. Split from
/// `run_turn` so the wire is testable without a real `claude`.
async fn drive<R, W>(
    reader: R,
    writer: W,
    spec: &TurnSpec,
    gate: Arc<dyn PermissionGate>,
    events: &EventTx,
    cancel: &CancellationToken,
) -> Result<TurnEnd, ProviderError>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let (link, mut incoming) = Link::start(reader, writer);
    link.control(json!({"subtype": "initialize", "hooks": null}), Duration::from_secs(90))
        .await?;
    let content = user_content(&spec.prompt, &spec.images);
    link.send(&json!({
        "type": "user",
        "message": {"role": "user", "content": content},
        "parent_tool_use_id": null,
        "session_id": "default",
    }))
    .await?;
    let mut st = TurnState::default();
    let mut stop_by: Option<tokio::time::Instant> = None;
    loop {
        let msg = tokio::select! {
            () = cancel.cancelled(), if !st.interrupting => {
                st.interrupting = true;
                stop_by = Some(tokio::time::Instant::now() + Duration::from_secs(10));
                let l = link.clone();
                tokio::spawn(async move {
                    let _ = l.control(json!({"subtype": "interrupt"}), Duration::from_secs(10)).await;
                });
                continue;
            }
            () = sleep_until(stop_by) => return Ok(TurnEnd::Interrupted),
            msg = incoming.recv() => msg,
        };
        let Some(v) = msg else {
            if st.interrupting {
                return Ok(TurnEnd::Interrupted);
            }
            return Err(ProviderError::process("Claude Code exited before the turn finished."));
        };
        if let Some(end) = handle(&v, &mut st, &link, &gate, events).await? {
            link.close().await;
            return Ok(if st.interrupting { TurnEnd::Interrupted } else { end });
        }
    }
}

async fn sleep_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d).await,
        None => std::future::pending().await,
    }
}

fn user_content(prompt: &str, images: &[PathBuf]) -> Value {
    if images.is_empty() {
        return json!(prompt);
    }
    let mut blocks = vec![json!({"type": "text", "text": prompt})];
    for p in images {
        if let Some((media_type, data)) = crate::image_base64(p) {
            blocks.push(json!({
                "type": "image",
                "source": {"type": "base64", "media_type": media_type, "data": data},
            }));
        }
    }
    Value::Array(blocks)
}

/// One stdout message. `Some(end)` = the turn is over.
async fn handle(
    v: &Value,
    st: &mut TurnState,
    link: &Arc<Link>,
    gate: &Arc<dyn PermissionGate>,
    events: &EventTx,
) -> Result<Option<TurnEnd>, ProviderError> {
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    // Subagent traffic carries its parent tool call; the thread shows the
    // main conversation, and the subagent's result arrives as that tool's
    // result.
    let main = v.get("parent_tool_use_id").is_none_or(Value::is_null);
    match kind {
        "control_request" => {
            let request_id = v.get("request_id").and_then(Value::as_str).unwrap_or("").to_string();
            let req = v.get("request").cloned().unwrap_or(Value::Null);
            if req.get("subtype").and_then(Value::as_str) == Some("can_use_tool") {
                let (link, gate) = (link.clone(), gate.clone());
                // A person may take minutes: never block the stream on it.
                tokio::spawn(async move {
                    let (request, input) = permission_request(&req);
                    let response = match gate.decide(request).await {
                        PermissionDecision::Allow | PermissionDecision::AllowAlways => {
                            json!({"behavior": "allow", "updatedInput": input})
                        }
                        PermissionDecision::Deny(why) => json!({"behavior": "deny", "message": why}),
                    };
                    link.answer(&request_id, response).await;
                });
            } else {
                link.answer_error(&request_id, "not supported by Parzi").await;
            }
        }
        "system" => match v.get("subtype").and_then(Value::as_str).unwrap_or("") {
            "init" => {
                if let Some(id) = v.get("session_id").and_then(Value::as_str) {
                    let _ = events.send(ProviderEvent::Session {
                        resume: json!({"session_id": id}),
                    });
                }
            }
            "api_retry" => {
                let error = v.get("error").and_then(Value::as_str).unwrap_or("error");
                let attempt = v.get("attempt").and_then(Value::as_u64).unwrap_or(0);
                let max = v.get("max_retries").and_then(Value::as_u64).unwrap_or(0);
                let _ = events.send(ProviderEvent::Notice(format!(
                    "Claude is retrying ({}, attempt {attempt} of {max})",
                    error.replace('_', " ")
                )));
            }
            "compact_boundary" => {
                let _ = events.send(ProviderEvent::Notice(
                    "Claude compacted the conversation".into(),
                ));
            }
            _ => {}
        },
        "stream_event" if main => {
            let ev = v.get("event").unwrap_or(&Value::Null);
            if ev.get("type").and_then(Value::as_str) == Some("content_block_delta") {
                let d = ev.get("delta").unwrap_or(&Value::Null);
                match d.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(t) = d.get("text").and_then(Value::as_str) {
                            let _ = events.send(ProviderEvent::TextDelta(t.to_string()));
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(t) = d.get("thinking").and_then(Value::as_str) {
                            let _ = events.send(ProviderEvent::ReasoningDelta(t.to_string()));
                        }
                    }
                    _ => {}
                }
            }
        }
        "assistant" if main => {
            let msg = v.get("message").unwrap_or(&Value::Null);
            let mut text = String::new();
            for block in msg.get("content").and_then(Value::as_array).into_iter().flatten() {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            text.push_str(t);
                        }
                    }
                    Some("thinking") => {
                        if let Some(t) = block.get("thinking").and_then(Value::as_str) {
                            if !t.trim().is_empty() {
                                let _ = events.send(ProviderEvent::Reasoning(t.to_string()));
                            }
                        }
                    }
                    Some("tool_use") => {
                        let id = block.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                        let name = block.get("name").and_then(Value::as_str).unwrap_or("tool").to_string();
                        st.tool_names.insert(id.clone(), name.clone());
                        let _ = events.send(ProviderEvent::ToolStarted {
                            id,
                            name,
                            input: block.get("input").cloned().unwrap_or(Value::Null),
                        });
                    }
                    _ => {}
                }
            }
            if !text.trim().is_empty() {
                let _ = events.send(ProviderEvent::Message(text));
            }
            // One API call can arrive split over several messages that share
            // its id and usage: count each call once.
            let mid = msg.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            if let Some(u) = msg.get("usage") {
                if mid.is_empty() || st.counted_messages.insert(mid) {
                    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                    let input = n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens");
                    let output = n("output_tokens");
                    st.last_context = Some(input + output);
                    let _ = events.send(ProviderEvent::Usage {
                        input,
                        output,
                        cost_usd: None,
                    });
                }
            }
        }
        "user" if main => {
            let msg = v.get("message").unwrap_or(&Value::Null);
            for block in msg.get("content").and_then(Value::as_array).into_iter().flatten() {
                if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                    continue;
                }
                let id = block.get("tool_use_id").and_then(Value::as_str).unwrap_or("").to_string();
                let name = st.tool_names.get(&id).cloned().unwrap_or_else(|| "tool".into());
                let _ = events.send(ProviderEvent::ToolFinished {
                    ok: !block.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                    output: result_text(block.get("content")),
                    id,
                    name,
                });
            }
        }
        "rate_limit_event" => {
            let info = v.get("rate_limit_info").unwrap_or(&Value::Null);
            if let Some(w) = usage_window(info) {
                if info.get("status").and_then(Value::as_str) == Some("rejected") {
                    let _ = events.send(ProviderEvent::Notice(format!(
                        "Claude's {} limit is used up",
                        w.label.to_lowercase()
                    )));
                }
                let _ = events.send(ProviderEvent::Limits(vec![w]));
            }
        }
        "result" => return finish(v, st, events).map(Some),
        _ => {}
    }
    Ok(None)
}

fn permission_request(req: &Value) -> (PermissionRequest, Value) {
    let tool = req.get("tool_name").and_then(Value::as_str).unwrap_or("tool").to_string();
    let input = req.get("input").cloned().unwrap_or(Value::Null);
    let title = req
        .get("title")
        .or_else(|| req.get("description"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| describe(&tool, &input));
    let paths = ["file_path", "notebook_path", "path"]
        .iter()
        .filter_map(|k| input.get(*k).and_then(Value::as_str))
        .map(str::to_string)
        .filter(|_| matches!(tool.as_str(), "Edit" | "MultiEdit" | "Write" | "NotebookEdit"))
        .collect();
    let request = PermissionRequest {
        id: req
            .get("tool_use_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        tool,
        title,
        input: input.clone(),
        paths,
    };
    (request, input)
}

fn describe(tool: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("");
    match tool {
        "Bash" => format!("Run `{}`", s("command")),
        "Edit" | "MultiEdit" => format!("Edit {}", s("file_path")),
        "Write" => format!("Write {}", s("file_path")),
        "WebFetch" => format!("Fetch {}", s("url")),
        other => other.to_string(),
    }
}

fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn usage_window(info: &Value) -> Option<UsageWindow> {
    let kind = info.get("rateLimitType").and_then(Value::as_str)?;
    let label = match kind {
        "five_hour" => "Session".to_string(),
        "seven_day" => "Weekly".to_string(),
        "seven_day_opus" => "Weekly · Opus".to_string(),
        "seven_day_sonnet" => "Weekly · Sonnet".to_string(),
        other => other.replace('_', " "),
    };
    let used = info.get("utilization").and_then(Value::as_f64)?;
    Some(UsageWindow {
        label,
        used_percent: (used * 100.0).clamp(0.0, 100.0),
        resets_at: info.get("resetsAt").and_then(Value::as_u64),
    })
}

fn finish(v: &Value, st: &TurnState, events: &EventTx) -> Result<TurnEnd, ProviderError> {
    if let Some(cost) = v.get("total_cost_usd").and_then(Value::as_f64) {
        let _ = events.send(ProviderEvent::Usage {
            input: 0,
            output: 0,
            cost_usd: Some(cost),
        });
    }
    let limit = v
        .get("modelUsage")
        .and_then(Value::as_object)
        .and_then(|m| {
            m.values()
                .filter_map(|u| u.get("contextWindow").and_then(Value::as_u64))
                .max()
        });
    if let (Some(used), Some(limit)) = (st.last_context, limit) {
        let _ = events.send(ProviderEvent::Context { used, limit });
    }
    let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("");
    let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
    if !is_error && subtype == "success" {
        return Ok(TurnEnd::Completed);
    }
    if subtype == "error_max_turns" {
        let _ = events.send(ProviderEvent::Notice(
            "Claude stopped at its turn limit".into(),
        ));
        return Ok(TurnEnd::Completed);
    }
    if st.interrupting {
        return Ok(TurnEnd::Interrupted);
    }
    let status = v.get("api_error_status").and_then(Value::as_u64);
    let class = match status {
        Some(401 | 403) => ErrorClass::Auth,
        Some(429) => ErrorClass::RateLimit,
        Some(413) => ErrorClass::ContextOverflow,
        Some(400) => ErrorClass::BadRequest,
        Some(500..=599) => ErrorClass::Overloaded,
        _ => ErrorClass::Unknown,
    };
    let mut message = v
        .get("result")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();
    if message.trim().is_empty() {
        message = v
            .get("errors")
            .and_then(Value::as_array)
            .map(|e| e.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; "))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("Claude Code ended the turn with `{subtype}`"));
    }
    if let Some(code) = status {
        message = format!("{message} (HTTP {code})");
    }
    Err(ProviderError::new(class, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PermissionDecision, PermissionGate, PermissionRequest};
    use tokio::io::AsyncBufReadExt;

    struct Gate(PermissionDecision);

    #[async_trait::async_trait]
    impl PermissionGate for Gate {
        async fn decide(&self, _r: PermissionRequest) -> PermissionDecision {
            self.0.clone()
        }
    }

    fn spec() -> TurnSpec {
        TurnSpec {
            session_id: "s".into(),
            cwd: std::env::temp_dir(),
            model: None,
            effort: None,
            access: Access::Ask,
            instructions: None,
            resume: None,
            prompt: "hi".into(),
            images: vec![],
            tools: None,
        }
    }

    /// Plays the CLI side: answers initialize, checks the prompt, asks one
    /// permission, then streams a turn with two tool calls and a result.
    async fn fake_cli(
        io: tokio::io::DuplexStream,
        script: Vec<Value>,
    ) -> Vec<Value> {
        let (r, mut w) = tokio::io::split(io);
        let mut lines = BufReader::new(r).lines();
        let mut seen = vec![];
        let init: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        let rid = init["request_id"].as_str().unwrap().to_string();
        seen.push(init);
        let ok = json!({"type": "control_response", "response": {"subtype": "success", "request_id": rid, "response": {}}});
        w.write_all(format!("{ok}\n").as_bytes()).await.unwrap();
        seen.push(serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap());
        for msg in script {
            let is_permission = msg["type"] == "control_request";
            w.write_all(format!("{msg}\n").as_bytes()).await.unwrap();
            if is_permission {
                seen.push(serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap());
            }
        }
        seen
    }

    #[tokio::test]
    async fn a_turn_streams_every_tool_call_and_asks_the_gate() {
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let script = vec![
            json!({"type": "system", "subtype": "hook_started"}),
            json!({"type": "system", "subtype": "init", "session_id": "sess-1"}),
            json!({"type": "control_request", "request_id": "p1", "request": {"subtype": "can_use_tool", "tool_name": "Edit", "input": {"file_path": "a.txt"}, "tool_use_id": "t1"}}),
            json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "Grüße"}}}),
            json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m1", "content": [
                {"type": "text", "text": "Grüße"},
                {"type": "tool_use", "id": "t1", "name": "Edit", "input": {"file_path": "a.txt"}},
                {"type": "tool_use", "id": "t2", "name": "Bash", "input": {"command": "ls"}}
            ], "usage": {"input_tokens": 10, "output_tokens": 5}}}),
            json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": "m1", "content": [], "usage": {"input_tokens": 10, "output_tokens": 5}}}),
            json!({"type": "user", "parent_tool_use_id": null, "message": {"content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "ok"},
                {"type": "tool_result", "tool_use_id": "t2", "content": [{"type": "text", "text": "a.txt"}], "is_error": true}
            ]}}),
            json!({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed", "rateLimitType": "five_hour", "utilization": 0.37, "resetsAt": 1_800_000_000u64}}),
            json!({"type": "result", "subtype": "success", "is_error": false, "total_cost_usd": 0.01, "modelUsage": {"claude-opus-5": {"contextWindow": 200000}}}),
        ];
        let cli = tokio::spawn(fake_cli(theirs, script));
        let (r, w) = tokio::io::split(ours);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let gate: Arc<dyn PermissionGate> = Arc::new(Gate(PermissionDecision::Allow));
        let end = drive(r, w, &spec(), gate, &tx, &CancellationToken::new()).await.unwrap();
        assert_eq!(end, TurnEnd::Completed);
        let seen = cli.await.unwrap();
        assert_eq!(seen[0]["request"]["subtype"], "initialize");
        assert_eq!(seen[1]["message"]["content"], "hi");
        assert_eq!(seen[2]["response"]["response"]["behavior"], "allow");
        assert_eq!(seen[2]["response"]["request_id"], "p1");
        drop(tx);
        let mut got = vec![];
        while let Some(e) = rx.recv().await {
            got.push(e);
        }
        let started: Vec<_> = got.iter().filter(|e| matches!(e, ProviderEvent::ToolStarted { .. })).collect();
        assert_eq!(started.len(), 2, "both tool calls: {got:?}");
        assert!(got.contains(&ProviderEvent::Session { resume: json!({"session_id": "sess-1"}) }));
        assert!(got.contains(&ProviderEvent::Message("Grüße".into())));
        assert!(got.iter().any(|e| matches!(e, ProviderEvent::ToolFinished { id, ok: false, .. } if id == "t2")));
        let usage: Vec<_> = got.iter().filter(|e| matches!(e, ProviderEvent::Usage { cost_usd: None, .. })).collect();
        assert_eq!(usage.len(), 1, "one API call counted once");
        assert!(got.iter().any(|e| matches!(e, ProviderEvent::Limits(w) if w[0].label == "Session" && (w[0].used_percent - 37.0).abs() < 0.01)));
        assert!(got.contains(&ProviderEvent::Context { used: 15, limit: 200_000 }));
    }

    #[tokio::test]
    async fn a_failed_turn_says_what_the_vendor_said() {
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let script = vec![json!({
            "type": "result", "subtype": "success", "is_error": true,
            "result": "Invalid API key · Please run /login", "api_error_status": 401
        })];
        let cli = tokio::spawn(fake_cli(theirs, script));
        let (r, w) = tokio::io::split(ours);
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let gate: Arc<dyn PermissionGate> = Arc::new(Gate(PermissionDecision::Allow));
        let err = drive(r, w, &spec(), gate, &tx, &CancellationToken::new()).await.unwrap_err();
        assert_eq!(err.class, ErrorClass::Auth);
        assert!(err.message.contains("Invalid API key") && err.message.contains("401"), "{err}");
        cli.await.unwrap();
    }

    #[test]
    fn edits_name_the_file_for_the_lease_gate() {
        let req = json!({"subtype": "can_use_tool", "tool_name": "Write", "input": {"file_path": "src/a.rs", "content": "x"}, "tool_use_id": "t9"});
        let (r, input) = permission_request(&req);
        assert_eq!(r.paths, vec!["src/a.rs".to_string()]);
        assert_eq!(input["content"], "x");
        let req = json!({"subtype": "can_use_tool", "tool_name": "Bash", "input": {"command": "rm -rf target"}});
        let (r, _) = permission_request(&req);
        assert!(r.paths.is_empty());
        assert_eq!(r.title, "Run `rm -rf target`");
    }

    #[test]
    fn models_come_with_their_effort_levels() {
        let init = json!({"models": [
            {"value": "default", "displayName": "Default (recommended)"},
            {"value": "opus[1m]", "displayName": "Opus (1M context)", "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"]}
        ]});
        let m = models_from_init(&init);
        assert_eq!(m.len(), 2);
        assert!(m[0].is_default);
        assert_eq!(m[1].efforts.len(), 5);
    }
}
