use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use parzi_core::config::ParziConfig;
use parzi_providers::{Provider, ProviderStatus, State, UsageWindow};

pub type ProviderSource =
    Arc<dyn Fn(&str, &ParziConfig) -> Option<Arc<dyn Provider>> + Send + Sync>;

pub(crate) fn roster_source() -> ProviderSource {
    Arc::new(|id: &str, cfg: &ParziConfig| parzi_providers::provider(id, cfg))
}

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
    pub(crate) fn load() -> Self {
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

    pub fn in_memory() -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
            persist: false,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<ProviderStatus> {
        self.map.read().ok()?.get(id).cloned()
    }

    pub(crate) fn all(&self) -> Vec<ProviderStatus> {
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

    pub(crate) fn update_usage(&self, id: &str, windows: &[UsageWindow]) {
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

    pub(crate) async fn refresh(
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
            let Some(provider) = source(canon, cfg) else {
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
            set.spawn(async move {
                tokio::time::timeout(std::time::Duration::from_secs(90), provider.status())
                    .await
                    .unwrap_or_else(|_| {
                        ProviderStatus::new(canon, State::Error, "The status check took over 90 s.")
                    })
            });
        }
        while let Some(done) = set.join_next().await {
            if let Ok(mut s) = done {
                if s.usage.is_empty() {
                    if let Some(old) = self.get(&s.provider) {
                        let now = parzi_providers::now_secs();
                        s.usage = old.usage.into_iter().filter(|w| w.current(now)).collect();
                    }
                }
                self.put(s);
            }
        }
        self.all()
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
        fn gated(&self) -> bool {
            true
        }
        async fn status(&self) -> ProviderStatus {
            let mut s = ProviderStatus::new(self.0, self.1, "");
            if self.2 >= 0.0 {
                s.usage = vec![UsageWindow {
                    label: "Session".into(),
                    used_percent: self.2,
                    resets_at: None,
                }];
            }
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

    #[tokio::test]
    async fn a_used_up_window_stops_counting_once_it_has_reset() {
        let board = StatusBoard::in_memory();
        let quiet: ProviderSource = Arc::new(
            |id: &str, _cfg: &ParziConfig| -> Option<Arc<dyn Provider>> {
                Some(Arc::new(Fixed(
                    parzi_providers::canonical_id(id)?,
                    State::Ready,
                    -1.0,
                )))
            },
        );
        let mut cfg = ParziConfig::default();
        cfg.routing.order = vec!["claude".into()];
        board.refresh(&cfg, &["claude".into()], &quiet).await;
        let now = parzi_providers::now_secs();
        let spent = |resets_at| UsageWindow {
            label: "Session".into(),
            used_percent: 100.0,
            resets_at,
        };
        board.update_usage("claude", &[spent(Some(now + 3600))]);
        assert!(
            board
                .get("claude")
                .unwrap()
                .usage
                .iter()
                .any(|w| w.spent(now)),
            "used up until it resets"
        );
        board.update_usage("claude", &[spent(Some(now - 1))]);
        assert!(
            !board
                .get("claude")
                .unwrap()
                .usage
                .iter()
                .any(|w| w.spent(now)),
            "past its reset, the window is open again"
        );
        board.update_usage("claude", &[spent(None)]);
        assert!(board
            .get("claude")
            .unwrap()
            .usage
            .iter()
            .any(|w| w.spent(now)));
        board.refresh(&cfg, &["claude".into()], &quiet).await;
        assert!(
            board.get("claude").unwrap().usage.is_empty(),
            "a check that reports nothing does not carry a window with no reset time"
        );
        assert!(
            board
                .get("claude")
                .unwrap()
                .usage
                .iter()
                .all(|w| !w.spent(now)),
            "no window reports spent after a clean check"
        );
    }
}
