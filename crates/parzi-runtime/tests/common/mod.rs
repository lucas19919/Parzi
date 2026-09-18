//! Shared test harness: a scripted agent that behaves like a vendor's.
//! It streams words, asks Parzi's gate before acting, and calls Parzi's
//! tools over the real MCP endpoint — the same paths Claude Code, Codex
//! and the ACP agents take.

#![allow(dead_code)]

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use parzi_core::config::ParziConfig;
use parzi_core::store::{Event, SessionMeta, SessionStatus, SessionStore};
use parzi_providers::{
    EventTx, PermissionDecision, PermissionGate, PermissionRequest, Provider, ProviderError,
    ProviderEvent, ProviderStatus, State, TurnEnd, TurnSpec,
};
use parzi_runtime::status::{ProviderSource, StatusBoard};
use parzi_runtime::tools::to_mcp;
use parzi_runtime::Orchestrator;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

/// A hermetic home for this test binary: no test session ever lands in the
/// real `~/.parzi`. Every test calls it first; the Once shares one home.
pub fn home(tag: &str) -> PathBuf {
    static INIT: std::sync::Once = std::sync::Once::new();
    static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
    INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("parzi-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
        *DIR.lock().unwrap() = Some(dir);
    });
    DIR.lock().unwrap().clone().unwrap()
}

pub type Script = Arc<
    dyn Fn(Agent) -> Pin<Box<dyn Future<Output = Result<TurnEnd, ProviderError>> + Send>>
        + Send
        + Sync,
>;

/// Wrap an async closure as a script.
pub fn script<F, Fut>(f: F) -> Script
where
    F: Fn(Agent) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<TurnEnd, ProviderError>> + Send + 'static,
{
    Arc::new(move |a| Box::pin(f(a)))
}

/// A provider whose every turn runs `script`. It records each turn's spec.
pub struct Fake {
    pub id: &'static str,
    pub script: Script,
    pub turns: Arc<Mutex<Vec<TurnSpec>>>,
    /// Plays an agent that asks before every change (the usual case).
    pub gated: bool,
}

impl Fake {
    pub fn new(id: &'static str, script: Script) -> Arc<Self> {
        Arc::new(Self {
            id,
            script,
            turns: Arc::default(),
            gated: true,
        })
    }

    /// An agent that applies some changes without asking Parzi first.
    pub fn ungated(id: &'static str, script: Script) -> Arc<Self> {
        Arc::new(Self {
            id,
            script,
            turns: Arc::default(),
            gated: false,
        })
    }

    pub fn seen(&self) -> Vec<TurnSpec> {
        self.turns.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Provider for Fake {
    fn id(&self) -> &'static str {
        self.id
    }
    fn gated(&self) -> bool {
        self.gated
    }
    async fn status(&self) -> ProviderStatus {
        ProviderStatus::new(self.id, State::Ready, "")
    }
    async fn run_turn(
        &self,
        spec: TurnSpec,
        gate: Arc<dyn PermissionGate>,
        events: EventTx,
        cancel: CancellationToken,
    ) -> Result<TurnEnd, ProviderError> {
        self.turns.lock().unwrap().push(spec.clone());
        (self.script)(Agent {
            spec,
            gate,
            events,
            cancel,
        })
        .await
    }
}

/// What a scripted turn can do.
pub struct Agent {
    pub spec: TurnSpec,
    pub gate: Arc<dyn PermissionGate>,
    pub events: EventTx,
    pub cancel: CancellationToken,
}

impl Agent {
    pub fn session(&self, id: &str) {
        let _ = self.events.send(ProviderEvent::Session {
            resume: json!({"session_id": id}),
        });
    }

    /// Stream `text` and finish it as a message.
    pub fn say(&self, text: &str) {
        let _ = self.events.send(ProviderEvent::TextDelta(text.to_string()));
        let _ = self.events.send(ProviderEvent::Message(text.to_string()));
    }

    pub fn usage(&self, input: u64, output: u64, cost: Option<f64>) {
        let _ = self.events.send(ProviderEvent::Usage {
            input,
            output,
            cost_usd: cost,
        });
    }

    /// Ask Parzi's gate, as a vendor does before one of its own actions.
    pub async fn ask(&self, tool: &str, input: Value, paths: &[&str]) -> PermissionDecision {
        self.gate
            .decide(PermissionRequest {
                id: format!("req-{}", uuid::Uuid::new_v4()),
                tool: tool.to_string(),
                title: tool.to_string(),
                input,
                paths: paths.iter().map(|p| (*p).to_string()).collect(),
            })
            .await
    }

    /// One of the agent's own tools, start to finish, reported to Parzi.
    /// `permit` is the permission request id when the gate was asked first.
    pub async fn own_tool<F, Fut>(
        &self,
        id: &str,
        name: &str,
        input: Value,
        act: F,
    ) -> (bool, String)
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = (bool, String)>,
    {
        let _ = self.events.send(ProviderEvent::ToolStarted {
            id: id.to_string(),
            name: name.to_string(),
            input,
        });
        let (ok, output) = act().await;
        let _ = self.events.send(ProviderEvent::ToolFinished {
            id: id.to_string(),
            name: name.to_string(),
            ok,
            output: output.clone(),
        });
        (ok, output)
    }

    /// Call one of Parzi's tools over MCP, reported like a vendor reports it.
    pub async fn parzi(&self, tool: &str, args: Value) -> (bool, String) {
        let id = format!("mcp-{}", uuid::Uuid::new_v4());
        let name = format!("mcp__parzi__{}", to_mcp(tool));
        let _ = self.events.send(ProviderEvent::ToolStarted {
            id: id.clone(),
            name: name.clone(),
            input: args.clone(),
        });
        let (ok, output) = mcp_call(&self.spec, tool, args).await;
        let _ = self.events.send(ProviderEvent::ToolFinished {
            id,
            name,
            ok,
            output: output.clone(),
        });
        (ok, output)
    }

    /// The tools Parzi offers this turn, by their MCP names.
    pub async fn tool_names(&self) -> Vec<String> {
        let v = mcp(&self.spec, "tools/list", json!({})).await;
        v["result"]["tools"]
            .as_array()
            .map(|t| {
                t.iter()
                    .filter_map(|x| x["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }
}

pub async fn mcp(spec: &TurnSpec, method: &str, params: Value) -> Value {
    let url = &spec
        .tools
        .as_ref()
        .expect("this turn offers Parzi's tools")
        .url;
    reqwest::Client::new()
        .post(url)
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))
        .send()
        .await
        .expect("MCP endpoint reachable")
        .json()
        .await
        .expect("MCP answer is JSON")
}

pub async fn mcp_call(spec: &TurnSpec, tool: &str, args: Value) -> (bool, String) {
    let v = mcp(
        spec,
        "tools/call",
        json!({"name": to_mcp(tool), "arguments": args}),
    )
    .await;
    let r = &v["result"];
    let text = r["content"][0]["text"].as_str().unwrap_or("").to_string();
    (!r["isError"].as_bool().unwrap_or(true), text)
}

/// A source with these fakes under roster ids; anything else is unknown.
pub fn source(fakes: &[Arc<Fake>]) -> ProviderSource {
    let fakes: Vec<Arc<Fake>> = fakes.to_vec();
    Arc::new(
        move |id: &str, _cfg: &ParziConfig| -> Option<Arc<dyn Provider>> {
            fakes
                .iter()
                .find(|f| f.id == id)
                .map(|f| f.clone() as Arc<dyn Provider>)
        },
    )
}

pub fn orch_with(cfg: ParziConfig, fakes: &[Arc<Fake>]) -> (Arc<Orchestrator>, SessionStore) {
    let store = SessionStore::open().unwrap();
    let orch = Orchestrator::new(cfg, store.clone())
        .with_source(source(fakes))
        .with_status(Arc::new(StatusBoard::in_memory()));
    (Arc::new(orch), store)
}

pub fn orch(fakes: &[Arc<Fake>]) -> (Arc<Orchestrator>, SessionStore) {
    let mut cfg = ParziConfig::default();
    cfg.lanes.default_mode = "auto".into();
    orch_with(cfg, fakes)
}

/// Wait until the session is no longer active or queued.
pub async fn settle(store: &SessionStore, id: &str) -> SessionMeta {
    for _ in 0..400 {
        let m = store.get(id).unwrap();
        if !matches!(m.status, SessionStatus::Active | SessionStatus::Queued) {
            return m;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("session {id} did not settle in 20s");
}

pub async fn wait_status(store: &SessionStore, id: &str, want: SessionStatus) -> bool {
    for _ in 0..400 {
        if store.get(id).is_ok_and(|m| m.status == want) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    false
}

pub fn events(store: &SessionStore, id: &str) -> Vec<Event> {
    store.events(id).unwrap()
}

pub fn last_reply(store: &SessionStore, id: &str) -> String {
    events(store, id)
        .into_iter()
        .rev()
        .find_map(|e| match e {
            Event::Assistant { text, .. } => Some(text),
            _ => None,
        })
        .unwrap_or_default()
}
