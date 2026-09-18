//! Where each provider stands, as its own program last said. Probed on
//! demand (Settings, start-up, Smart Auto), kept on disk so the picker has
//! an answer before any probe finishes, and fed by runs: a plan window a
//! turn reports lands here without a probe.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use parzi_core::config::ParziConfig;
use parzi_providers::{Provider, ProviderStatus, State, UsageWindow};

/// How the runtime gets a provider: the real roster, or a test's fakes.
pub type ProviderSource =
    Arc<dyn Fn(&str, &ParziConfig) -> Option<Arc<dyn Provider>> + Send + Sync>;

pub fn roster_source() -> ProviderSource {
    Arc::new(|id: &str, cfg: &ParziConfig| parzi_providers::provider(id, cfg))
}

/// A status older than this is probed again before Smart Auto trusts it.
const FRESH_SECS: u64 = 30 * 60;

pub struct StatusBoard {
    map: RwLock<HashMap<String, ProviderStatus>>,
    persist: bool,
}

fn file() -> Option<std::path::PathBuf> {
    parzi_core::paths::parzi_dir()
        .ok()
        .map(|d| d.join("providers.json"))
}

impl StatusBoard {
    /// The board as it was last saved; empty on first start.
    pub fn load() -> Self {
        let map = file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Vec<ProviderStatus>>(&t).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|s| (s.provider.clone(), s))
            .collect();
        Self {
            map: RwLock::new(map),
            persist: true,
        }
    }

    /// An empty board that never touches the disk (tests, one-shot tools).
    pub fn in_memory() -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
            persist: false,
        }
    }

    pub fn get(&self, id: &str) -> Option<ProviderStatus> {
        self.map.read().ok()?.get(id).cloned()
    }

    /// Every roster provider in picker order; one never checked says so.
    pub fn all(&self) -> Vec<ProviderStatus> {
        parzi_providers::PROVIDERS
            .iter()
            .map(|id| {
                self.get(id).unwrap_or_else(|| {
                    let mut s = ProviderStatus::new(id, State::Unchecked, "Not checked yet.");
                    s.checked_at = 0;
                    s
                })
            })
            .collect()
    }

    fn put(&self, s: ProviderStatus) {
        if let Ok(mut m) = self.map.write() {
            m.insert(s.provider.clone(), s);
        }
        self.save();
    }

    fn save(&self) {
        if !self.persist {
            return;
        }
        let Some(path) = file() else { return };
        let list: Vec<ProviderStatus> = match self.map.read() {
            Ok(m) => m.values().cloned().collect(),
            Err(_) => return,
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(&list) {
            if let Err(e) = parzi_core::atomic_write(&path, &bytes) {
                tracing::warn!("provider status not saved: {e}");
            }
        }
    }

    /// Plan windows a run just reported, merged by label.
    pub fn update_usage(&self, id: &str, windows: &[UsageWindow]) {
        let Ok(mut m) = self.map.write() else { return };
        let Some(s) = m.get_mut(id) else { return };
        for w in windows {
            match s.usage.iter_mut().find(|u| u.label == w.label) {
                Some(u) => *u = w.clone(),
                None => s.usage.push(w.clone()),
            }
        }
        drop(m);
        self.save();
    }

    /// Probe `ids` (all when empty) side by side. A provider switched off in
    /// Settings is reported as such without starting anything.
    pub async fn refresh(
        &self,
        cfg: &ParziConfig,
        ids: &[String],
        source: &ProviderSource,
    ) -> Vec<ProviderStatus> {
        let ids: Vec<String> = if ids.is_empty() {
            parzi_providers::PROVIDERS
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        } else {
            ids.to_vec()
        };
        let mut set = tokio::task::JoinSet::new();
        for id in ids {
            let Some(canon) = parzi_providers::canonical_id(&id) else {
                continue;
            };
            if !cfg.provider(canon).enabled {
                self.put(ProviderStatus::new(
                    canon,
                    State::Disabled,
                    "Switched off in Settings.",
                ));
                continue;
            }
            let Some(provider) = source(canon, cfg) else {
                continue;
            };
            set.spawn(async move {
                // One slow vendor must not hold the rest.
                tokio::time::timeout(std::time::Duration::from_secs(90), provider.status())
                    .await
                    .unwrap_or_else(|_| {
                        ProviderStatus::new(canon, State::Error, "The status check took over 90 s.")
                    })
            });
        }
        while let Some(done) = set.join_next().await {
            if let Ok(mut s) = done {
                // Keep plan windows a run saw when the probe itself has none.
                if s.usage.is_empty() {
                    if let Some(old) = self.get(&s.provider) {
                        s.usage = old.usage;
                    }
                }
                self.put(s);
            }
        }
        self.all()
    }

    /// Smart Auto: the first provider in the configured order that is on,
    /// installed and signed in (or cannot be checked short of a session),
    /// and has no plan window used up. Stale answers are probed first.
    pub async fn pick(&self, cfg: &ParziConfig, source: &ProviderSource) -> Option<String> {
        let now = parzi_providers::now_secs();
        for id in &cfg.routing.order {
            let Some(id) = parzi_providers::canonical_id(id) else {
                continue;
            };
            if !cfg.provider(id).enabled {
                continue;
            }
            let stale = self
                .get(id)
                .is_none_or(|s| now.saturating_sub(s.checked_at) > FRESH_SECS);
            if stale {
                self.refresh(cfg, &[id.to_string()], source).await;
            }
            let Some(s) = self.get(id) else { continue };
            let usable = matches!(s.state, State::Ready | State::Unchecked);
            let spent = s.usage.iter().any(|w| w.used_percent >= 100.0);
            if usable && !spent {
                return Some(id.to_string());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parzi_providers::{EventTx, PermissionGate, ProviderError, TurnEnd, TurnSpec};
    use tokio_util::sync::CancellationToken;

    struct Fixed(&'static str, State, f64);

    #[async_trait::async_trait]
    impl Provider for Fixed {
        fn id(&self) -> &'static str {
            self.0
        }
        async fn status(&self) -> ProviderStatus {
            let mut s = ProviderStatus::new(self.0, self.1, "");
            s.usage = vec![UsageWindow {
                label: "Session".into(),
                used_percent: self.2,
                resets_at: None,
            }];
            s
        }
        async fn run_turn(
            &self,
            _spec: TurnSpec,
            _gate: Arc<dyn PermissionGate>,
            _events: EventTx,
            _cancel: CancellationToken,
        ) -> Result<TurnEnd, ProviderError> {
            Ok(TurnEnd::Completed)
        }
    }

    fn source() -> ProviderSource {
        Arc::new(
            |id: &str, _cfg: &ParziConfig| -> Option<Arc<dyn Provider>> {
                Some(match id {
                    "claude" => Arc::new(Fixed("claude", State::Ready, 100.0)),
                    "codex" => Arc::new(Fixed("codex", State::SignedOut, 0.0)),
                    "opencode" => Arc::new(Fixed("opencode", State::Ready, 12.0)),
                    other => Arc::new(Fixed(
                        parzi_providers::canonical_id(other)?,
                        State::NotInstalled,
                        0.0,
                    )),
                })
            },
        )
    }

    #[tokio::test]
    async fn smart_auto_skips_used_up_and_signed_out_providers() {
        let board = StatusBoard::in_memory();
        let cfg = ParziConfig::default();
        // claude's session window is at 100%, codex is signed out.
        assert_eq!(
            board.pick(&cfg, &source()).await.as_deref(),
            Some("opencode")
        );
        let mut off = cfg.clone();
        off.providers.insert(
            "opencode".into(),
            parzi_core::config::ProviderEntry {
                enabled: false,
                ..Default::default()
            },
        );
        assert_eq!(
            board.pick(&off, &source()).await,
            None,
            "nothing else is ready"
        );
    }

    #[tokio::test]
    async fn a_run_s_windows_merge_into_the_probe() {
        let board = StatusBoard::in_memory();
        board
            .refresh(&ParziConfig::default(), &["opencode".into()], &source())
            .await;
        board.update_usage(
            "opencode",
            &[UsageWindow {
                label: "Session".into(),
                used_percent: 55.0,
                resets_at: Some(1),
            }],
        );
        board.update_usage(
            "opencode",
            &[UsageWindow {
                label: "Weekly".into(),
                used_percent: 5.0,
                resets_at: None,
            }],
        );
        let s = board.get("opencode").unwrap();
        assert_eq!(s.usage.len(), 2);
        assert!((s.usage[0].used_percent - 55.0).abs() < 1e-9);
    }
}
