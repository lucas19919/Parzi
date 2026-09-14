//! Thread-safe provider circuit breaker and rate-limit cooldown tracker.
//!
//! When an API endpoint responds with a 429 (rate-limited) or 503/529 (overloaded),
//! the breaker caches the cooldown window (e.g. 60s, or extracted `retry-after`).
//! Subsequent routing hops bypass the throttled provider instantly without
//! waiting for a failing network roundtrip.

use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Default)]
pub struct CircuitBreaker {
    /// Provider ID -> Unix timestamp (seconds) until which the provider is cooling down.
    cooldowns: RwLock<HashMap<String, u64>>,
    /// Provider ID -> consecutive failure count.
    consecutive_failures: RwLock<HashMap<String, u32>>,
    /// Provider ID -> last error message.
    last_errors: RwLock<HashMap<String, String>>,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self::default()
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Record a rate limit with a specific cooldown duration.
    pub fn record_rate_limit(&self, provider: &str, cooldown_secs: u64, reason: &str) {
        let until = Self::now_secs() + cooldown_secs.max(10);
        if let Ok(mut cds) = self.cooldowns.write() {
            cds.insert(provider.to_string(), until);
        }
        if let Ok(mut errs) = self.last_errors.write() {
            errs.insert(provider.to_string(), reason.to_string());
        }
        if let Ok(mut fails) = self.consecutive_failures.write() {
            let count = fails.entry(provider.to_string()).or_insert(0);
            *count = count.saturating_add(1);
        }
    }

    /// Record a generic failure. Trips cooldown after 3 consecutive failures.
    pub fn record_failure(&self, provider: &str, error: &str) {
        let failures = if let Ok(mut fails) = self.consecutive_failures.write() {
            let count = fails.entry(provider.to_string()).or_insert(0);
            *count = count.saturating_add(1);
            *count
        } else {
            1
        };

        if let Ok(mut errs) = self.last_errors.write() {
            errs.insert(provider.to_string(), error.to_string());
        }

        if failures >= 3 {
            let until = Self::now_secs() + 45;
            if let Ok(mut cds) = self.cooldowns.write() {
                cds.insert(provider.to_string(), until);
            }
        }
    }

    /// Record a successful response. Clears failures and active cooldowns.
    pub fn record_success(&self, provider: &str) {
        if let Ok(mut fails) = self.consecutive_failures.write() {
            fails.remove(provider);
        }
        if let Ok(mut cds) = self.cooldowns.write() {
            cds.remove(provider);
        }
        if let Ok(mut errs) = self.last_errors.write() {
            errs.remove(provider);
        }
    }

    /// Check if a provider is available (not cooling down).
    pub fn is_available(&self, provider: &str) -> bool {
        self.cooldown_remaining(provider).is_none()
    }

    /// Seconds remaining in cooldown, or None if ready.
    pub fn cooldown_remaining(&self, provider: &str) -> Option<u64> {
        let cds = self.cooldowns.read().ok()?;
        let until = cds.get(provider)?;
        let now = Self::now_secs();
        if *until > now {
            Some(*until - now)
        } else {
            None
        }
    }

    /// Last error recorded for the provider.
    pub fn last_error(&self, provider: &str) -> Option<String> {
        self.last_errors.read().ok()?.get(provider).cloned()
    }

    /// Reset cooldowns. If `provider` is None, resets all providers.
    pub fn reset(&self, provider: Option<&str>) {
        if let Some(p) = provider {
            if let Ok(mut cds) = self.cooldowns.write() {
                cds.remove(p);
            }
            if let Ok(mut fails) = self.consecutive_failures.write() {
                fails.remove(p);
            }
            if let Ok(mut errs) = self.last_errors.write() {
                errs.remove(p);
            }
        } else {
            if let Ok(mut cds) = self.cooldowns.write() {
                cds.clear();
            }
            if let Ok(mut fails) = self.consecutive_failures.write() {
                fails.clear();
            }
            if let Ok(mut errs) = self.last_errors.write() {
                errs.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CircuitBreaker;

    #[test]
    fn rate_limit_trips_and_expires() {
        let cb = CircuitBreaker::new();
        assert!(cb.is_available("antigravity"));
        cb.record_rate_limit("antigravity", 60, "429 quota");
        assert!(!cb.is_available("antigravity"));
        assert!(cb.cooldown_remaining("antigravity").is_some());
        assert_eq!(cb.last_error("antigravity").as_deref(), Some("429 quota"));
        // Success clears the cooldown.
        cb.record_success("antigravity");
        assert!(cb.is_available("antigravity"));
        assert!(cb.cooldown_remaining("antigravity").is_none());
    }

    #[test]
    fn repeated_failures_trip_cooldown() {
        let cb = CircuitBreaker::new();
        cb.record_failure("codex", "boom");
        cb.record_failure("codex", "boom");
        assert!(cb.is_available("codex"));
        cb.record_failure("codex", "boom");
        assert!(!cb.is_available("codex"));
    }

    #[test]
    fn reset_scopes_to_provider_or_all() {
        let cb = CircuitBreaker::new();
        cb.record_rate_limit("a", 60, "429");
        cb.record_rate_limit("b", 60, "429");
        cb.reset(Some("a"));
        assert!(cb.is_available("a"));
        assert!(!cb.is_available("b"));
        cb.reset(None);
        assert!(cb.is_available("b"));
    }
}
