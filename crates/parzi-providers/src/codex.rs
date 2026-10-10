use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::jsonrpc::{Incoming, Peer, RpcError};
use crate::process::{self, Proc, STOP_GRACE};
use crate::types::{
    or_cancel, sleep_until, tail, ErrorClass, EventTx, ModelInfo, PermissionDecision,
    PermissionGate, PermissionRequest, Provider, ProviderError, ProviderEvent, ProviderStatus,
    State, TurnEnd, TurnSpec, UsageWindow,
};

pub const ID: &str = "codex";
const INSTALL_HINT: &str = "Install Codex from Set up Parzi, or set its path in Settings.";
const LOGIN_HINT: &str = "Sign in from Set up Parzi (it runs `codex login`), then check again.";

pub struct Codex {
    binary: String,
}

impl Codex {
    pub fn new(binary: &str) -> Self {
        Self {
            binary: binary.to_string(),
        }
    }

    fn program(&self) -> Option<PathBuf> {
        process::resolve(if self.binary.trim().is_empty() {
            "codex"
        } else {
            &self.binary
        })
    }
}

struct Server {
    proc: Proc,
    peer: Arc<Peer>,
    incoming: mpsc::UnboundedReceiver<Incoming>,
    version: Option<String>,
}

async fn open(program: &Path, cwd: &Path) -> Result<Server, ProviderError> {
    process::existing_folder(cwd)?;
    let mut proc = Proc::spawn(program, &["app-server".to_string()], cwd, &[])
        .map_err(|e| ProviderError::process(format!("could not start Codex: {e}")))?;
    let (Some(stdin), Some(stdout)) = (proc.child.stdin.take(), proc.child.stdout.take()) else {
        proc.kill().await;
        return Err(ProviderError::process("Codex started without stdio"));
    };
    let (peer, incoming) = Peer::start("Codex", stdout, stdin);
    let init = peer
        .request_within(
            "initialize",
            json!({"clientInfo": {"name": "parzi", "title": "Parzi", "version": env!("CARGO_PKG_VERSION")}}),
            Duration::from_secs(30),
        )
        .await;
    let init = match init {
        Ok(v) => v,
        Err(e) => {
            let err = ProviderError::process(format!(
                "Codex app-server did not start: {e} {}",
                tail(&proc.stderr(), 400)
            ));
            proc.kill().await;
            return Err(err);
        }
    };
    let _ = peer.notify("initialized", Value::Null).await;
    let version = init
        .get("userAgent")
        .and_then(Value::as_str)
        .and_then(|ua| ua.split('/').nth(1))
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_string);
    Ok(Server {
        proc,
        peer,
        incoming,
        version,
    })
}

#[async_trait::async_trait]
impl Provider for Codex {
    fn id(&self) -> &'static str {
        ID
    }

    fn gated(&self) -> bool {
        true
    }

    async fn status(&self) -> ProviderStatus {
        let Some(program) = self.program() else {
            return ProviderStatus::new(ID, State::NotInstalled, INSTALL_HINT);
        };
        let mut server = match open(&program, &std::env::temp_dir()).await {
            Ok(s) => s,
            Err(e) => return ProviderStatus::new(ID, State::Error, e.message),
        };
        let status = probe(&server.peer, server.version.clone()).await;
        server.proc.kill().await;
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
            ProviderError::process(format!("Codex is not installed. {INSTALL_HINT}"))
        })?;
        // Dropping a half-open server kills it.
        let Some(server) = or_cancel(&cancel, open(&program, &spec.cwd)).await else {
            return Ok(TurnEnd::Interrupted);
        };
        let mut server = server?;
        let outcome = drive(
            &server.peer,
            &mut server.incoming,
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
                tail(&server.proc.stderr(), 600)
            ))),
            other => other,
        };
        server.proc.kill().await;
        outcome
    }
}

async fn probe(peer: &Peer, version: Option<String>) -> ProviderStatus {
    let limit = Duration::from_secs(20);
    let account = match peer.request_within("account/read", json!({}), limit).await {
        Ok(v) => v,
        Err(e) => {
            let mut s =
                ProviderStatus::new(ID, State::Error, format!("Codex account check failed: {e}"));
            s.version = version;
            return s;
        }
    };
    let signed_in = account.get("account").is_some_and(|a| !a.is_null());
    if !signed_in && account.get("requiresOpenaiAuth").and_then(Value::as_bool) == Some(true) {
        let mut s = ProviderStatus::new(ID, State::SignedOut, LOGIN_HINT);
        s.version = version;
        return s;
    }
    let mut s = ProviderStatus::new(ID, State::Ready, "");
    s.version = version;
    s.account = account.get("account").and_then(account_label);
    s.models = list_models(peer).await;
    if account.pointer("/account/type").and_then(Value::as_str) != Some("apiKey") {
        if let Ok(r) = peer
            .request_within("account/rateLimits/read", Value::Null, limit)
            .await
        {
            s.usage = windows(r.get("rateLimits").unwrap_or(&Value::Null));
        }
    }
    s
}

fn account_label(a: &Value) -> Option<String> {
    match a.get("type").and_then(Value::as_str)? {
        "chatgpt" => Some(match a.get("planType").and_then(Value::as_str) {
            Some(plan) if !plan.is_empty() => format!("ChatGPT {}", title_case(plan)),
            _ => "ChatGPT".to_string(),
        }),
        "apiKey" => Some("API key".into()),
        other => Some(title_case(other)),
    }
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

async fn list_models(peer: &Peer) -> Vec<ModelInfo> {
    let mut out = vec![];
    let mut cursor: Option<String> = None;
    for _ in 0..5 {
        let params = match &cursor {
            Some(c) => json!({"cursor": c}),
            None => json!({}),
        };
        let Ok(page) = peer
            .request_within("model/list", params, Duration::from_secs(20))
            .await
        else {
            break;
        };
        for m in page
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if m.get("hidden").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let Some(id) = m
                .get("model")
                .or_else(|| m.get("id"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            out.push(ModelInfo {
                id: id.to_string(),
                name: m
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .to_string(),
                is_default: m.get("isDefault").and_then(Value::as_bool).unwrap_or(false),
                efforts: m
                    .get("supportedReasoningEfforts")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|e| e.get("reasoningEffort").and_then(Value::as_str))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
        cursor = page
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if cursor.is_none() {
            break;
        }
    }
    out
}

fn windows(snapshot: &Value) -> Vec<UsageWindow> {
    let mut out = vec![];
    for key in ["primary", "secondary"] {
        let Some(w) = snapshot.get(key).filter(|w| !w.is_null()) else {
            continue;
        };
        let Some(used) = w.get("usedPercent").and_then(Value::as_f64) else {
            continue;
        };
        let mins = w.get("windowDurationMins").and_then(Value::as_u64);
        let label = match mins {
            Some(m) if m <= 6 * 60 => "Session".to_string(),
            Some(m) if m >= 6 * 24 * 60 => "Weekly".to_string(),
            Some(m) => format!("{} h", m / 60),
            None => title_case(key),
        };
        out.push(UsageWindow {
            label,
            used_percent: used.clamp(0.0, 100.0),
            resets_at: w.get("resetsAt").and_then(Value::as_u64),
        });
    }
    out
}

const APPROVAL: &str = "untrusted";
const SANDBOX: &str = "read-only";

fn thread_id(v: &Value) -> Option<String> {
    v.pointer("/thread/id")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn effort(e: &str) -> &str {
    match e {
        "extra" | "ultra" | "max" => "xhigh",
        other => other,
    }
}

async fn drive(
    peer: &Arc<Peer>,
    incoming: &mut mpsc::UnboundedReceiver<Incoming>,
    spec: &TurnSpec,
    gate: Arc<dyn PermissionGate>,
    events: &EventTx,
    cancel: &CancellationToken,
) -> Result<TurnEnd, ProviderError> {
    let mut thread = json!({
        "cwd": spec.cwd.display().to_string(),
        "approvalPolicy": APPROVAL,
        "sandbox": SANDBOX,
    });
    if let Some(m) = spec.model.as_deref().filter(|m| !m.is_empty()) {
        thread["model"] = json!(m);
    }
    if let Some(i) = spec
        .instructions
        .as_deref()
        .filter(|i| !i.trim().is_empty())
    {
        thread["developerInstructions"] = json!(i);
    }
    if let Some(t) = &spec.tools {
        thread["config"] = json!({"mcp_servers": {t.name.clone(): {"url": t.url}}});
    }
    let limit = Duration::from_secs(60);
    let resume = spec
        .resume
        .as_ref()
        .and_then(|r| r.get("thread_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let tid = match resume {
        Some(tid) => {
            let mut p = thread.clone();
            p["threadId"] = json!(tid);
            match or_cancel(cancel, peer.request_within("thread/resume", p, limit)).await {
                None => return Ok(TurnEnd::Interrupted),
                Some(Ok(v)) => thread_id(&v).unwrap_or(tid),
                Some(Err(e)) if e.not_found() => {
                    return Err(ProviderError::new(
                        ErrorClass::SessionLost,
                        format!("Codex could not resume the conversation: {e}"),
                    ));
                }
                Some(Err(e)) => return Err(rpc_failure(e)),
            }
        }
        None => match or_cancel(cancel, start_thread(peer, thread, limit)).await {
            None => return Ok(TurnEnd::Interrupted),
            Some(tid) => tid?,
        },
    };
    let _ = events.send(ProviderEvent::Session {
        resume: json!({"thread_id": tid}),
    });
    let mut input = vec![json!({"type": "text", "text": spec.prompt})];
    for p in &spec.images {
        input.push(json!({"type": "localImage", "path": p.display().to_string()}));
    }
    let mut turn = json!({
        "threadId": tid,
        "input": input,
        "approvalPolicy": APPROVAL,
        "sandboxPolicy": {"type": "readOnly"},
    });
    if let Some(m) = spec.model.as_deref().filter(|m| !m.is_empty()) {
        turn["model"] = json!(m);
    }
    if let Some(e) = spec.effort.as_deref().filter(|e| !e.is_empty()) {
        turn["effort"] = json!(effort(e));
    }
    let Some(started) = or_cancel(cancel, peer.request_within("turn/start", turn, limit)).await
    else {
        return Ok(TurnEnd::Interrupted);
    };
    let started = started.map_err(rpc_failure)?;
    let turn_id = started
        .pointer("/turn/id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut st = Turn {
        thread: tid,
        turn: turn_id,
        file_paths: HashMap::new(),
        last_error: None,
        interrupting: false,
    };
    let mut stop_by: Option<tokio::time::Instant> = None;
    loop {
        let msg = tokio::select! {
            () = cancel.cancelled(), if !st.interrupting => {
                st.interrupting = true;
                stop_by = Some(tokio::time::Instant::now() + STOP_GRACE);
                let (p, params) = (peer.clone(), json!({"threadId": st.thread, "turnId": st.turn}));
                tokio::spawn(async move {
                    let _ = p.request_within("turn/interrupt", params, STOP_GRACE).await;
                });
                continue;
            }
            () = sleep_until(stop_by) => return Ok(TurnEnd::Interrupted),
            msg = incoming.recv() => msg,
        };
        let Some(msg) = msg else {
            if st.interrupting {
                return Ok(TurnEnd::Interrupted);
            }
            return Err(ProviderError::process(
                "Codex exited before the turn finished.",
            ));
        };
        match msg {
            Incoming::Notification { method, params } => {
                if let Some(end) = st.notification(&method, &params, events)? {
                    return Ok(end);
                }
            }
            Incoming::Request { id, method, params } => {
                let (peer, gate) = (peer.clone(), gate.clone());
                let paths = params
                    .get("itemId")
                    .and_then(Value::as_str)
                    .and_then(|i| st.file_paths.get(i).cloned())
                    .unwrap_or_default();
                tokio::spawn(async move { answer(&peer, id, &method, params, paths, gate).await });
            }
            Incoming::Raw(_) => {}
        }
    }
}

async fn start_thread(
    peer: &Peer,
    params: Value,
    limit: Duration,
) -> Result<String, ProviderError> {
    let v = peer
        .request_within("thread/start", params, limit)
        .await
        .map_err(rpc_failure)?;
    thread_id(&v).ok_or_else(|| {
        ProviderError::new(ErrorClass::Unknown, "Codex started a thread without an id")
    })
}

fn rpc_failure(e: RpcError) -> ProviderError {
    if e.code == RpcError::CLOSED || e.code == RpcError::TIMEOUT {
        return ProviderError::process(e.message);
    }
    ProviderError::new(
        ErrorClass::Unknown,
        format!("Codex refused the request: {e}"),
    )
}

async fn answer(
    peer: &Peer,
    id: Value,
    method: &str,
    params: Value,
    paths: Vec<String>,
    gate: Arc<dyn PermissionGate>,
) {
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            let command = params.get("command").and_then(Value::as_str).unwrap_or("");
            let (tool, title) = if method.contains("commandExecution") {
                ("shell", format!("Run `{command}`"))
            } else if paths.is_empty() {
                ("edit", "Edit files".to_string())
            } else {
                ("edit", format!("Edit {}", paths.join(", ")))
            };
            let request = PermissionRequest {
                id: params
                    .get("itemId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                tool: tool.to_string(),
                title,
                input: params.clone(),
                paths,
            };
            let decision = match gate.decide(request).await {
                PermissionDecision::Allow => "accept",
                PermissionDecision::Deny(_) => "decline",
            };
            let _ = peer.respond(id, json!({"decision": decision})).await;
        }
        "mcpServer/elicitation/request" => {
            let _ = peer.respond(id, json!({"action": "decline"})).await;
        }
        _ => {
            let _ = peer
                .respond_error(id, -32601, &format!("Parzi does not handle {method}"))
                .await;
        }
    }
}

struct Turn {
    thread: String,
    turn: String,
    file_paths: HashMap<String, Vec<String>>,
    last_error: Option<ProviderError>,
    interrupting: bool,
}

impl Turn {
    fn ours(&self, params: &Value) -> bool {
        params
            .get("threadId")
            .and_then(Value::as_str)
            .is_none_or(|t| t == self.thread)
    }

    fn notification(
        &mut self,
        method: &str,
        p: &Value,
        events: &EventTx,
    ) -> Result<Option<TurnEnd>, ProviderError> {
        if !self.ours(p) {
            return Ok(None);
        }
        let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        match method {
            "item/agentMessage/delta" => {
                let _ = events.send(ProviderEvent::TextDelta(s("delta")));
            }
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                let _ = events.send(ProviderEvent::ReasoningDelta(s("delta")));
            }
            "item/started" => {
                if let Some(item) = p.get("item") {
                    self.item_started(item, events);
                }
            }
            "item/completed" => {
                if let Some(item) = p.get("item") {
                    item_completed(item, events);
                }
            }
            "thread/tokenUsage/updated" => {
                let u = p.get("tokenUsage").unwrap_or(&Value::Null);
                let n = |path: &str| u.pointer(path).and_then(Value::as_u64).unwrap_or(0);
                let (input, output) = (n("/last/inputTokens"), n("/last/outputTokens"));
                let _ = events.send(ProviderEvent::Usage {
                    input,
                    output,
                    cost_usd: None,
                });
                if let Some(limit) = u.get("modelContextWindow").and_then(Value::as_u64) {
                    let _ = events.send(ProviderEvent::Context {
                        used: input + output,
                        limit,
                    });
                }
            }
            "account/rateLimits/updated" => {
                let w = windows(p.get("rateLimits").unwrap_or(&Value::Null));
                if !w.is_empty() {
                    let _ = events.send(ProviderEvent::Limits(w));
                }
            }
            "model/rerouted" => {
                let _ = events.send(ProviderEvent::Notice(format!(
                    "Codex switched from {} to {}",
                    s("fromModel"),
                    s("toModel")
                )));
            }
            "error" => {
                let err = turn_error(p.get("error").unwrap_or(&Value::Null));
                if p.get("willRetry").and_then(Value::as_bool) == Some(true) {
                    let _ = events.send(ProviderEvent::Notice(format!("Codex is retrying: {err}")));
                } else {
                    self.last_error = Some(err);
                }
            }
            "turn/completed" => {
                let turn = p.get("turn").unwrap_or(&Value::Null);
                let id = turn.get("id").and_then(Value::as_str).unwrap_or("");
                if !self.turn.is_empty() && !id.is_empty() && id != self.turn {
                    return Ok(None);
                }
                return match turn.get("status").and_then(Value::as_str).unwrap_or("") {
                    "interrupted" => Ok(Some(TurnEnd::Interrupted)),
                    "failed" => Err(match turn.get("error").filter(|e| !e.is_null()) {
                        Some(e) => turn_error(e),
                        None => self.last_error.take().unwrap_or_else(|| {
                            ProviderError::new(
                                ErrorClass::Unknown,
                                "Codex ended the turn as failed",
                            )
                        }),
                    }),
                    _ if self.interrupting => Ok(Some(TurnEnd::Interrupted)),
                    _ => Ok(Some(TurnEnd::Completed)),
                };
            }
            _ => {}
        }
        Ok(None)
    }

    fn item_started(&mut self, item: &Value, events: &EventTx) {
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let (name, input) = match item.get("type").and_then(Value::as_str).unwrap_or("") {
            "commandExecution" => (
                "shell".to_string(),
                json!({"command": item.get("command"), "cwd": item.get("cwd")}),
            ),
            "fileChange" => {
                let paths: Vec<String> = item
                    .get("changes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c.get("path").and_then(Value::as_str).map(str::to_string))
                    .collect();
                self.file_paths.insert(id.clone(), paths.clone());
                ("edit".to_string(), json!({"paths": paths}))
            }
            "mcpToolCall" => (
                format!(
                    "{}.{}",
                    item.get("server").and_then(Value::as_str).unwrap_or("mcp"),
                    item.get("tool").and_then(Value::as_str).unwrap_or("tool")
                ),
                item.get("arguments").cloned().unwrap_or(Value::Null),
            ),
            "webSearch" => (
                "web_search".to_string(),
                json!({"query": item.get("query")}),
            ),
            _ => return,
        };
        let _ = events.send(ProviderEvent::ToolStarted { id, name, input });
    }
}

fn item_completed(item: &Value, events: &EventTx) {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let finished = |name: &str, ok: bool, output: String| {
        let _ = events.send(ProviderEvent::ToolFinished {
            id: id.clone(),
            name: name.to_string(),
            ok,
            output,
        });
    };
    match item.get("type").and_then(Value::as_str).unwrap_or("") {
        "agentMessage" => {
            let text = item.get("text").and_then(Value::as_str).unwrap_or("");
            if !text.trim().is_empty() {
                let _ = events.send(ProviderEvent::Message(text.to_string()));
            }
        }
        "reasoning" => {
            let mut parts = vec![];
            for key in ["summary", "content"] {
                for p in item
                    .get(key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(t) = p.as_str().or_else(|| p.get("text").and_then(Value::as_str)) {
                        parts.push(t.to_string());
                    }
                }
            }
            let text = parts.join("\n\n");
            if !text.trim().is_empty() {
                let _ = events.send(ProviderEvent::Reasoning(text));
            }
        }
        "commandExecution" => {
            let exit = item.get("exitCode").and_then(Value::as_i64);
            finished(
                "shell",
                status == "completed" && exit.is_none_or(|c| c == 0),
                item.get("aggregatedOutput")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            );
        }
        "fileChange" => {
            let paths: Vec<&str> = item
                .get("changes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|c| c.get("path").and_then(Value::as_str))
                .collect();
            finished("edit", status == "completed", paths.join("\n"));
        }
        "mcpToolCall" => {
            let ok = status == "completed" && item.get("error").is_none_or(Value::is_null);
            let output = match item.get("error").filter(|e| !e.is_null()) {
                Some(e) => e
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("error")
                    .to_string(),
                None => item
                    .pointer("/result/content")
                    .and_then(Value::as_array)
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|p| p.get("text").and_then(Value::as_str))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default(),
            };
            let name = format!(
                "{}.{}",
                item.get("server").and_then(Value::as_str).unwrap_or("mcp"),
                item.get("tool").and_then(Value::as_str).unwrap_or("tool")
            );
            finished(&name, ok, output);
        }
        "webSearch" => finished("web_search", true, String::new()),
        _ => {}
    }
}

fn turn_error(e: &Value) -> ProviderError {
    let message = e
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Codex reported an error")
        .to_string();
    let info = e.get("codexErrorInfo").unwrap_or(&Value::Null);
    let kind = info
        .as_str()
        .map(str::to_string)
        .or_else(|| info.as_object().and_then(|o| o.keys().next().cloned()))
        .unwrap_or_default();
    let class = match kind.as_str() {
        "unauthorized" => ErrorClass::Auth,
        "usageLimitExceeded" | "rateLimitExceeded" | "sessionBudgetExceeded" => {
            ErrorClass::RateLimit
        }
        "serverOverloaded"
        | "internalServerError"
        | "httpConnectionFailed"
        | "responseStreamConnectionFailed"
        | "responseStreamDisconnected" => ErrorClass::Overloaded,
        "contextWindowExceeded" => ErrorClass::ContextOverflow,
        "badRequest" => ErrorClass::BadRequest,
        _ => ErrorClass::Unknown,
    };
    ProviderError::new(class, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[test]
    fn the_effort_pill_speaks_codex() {
        assert_eq!(effort("ultra"), "xhigh");
        assert_eq!(effort("extra"), "xhigh");
        assert_eq!(
            effort("minimal"),
            "minimal",
            "Codex's own word passes through"
        );
    }

    struct Gate(PermissionDecision, std::sync::Mutex<Vec<PermissionRequest>>);

    #[async_trait::async_trait]
    impl PermissionGate for Gate {
        async fn decide(&self, r: PermissionRequest) -> PermissionDecision {
            self.1.lock().unwrap().push(r);
            self.0.clone()
        }
    }

    fn spec() -> TurnSpec {
        TurnSpec {
            cwd: std::env::temp_dir(),
            model: Some("gpt-5.5".into()),
            effort: Some("high".into()),
            instructions: Some("be brief".into()),
            resume: None,
            prompt: "hi".into(),
            images: vec![],
            tools: None,
        }
    }

    async fn send(w: &mut (impl AsyncWriteExt + Unpin), v: Value) {
        w.write_all(format!("{v}\n").as_bytes()).await.unwrap();
    }

    async fn read(
        lines: &mut tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    ) -> Value {
        serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn a_turn_maps_items_asks_for_edits_and_ends() {
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let (r, mut w) = tokio::io::split(theirs);
            let mut lines = BufReader::new(r).lines();
            let start = read(&mut lines).await;
            assert_eq!(start["method"], "thread/start");
            assert_eq!(start["params"]["approvalPolicy"], "untrusted");
            assert_eq!(start["params"]["developerInstructions"], "be brief");
            send(
                &mut w,
                json!({"jsonrpc": "2.0", "id": start["id"], "result": {"thread": {"id": "th1"}}}),
            )
            .await;
            let turn = read(&mut lines).await;
            assert_eq!(turn["method"], "turn/start");
            assert_eq!(turn["params"]["effort"], "high");
            send(
                &mut w,
                json!({"jsonrpc": "2.0", "id": turn["id"], "result": {"turn": {"id": "tu1"}}}),
            )
            .await;
            send(&mut w, json!({"jsonrpc": "2.0", "method": "item/started", "params": {"threadId": "th1", "item": {"type": "fileChange", "id": "f1", "changes": [{"path": "src/a.rs"}]}}})).await;
            send(&mut w, json!({"jsonrpc": "2.0", "id": 77, "method": "item/fileChange/requestApproval", "params": {"threadId": "th1", "turnId": "tu1", "itemId": "f1"}})).await;
            let approval = read(&mut lines).await;
            assert_eq!(approval["id"], 77);
            assert_eq!(approval["result"]["decision"], "accept");
            send(&mut w, json!({"jsonrpc": "2.0", "method": "item/completed", "params": {"threadId": "th1", "item": {"type": "fileChange", "id": "f1", "status": "completed", "changes": [{"path": "src/a.rs"}]}}})).await;
            send(&mut w, json!({"jsonrpc": "2.0", "method": "item/started", "params": {"threadId": "th1", "item": {"type": "commandExecution", "id": "c1", "command": "cargo test"}}})).await;
            send(&mut w, json!({"jsonrpc": "2.0", "method": "item/completed", "params": {"threadId": "th1", "item": {"type": "commandExecution", "id": "c1", "status": "completed", "exitCode": 1, "aggregatedOutput": "1 failed"}}})).await;
            send(&mut w, json!({"jsonrpc": "2.0", "method": "item/agentMessage/delta", "params": {"threadId": "th1", "delta": "Do"}})).await;
            send(&mut w, json!({"jsonrpc": "2.0", "method": "item/completed", "params": {"threadId": "th1", "item": {"type": "agentMessage", "id": "m1", "text": "Done."}}})).await;
            send(&mut w, json!({"jsonrpc": "2.0", "method": "thread/tokenUsage/updated", "params": {"threadId": "th1", "tokenUsage": {"last": {"inputTokens": 100, "outputTokens": 20}, "modelContextWindow": 272000}}})).await;
            send(&mut w, json!({"jsonrpc": "2.0", "method": "account/rateLimits/updated", "params": {"rateLimits": {"primary": {"usedPercent": 12, "windowDurationMins": 300, "resetsAt": 1_800_000_000u64}, "secondary": {"usedPercent": 40, "windowDurationMins": 10080}}}})).await;
            send(&mut w, json!({"jsonrpc": "2.0", "method": "turn/completed", "params": {"threadId": "th1", "turn": {"id": "tu1", "status": "completed"}}})).await;
        });
        let (r, w) = tokio::io::split(ours);
        let (peer, mut incoming) = Peer::start("Test", r, w);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let gate = Arc::new(Gate(
            PermissionDecision::Allow,
            std::sync::Mutex::new(vec![]),
        ));
        let end = drive(
            &peer,
            &mut incoming,
            &spec(),
            gate.clone(),
            &tx,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(end, TurnEnd::Completed);
        server.await.unwrap();
        let asked = gate.1.lock().unwrap().clone();
        assert_eq!(asked.len(), 1);
        assert_eq!(
            asked[0].paths,
            vec!["src/a.rs".to_string()],
            "lease gate sees the file"
        );
        drop(tx);
        let mut got = vec![];
        while let Some(e) = rx.recv().await {
            got.push(e);
        }
        assert!(got.contains(&ProviderEvent::Session {
            resume: json!({"thread_id": "th1"})
        }));
        assert!(got.contains(&ProviderEvent::Message("Done.".into())));
        assert!(got.iter().any(|e| matches!(e, ProviderEvent::ToolFinished { id, ok: false, output, .. } if id == "c1" && output == "1 failed")));
        assert!(got.contains(&ProviderEvent::Context {
            used: 120,
            limit: 272_000
        }));
        let limits = got
            .iter()
            .find_map(|e| match e {
                ProviderEvent::Limits(w) => Some(w.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            (limits[0].label.as_str(), limits[1].label.as_str()),
            ("Session", "Weekly")
        );
    }

    #[tokio::test]
    async fn a_failed_turn_is_classed_by_codex_error_info() {
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let (r, mut w) = tokio::io::split(theirs);
            let mut lines = BufReader::new(r).lines();
            let start: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            send(
                &mut w,
                json!({"jsonrpc": "2.0", "id": start["id"], "result": {"thread": {"id": "th1"}}}),
            )
            .await;
            let turn: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            send(
                &mut w,
                json!({"jsonrpc": "2.0", "id": turn["id"], "result": {"turn": {"id": "tu1"}}}),
            )
            .await;
            send(&mut w, json!({"jsonrpc": "2.0", "method": "turn/completed", "params": {"threadId": "th1", "turn": {"id": "tu1", "status": "failed", "error": {"message": "You've hit your usage limit.", "codexErrorInfo": "usageLimitExceeded"}}}})).await;
        });
        let (r, w) = tokio::io::split(ours);
        let (peer, mut incoming) = Peer::start("Test", r, w);
        let (tx, _rx) = mpsc::unbounded_channel();
        let gate = Arc::new(Gate(
            PermissionDecision::Allow,
            std::sync::Mutex::new(vec![]),
        ));
        let err = drive(
            &peer,
            &mut incoming,
            &spec(),
            gate,
            &tx,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.class, ErrorClass::RateLimit);
        assert_eq!(err.message, "You've hit your usage limit.");
        server.await.unwrap();
    }

    async fn resume_fails_with(message: &'static str) -> ProviderError {
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let (r, mut w) = tokio::io::split(theirs);
            let mut lines = BufReader::new(r).lines();
            let resume = read(&mut lines).await;
            assert_eq!(resume["method"], "thread/resume");
            send(&mut w, json!({"jsonrpc": "2.0", "id": resume["id"], "error": {"code": -32600, "message": message}})).await;
        });
        let (r, w) = tokio::io::split(ours);
        let (peer, mut incoming) = Peer::start("Test", r, w);
        let (tx, _rx) = mpsc::unbounded_channel();
        let gate = Arc::new(Gate(
            PermissionDecision::Allow,
            std::sync::Mutex::new(vec![]),
        ));
        let mut s = spec();
        s.resume = Some(json!({"thread_id": "th0"}));
        let err = drive(
            &peer,
            &mut incoming,
            &s,
            gate,
            &tx,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        server.await.unwrap();
        err
    }

    #[tokio::test]
    async fn only_a_missing_thread_is_a_lost_conversation() {
        let lost = resume_fails_with("no rollout found for thread id th0").await;
        assert_eq!(lost.class, ErrorClass::SessionLost);
        let other = resume_fails_with("config.toml is malformed").await;
        assert_ne!(other.class, ErrorClass::SessionLost);
        assert!(other.message.contains("malformed"), "{other}");
    }

    #[tokio::test]
    async fn a_stop_while_the_thread_starts_sends_no_turn() {
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let server = tokio::spawn(async move {
            let (r, _w) = tokio::io::split(theirs);
            let mut lines = BufReader::new(r).lines();
            let start = read(&mut lines).await;
            assert_eq!(start["method"], "thread/start");
            stop.cancel();
            let next = tokio::time::timeout(Duration::from_millis(300), lines.next_line()).await;
            assert!(next.is_err(), "nothing follows a stop: {next:?}");
        });
        let (r, w) = tokio::io::split(ours);
        let (peer, mut incoming) = Peer::start("Test", r, w);
        let (tx, _rx) = mpsc::unbounded_channel();
        let gate = Arc::new(Gate(
            PermissionDecision::Allow,
            std::sync::Mutex::new(vec![]),
        ));
        let end = drive(&peer, &mut incoming, &spec(), gate, &tx, &cancel)
            .await
            .unwrap();
        assert_eq!(end, TurnEnd::Interrupted);
        server.await.unwrap();
    }

    #[test]
    fn error_info_with_data_is_still_classed() {
        let e = turn_error(
            &json!({"message": "stream dropped", "codexErrorInfo": {"responseStreamConnectionFailed": {"httpStatusCode": 502}}}),
        );
        assert_eq!(e.class, ErrorClass::Overloaded);
        let e = turn_error(&json!({"message": "401", "codexErrorInfo": "unauthorized"}));
        assert_eq!(e.class, ErrorClass::Auth);
    }
}
