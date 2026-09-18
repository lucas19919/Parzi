//! `parzi doctor`: one function, used by CLI and GUI About alike.
//! Providers are asked where they stand by their own programs; no secret
//! is ever read or printed.

use parzi_core::config::ParziConfig;
use parzi_core::paths;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

impl Check {
    fn ok(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok: true,
            detail: detail.into(),
        }
    }
    fn fail(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok: false,
            detail: detail.into(),
        }
    }
}

pub struct Doctor {
    pub cfg: ParziConfig,
}

impl Doctor {
    pub fn new(cfg: ParziConfig) -> Self {
        Self { cfg }
    }

    pub async fn run(&self) -> Vec<Check> {
        let mut out = vec![];
        out.push(self.check_dirs());
        out.push(self.check_config());
        out.push(self.check_theme());
        out.extend(self.check_providers().await);
        out.extend(self.check_routing());
        out.extend(self.check_mcp().await);
        out.push(self.check_webview());
        out
    }

    /// Fast subset: everything except the MCP server probes (each probe can
    /// block up to 15s). Settings > System renders this first, then streams
    /// the MCP section in via `run_mcp_only`.
    pub async fn run_quick(&self) -> Vec<Check> {
        let mut out = vec![];
        out.push(self.check_dirs());
        out.push(self.check_config());
        out.push(self.check_theme());
        out.extend(self.check_providers().await);
        out.extend(self.check_routing());
        out.push(self.check_webview());
        out
    }

    /// MCP probes only (slow path: 15s timeout per configured server).
    pub async fn run_mcp_only(&self) -> Vec<Check> {
        self.check_mcp().await
    }

    fn check_dirs(&self) -> Check {
        match paths::ensure_dirs() {
            Ok(p) => Check::ok("dirs", format!("ok at {}", p.display())),
            Err(e) => Check::fail("dirs", e.to_string()),
        }
    }

    fn check_config(&self) -> Check {
        match ParziConfig::load() {
            Ok(c) => Check::ok("config", format!("version {} ok", c.version)),
            Err(e) => Check::fail("config", e.to_string()),
        }
    }

    fn check_theme(&self) -> Check {
        match parzi_core::theme::Theme::load() {
            Ok(_) => Check::ok("theme", "theme.toml parses"),
            Err(e) => Check::fail("theme", e.to_string()),
        }
    }

    /// Each provider, as its own program reports it. A provider switched off
    /// or not installed is not a failure: it is simply not in use.
    async fn check_providers(&self) -> Vec<Check> {
        use parzi_providers::State;
        let board = crate::status::StatusBoard::in_memory();
        let statuses = board
            .refresh(&self.cfg, &[], &crate::status::roster_source())
            .await;
        statuses
            .into_iter()
            .map(|s| {
                let name = format!("provider:{}", s.provider);
                let version = s.version.as_deref().map(|v| format!(" v{v}")).unwrap_or_default();
                match s.state {
                    State::Ready => Check::ok(
                        &name,
                        format!("ready{version} · {}", s.account.unwrap_or_else(|| "signed in".into())),
                    ),
                    State::Unchecked => Check::ok(&name, format!("installed{version} · {}", s.hint)),
                    State::NotInstalled | State::Disabled => Check::ok(&name, s.hint),
                    State::SignedOut | State::Error => Check::fail(&name, s.hint),
                }
            })
            .collect()
    }

    /// Smart Auto's order: where a new thread starts.
    fn check_routing(&self) -> Vec<Check> {
        vec![Check::ok(
            "routing:smart-auto",
            format!(
                "new threads start on the first ready provider of: {}",
                self.cfg.routing.order.join(" → ")
            ),
        )]
    }

    async fn check_mcp(&self) -> Vec<Check> {
        let mut out = vec![];
        if self.cfg.mcp.servers.is_empty() {
            return vec![Check::ok("mcp", "no servers configured")];
        }
        let mgr = crate::mcp::McpManager::new(
            self.cfg.mcp.servers.clone(),
            self.cfg.orchestrator.mcp_idle_kill_secs,
        );
        for name in mgr.server_names() {
            match tokio::time::timeout(std::time::Duration::from_secs(15), mgr.list_tools(&name))
                .await
            {
                Ok(Ok(tools)) => {
                    mgr.stop(&name).await;
                    out.push(Check::ok(
                        &format!("mcp:{name}"),
                        format!("{} tools", tools.len()),
                    ));
                }
                Ok(Err(e)) => out.push(Check::fail(&format!("mcp:{name}"), e.to_string())),
                Err(_) => out.push(Check::fail(&format!("mcp:{name}"), "probe timed out")),
            }
        }
        out
    }

    #[cfg(windows)]
    fn check_webview(&self) -> Check {
        // Best-effort: read Edge/WebView2 version from the registry.
        let key = reg_key_version();
        match key {
            Some(v) => Check::ok("webview2", format!("Edge/WebView2 {v}")),
            None => Check::fail("webview2", "version not detected"),
        }
    }

    #[cfg(target_os = "macos")]
    fn check_webview(&self) -> Check {
        // macOS renders via the system WKWebView — always present, no
        // Evergreen-style runtime to probe like WebView2 on Windows.
        Check::ok("webview", "system WKWebView")
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn check_webview(&self) -> Check {
        Check::ok("webview", "check runs on Windows and macOS targets")
    }
}

#[cfg(windows)]
fn reg_key_version() -> Option<String> {
    use std::process::Command;
    let out = Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
            "/v",
            "pv",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace().last().map(str::to_string)
}
