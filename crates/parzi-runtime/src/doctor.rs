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
    cfg: ParziConfig,
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
        out.push(self.check_webview());
        out
    }

    pub async fn run_quick(&self) -> Vec<Check> {
        self.run().await
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
                let version = s
                    .version
                    .as_deref()
                    .map(|v| format!(" v{v}"))
                    .unwrap_or_default();
                match s.state {
                    State::Ready => Check::ok(
                        &name,
                        format!(
                            "ready{version} · {}",
                            s.account.unwrap_or_else(|| "signed in".into())
                        ),
                    ),
                    State::Unchecked => {
                        Check::ok(&name, format!("installed{version} · {}", s.hint))
                    }
                    State::NotInstalled | State::Disabled => Check::ok(&name, s.hint),
                    State::SignedOut | State::Error => Check::fail(&name, s.hint),
                }
            })
            .collect()
    }

    fn check_routing(&self) -> Vec<Check> {
        vec![Check::ok(
            "routing:smart-auto",
            format!(
                "new threads start on the first ready provider of: {}",
                self.cfg.routing.order.join(" → ")
            ),
        )]
    }

    #[cfg(windows)]
    fn check_webview(&self) -> Check {
        let key = reg_key_version();
        match key {
            Some(v) => Check::ok("webview2", format!("Edge/WebView2 {v}")),
            None => Check::fail("webview2", "version not detected"),
        }
    }

    #[cfg(target_os = "macos")]
    fn check_webview(&self) -> Check {
        Check::ok("webview", "system WKWebView")
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn check_webview(&self) -> Check {
        Check::ok("webview", "check runs on Windows and macOS targets")
    }
}

#[cfg(windows)]
fn reg_key_version() -> Option<String> {
    use std::os::windows::process::CommandExt as _;
    use std::process::Command;
    let out = Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
            "/v",
            "pv",
        ])
        .creation_flags(parzi_providers::process::CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace().last().map(str::to_string)
}
