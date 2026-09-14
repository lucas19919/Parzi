//! Minimal native MCP client (STDIO, newline-delimited JSON-RPC).
//! No heavy framework: spawn -> initialize -> tools/list (cached) -> tools/call.
//! Servers start lazily on first use and die after idle timeout.
//! (Deviates from PLAN.md's rmcp pin on purpose: fewer deps, same protocol,
//! deterministic behavior. See PROGRESS.md.)

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parzi_core::config::McpServerCfg;
use parzi_core::error::{ParziError, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Clone)]
pub struct McpTool {
    pub server: String,
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

impl McpTool {
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.server, self.name)
    }

    pub fn to_provider_def(&self) -> parzi_providers::ToolDef {
        parzi_providers::ToolDef {
            name: self.qualified(),
            description: self.description.clone(),
            schema: self.schema.clone(),
        }
    }
}

struct LiveServer {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    tools: Option<Vec<McpTool>>,
    last_used: Instant,
}

pub struct McpManager {
    configs: std::sync::RwLock<HashMap<String, McpServerCfg>>,
    live: tokio::sync::Mutex<HashMap<String, LiveServer>>,
    idle_kill: Duration,
}

impl McpManager {
    pub fn new(configs: HashMap<String, McpServerCfg>, idle_kill_secs: u64) -> Self {
        Self {
            configs: std::sync::RwLock::new(configs),
            live: tokio::sync::Mutex::new(HashMap::new()),
            idle_kill: Duration::from_secs(idle_kill_secs.max(10)),
        }
    }

    pub fn server_names(&self) -> Vec<String> {
        self.configs.read().map(|c| c.keys().cloned().collect()).unwrap_or_default()
    }

    fn config_for(&self, name: &str) -> Option<McpServerCfg> {
        self.configs.read().ok()?.get(name).cloned()
    }

    /// Hot-swap server configs (Settings save path). Drops live handles for
    /// servers that vanished or were disabled so the next use re-spawns.
    pub async fn set_configs(&self, configs: HashMap<String, McpServerCfg>) {
        let dead: Vec<String> = {
            let live = self.live.lock().await;
            live.keys().filter(|k| {
                match configs.get(*k) {
                    None => true,
                    Some(c) => !c.enabled,
                }
            }).cloned().collect()
        };
        for k in dead {
            self.stop(&k).await;
        }
        if let Ok(mut w) = self.configs.write() {
            *w = configs;
        }
    }

    /// Exposure check: server allow/deny lists. Empty allow = all except denied.
    pub fn is_tool_exposed(&self, server: &str, tool: &str) -> bool {
        match self.config_for(server) {
            Some(c) => c.is_tool_exposed(tool),
            None => false,
        }
    }

    /// Per-tool approval override for `server.tool` (`auto`|`ask`|`deny`).
    pub fn tool_mode(&self, server: &str, tool: &str) -> Option<String> {
        self.config_for(server)?.tool_mode(tool)
    }

    /// Exposed tools for one server (allow/deny applied). Used by the agent
    /// to advertise `server.tool` defs and by the UI tool browser.
    pub async fn exposed_tools(&self, name: &str) -> Result<Vec<McpTool>> {
        let all = self.list_tools(name).await?;
        Ok(all.into_iter().filter(|t| self.is_tool_exposed(name, &t.name)).collect())
    }

    async fn request(
        sv: &mut LiveServer,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value> {
        sv.next_id += 1;
        let id = sv.next_id;
        let line = serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        });
        let mut wire = serde_json::to_string(&line)?;
        wire.push('\n');
        sv.stdin.write_all(wire.as_bytes()).await.map_err(|e| {
            ParziError::Tool("mcp".into(), format!("write failed: {e}"))
        })?;
        sv.stdin.flush().await.map_err(|e| {
            ParziError::Tool("mcp".into(), format!("flush failed: {e}"))
        })?;
        let mut out = String::new();
        tokio::time::timeout(timeout, sv.stdout.read_line(&mut out))
            .await
            .map_err(|_| ParziError::Tool("mcp".into(), format!("{method} timed out")))?
            .map_err(|e| ParziError::Tool("mcp".into(), format!("read failed: {e}")))?;
        let v: serde_json::Value = serde_json::from_str(out.trim()).map_err(|e| {
            ParziError::Tool("mcp".into(), format!("bad jsonrpc: {e}"))
        })?;
        if let Some(err) = v.get("error") {
            return Err(ParziError::Tool("mcp".into(), format!("server error: {err}")));
        }
        Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }

    async fn ensure_live(
        &self,
        live: &mut std::collections::hash_map::HashMap<String, LiveServer>,
        name: &str,
    ) -> Result<()> {
        if live.contains_key(name) {
            return Ok(());
        }
        let cfg = self.config_for(name).ok_or_else(|| {
            ParziError::Tool("mcp".into(), format!("unknown server `{name}`"))
        })?;
        if !cfg.enabled {
            return Err(ParziError::Tool("mcp".into(), format!("server `{name}` disabled")));
        }
        let mut child: Child = Command::new(&cfg.command)
            .args(&cfg.args)
            .envs(&cfg.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| ParziError::Tool("mcp".into(), format!("spawn `{name}`: {e}")))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            ParziError::Tool("mcp".into(), "no stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ParziError::Tool("mcp".into(), "no stdout".into())
        })?;
        let mut sv = LiveServer {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            tools: None,
            last_used: Instant::now(),
        };
        let timeout = Duration::from_millis(cfg.timeout_ms.max(1_000));
        let init = serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "parzi", "version": env!("CARGO_PKG_VERSION")},
        });
        Self::request(&mut sv, "initialize", init, timeout).await?;
        // Initialized notification (no id, no response expected).
        let note = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let mut wire = serde_json::to_string(&note)?;
        wire.push('\n');
        sv.stdin.write_all(wire.as_bytes()).await.map_err(|e| {
            ParziError::Tool("mcp".into(), format!("write failed: {e}"))
        })?;
        live.insert(name.to_string(), sv);
        Ok(())
    }

    /// Kill servers idle past the limit. Called on every op — no bg task needed.
    async fn reap_idle(&self, live: &mut HashMap<String, LiveServer>) {
        let dead: Vec<String> = live
            .iter()
            .filter(|(_, s)| s.last_used.elapsed() > self.idle_kill)
            .map(|(k, _)| k.clone())
            .collect();
        for k in dead {
            if let Some(mut s) = live.remove(&k) {
                let _ = s.child.kill().await;
            }
        }
    }

    fn timeout_for(&self, name: &str) -> Duration {
        Duration::from_millis(
            self.config_for(name).map(|c| c.timeout_ms).unwrap_or(30_000).max(1_000),
        )
    }

    /// List tools (cached per server). Exposure filtering happens in
    /// `exposed_tools` / the executor, not here.
    pub async fn list_tools(&self, name: &str) -> Result<Vec<McpTool>> {
        let mut live = self.live.lock().await;
        self.reap_idle(&mut live).await;
        self.ensure_live(&mut live, name).await?;
        let sv = live.get_mut(name).expect("live after ensure");
        sv.last_used = Instant::now();
        if let Some(t) = &sv.tools {
            return Ok(t.clone());
        }
        let timeout = self.timeout_for(name);
        let res = Self::request(sv, "tools/list", serde_json::json!({}), timeout).await?;
        let mut out = vec![];
        if let Some(arr) = res.get("tools").and_then(|t| t.as_array()) {
            for t in arr {
                out.push(McpTool {
                    server: name.to_string(),
                    name: t.get("name").and_then(|n| n.as_str()).unwrap_or("?").into(),
                    description: t.get("description").and_then(|d| d.as_str()).unwrap_or("").into(),
                    schema: t
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or(serde_json::json!({"type": "object"})),
                });
            }
        }
        sv.tools = Some(out.clone());
        Ok(out)
    }

    /// Call a tool. Returns (ok, text).
    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> (bool, String) {
        let res = self.call_inner(server, tool, args).await;
        match res {
            Ok(t) => (true, t),
            Err(e) => (false, e.to_string()),
        }
    }

    async fn call_inner(
        &self,
        server: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<String> {
        let mut live = self.live.lock().await;
        self.reap_idle(&mut live).await;
        self.ensure_live(&mut live, server).await?;
        let sv = live.get_mut(server).expect("live after ensure");
        sv.last_used = Instant::now();
        let timeout = self.timeout_for(server);
        let params = serde_json::json!({"name": tool, "arguments": args});
        let res = Self::request(sv, "tools/call", params, timeout).await?;
        // content: [{type:"text",text}, ...] — join text, flag isError.
        let is_err = res.get("isError").and_then(|b| b.as_bool()).unwrap_or(false);
        let mut text = String::new();
        if let Some(arr) = res.get("content").and_then(|c| c.as_array()) {
            for c in arr {
                if let Some(t) = c.get("text").and_then(|t| t.as_str()) {
                    text.push_str(t);
                    text.push('\n');
                }
            }
        }
        if text.is_empty() {
            text = res.to_string();
        }
        if is_err {
            return Err(ParziError::Tool(server.into(), text));
        }
        Ok(text)
    }

    /// Stop a server now (used by kill/health flows).
    pub async fn stop(&self, name: &str) {
        let mut live = self.live.lock().await;
        if let Some(mut s) = live.remove(name) {
            let _ = s.child.kill().await;
        }
    }
}
