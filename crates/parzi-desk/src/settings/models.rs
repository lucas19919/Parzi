//! Models settings: Smart Auto order, provider defaults, API keys in the
//! OS keyring, and Google OAuth for Antigravity (shared keyring slots).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use parzi_core::config::ParziConfig;

/// Providers that may route Smart Auto subscriptions, in default order.
pub const ROUTABLE: &[&str] = &["claude", "codex", "antigravity", "opencode"];

/// Google sign-in progress (worker thread; the UI only polls).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SignInStatus {
    #[default]
    Idle,
    WaitingForBrowser,
    Done,
    Failed(String),
}

/// Owns the worker thread handle state for one Google sign-in attempt.
#[derive(Debug, Clone, Default)]
pub struct GoogleSignIn {
    state: Arc<Mutex<SignInStatus>>,
}

impl GoogleSignIn {
    /// The Google consent URL (pure: safe in tests, no network).
    #[must_use]
    pub fn auth_url() -> String {
        parzi_providers::antigravity_oauth::auth_url()
    }

    /// Open the browser and wait for the localhost callback on a thread,
    /// then exchange the code and store both tokens in the keyring.
    pub fn start(&self) {
        if let Ok(mut guard) = self.state.lock() { *guard = SignInStatus::WaitingForBrowser; }
        let state = Arc::clone(&self.state);
        std::thread::spawn(move || {
            let outcome = run_sign_in_blocking();
            if let Ok(mut guard) = state.lock() { *guard = outcome; }
        });
    }

    /// Current status (never blocks).
    #[must_use]
    pub fn status(&self) -> SignInStatus {
        self.state.lock().ok().map_or(SignInStatus::Idle, |g| g.clone())
    }

    /// Forget Google tokens (subscription sign-ins of other providers stay).
    pub fn sign_out(&self) {
        for slot in ["antigravity", "antigravity-refresh", "antigravity-session"] {
            if let Ok(entry) = keyring::Entry::new("parzi", slot) {
                let _ = entry.delete_credential();
            }
        }
        if let Ok(mut guard) = self.state.lock() { *guard = SignInStatus::Idle; }
    }
}

/// Full OAuth loop: wait for the redirect, exchange the code, store the
/// tokens under the shared slot names. Runs on a worker thread.
fn run_sign_in_blocking() -> SignInStatus {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => return SignInStatus::Failed(format!("runtime: {e}")),
    };
    runtime.block_on(async {
        let code = match parzi_providers::antigravity_oauth::wait_for_code(300).await {
            Ok(code) => code,
            Err(e) => return SignInStatus::Failed(format!("callback: {e}")),
        };
        let toks = match parzi_providers::antigravity_oauth::exchange_code(&code).await {
            Ok(toks) => toks,
            Err(e) => return SignInStatus::Failed(format!("exchange: {e}")),
        };
        if let Ok(entry) = keyring::Entry::new("parzi", "antigravity") {
            if entry.set_password(&toks.access).is_err() {
                return SignInStatus::Failed("keyring store failed".to_string());
            }
        }
        if let Some(refresh) = toks.refresh {
            if let Ok(entry) = keyring::Entry::new("parzi", "antigravity-refresh") {
                let _ = entry.set_password(&refresh);
            }
        }
        SignInStatus::Done
    })
}

/// Plain models-settings state: load, edit, validate, save.
#[derive(Debug)]
pub struct ModelsState {
    pub default_provider: String,
    pub defaults: HashMap<String, String>,
    pub auto_order: Vec<String>,
    pub keys_in_auto: bool,
    pub auto_failover: bool,
    pub favorites: Vec<String>,
    /// Draft API key (cleared on save; never logged).
    pub key_input: String,
    pub google: GoogleSignIn,
    /// Last action result, shown under the form.
    pub status: String,
}

impl Default for ModelsState {
    fn default() -> Self {
        Self::from_config(&ParziConfig::default())
    }
}

impl ModelsState {
    #[must_use]
    pub fn from_config(cfg: &ParziConfig) -> Self {
        let defaults: HashMap<String, String> = cfg
            .providers
            .iter()
            .map(|(id, e)| (id.clone(), e.default_model.clone()))
            .collect();
        Self {
            default_provider: cfg.default_provider.clone(),
            defaults,
            auto_order: normalize_order(&cfg.routing.auto_order),
            keys_in_auto: cfg.routing.keys_in_auto,
            auto_failover: cfg.routing.auto_failover,
            favorites: cfg.favorite_models.clone(),
            key_input: String::new(),
            google: GoogleSignIn::default(),
            status: String::new(),
        }
    }

    /// Read from `config.toml`; failures fall back to defaults.
    #[must_use]
    pub fn load() -> Self {
        Self::from_config(&ParziConfig::load().unwrap_or_default())
    }

    pub fn apply_to(&self, cfg: &mut ParziConfig) {
        cfg.default_provider = self.default_provider.clone();
        for (id, model) in &self.defaults {
            if let Some(entry) = cfg.providers.get_mut(id) { entry.default_model = model.clone(); }
        }
        cfg.routing.auto_order = normalize_order(&self.auto_order);
        cfg.routing.keys_in_auto = self.keys_in_auto;
        cfg.routing.auto_failover = self.auto_failover;
        cfg.favorite_models = self.favorites.clone();
    }

    /// # Errors
    /// Unknown providers, empty default models, or malformed favourites.
    pub fn validate(&self) -> Result<(), String> {
        if parzi_providers::canonical_id(&self.default_provider).is_none() {
            return Err(format!("unknown provider `{}`", self.default_provider));
        }
        for id in &self.auto_order {
            if parzi_providers::canonical_id(id).is_none() {
                return Err(format!("unknown provider in Smart Auto order: `{id}`"));
            }
        }
        for (id, model) in &self.defaults {
            if parzi_providers::canonical_id(id).is_none() {
                return Err(format!("unknown provider `{id}`"));
            }
            if model.trim().is_empty() {
                return Err(format!("default model for {id} is empty"));
            }
        }
        for fav in &self.favorites {
            let (p, m) =
                fav.split_once('/').ok_or_else(|| format!("bad favourite `{fav}` (want provider/model)"))?;
            if parzi_providers::canonical_id(p).is_none() || m.trim().is_empty() {
                return Err(format!("bad favourite `{fav}`"));
            }
        }
        Ok(())
    }

    /// Validate, merge into the on-disk config and save.
    ///
    /// # Errors
    /// Validation or I/O errors; `status` describes the outcome.
    pub fn save(&mut self) -> Result<(), String> {
        let outcome = self.save_inner();
        self.status = match &outcome {
            Ok(()) => "Saved models".to_string(),
            Err(e) => e.clone(),
        };
        outcome
    }

    fn save_inner(&self) -> Result<(), String> {
        self.validate()?;
        let mut cfg = ParziConfig::load().map_err(|e| format!("load config: {e}"))?;
        self.apply_to(&mut cfg);
        cfg.save().map_err(|e| format!("save config: {e}"))?;
        Ok(())
    }

    /// Move a provider up (`-1`) or down (`+1`) in the Smart Auto order.
    pub fn move_provider(&mut self, id: &str, dir: i8) {
        let mut order = normalize_order(&self.auto_order);
        if let Some(i) = order.iter().position(|p| p == id) {
            let j = i.saturating_add_signed(dir as isize);
            if j < order.len() {
                order.swap(i, j);
                self.auto_order = order;
            }
        }
    }

    /// Star (`true`) or unstar (`false`) a `provider/model` spec.
    ///
    /// # Errors
    /// Specs without a `/` or with an unknown provider.
    pub fn set_favorite(&mut self, spec: &str, star: bool) -> Result<(), String> {
        let (p, m) = spec.split_once('/').ok_or_else(|| format!("bad favourite `{spec}`"))?;
        if parzi_providers::canonical_id(p).is_none() || m.trim().is_empty() {
            return Err(format!("bad favourite `{spec}`"));
        }
        self.favorites.retain(|f| f != spec);
        if star { self.favorites.push(spec.to_string()); }
        Ok(())
    }

    /// Is there a stored API key for this provider?
    #[must_use]
    pub fn has_key(provider: &str) -> bool {
        parzi_providers::key_entry(provider).is_some_and(|slot| {
            keyring::Entry::new("parzi", slot)
                .is_ok_and(|e| e.get_password().is_ok_and(|v| !v.trim().is_empty()))
        })
    }

    /// Store `key_input` for a provider and clear the draft.
    ///
    /// # Errors
    /// Providers without a slot, empty keys, or keyring failures.
    pub fn save_key(&mut self, provider: &str) -> Result<(), String> {
        let slot =
            parzi_providers::key_entry(provider).ok_or_else(|| format!("{provider} takes no API key"))?;
        let value = self.key_input.trim().to_string();
        if value.is_empty() {
            return Err("empty key".to_string());
        }
        let entry = keyring::Entry::new("parzi", slot).map_err(|e| format!("keyring: {e}"))?;
        entry.set_password(&value).map_err(|e| format!("keyring: {e}"))?;
        self.key_input.clear();
        self.status = format!("Saved API key for {provider}");
        Ok(())
    }

    /// Remove a stored API key (subscription sign-ins are untouched).
    ///
    /// # Errors
    /// Providers without a slot.
    pub fn delete_key(&mut self, provider: &str) -> Result<(), String> {
        let slot =
            parzi_providers::key_entry(provider).ok_or_else(|| format!("{provider} takes no API key"))?;
        if let Ok(entry) = keyring::Entry::new("parzi", slot) {
            let _ = entry.delete_credential();
        }
        self.status = format!("Removed API key for {provider}");
        Ok(())
    }
}

/// Canonicalize, dedupe and complete the Smart Auto order.
#[must_use]
pub fn normalize_order(order: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for id in order {
        if let Some(canon) = parzi_providers::canonical_id(id) {
            if ROUTABLE.contains(&canon) && !out.iter().any(|x| x == canon) {
                out.push(canon.to_string());
            }
        }
    }
    for id in ROUTABLE {
        if !out.iter().any(|x| x == *id) {
            out.push((*id).to_string());
        }
    }
    out
}

/// Thin form: Smart Auto order, provider defaults, keys, Google.
pub fn show(ui: &mut egui::Ui, s: &mut ModelsState) {
    ui.heading("Models");
    ui.label("Smart Auto walks subscriptions in this order. Keys are explicit picks unless they may join.");
    ui.separator();
    for i in 0..s.auto_order.len() {
        let id = s.auto_order[i].clone();
        ui.horizontal(|ui| {
            ui.label(format!("{}. {}", i + 1, parzi_providers::display_name(&id)));
            if ui.small_button("^").clicked() { s.move_provider(&id, -1); }
            if ui.small_button("v").clicked() { s.move_provider(&id, 1); }
        });
    }
    ui.checkbox(&mut s.keys_in_auto, "Let API keys join Smart Auto");
    ui.checkbox(&mut s.auto_failover, "Fail over on rate limits");
    ui.separator();
    for pid in parzi_providers::PROVIDERS {
        ui.horizontal(|ui| {
            ui.label(parzi_providers::display_name(pid));
            let def = s.defaults.entry((*pid).to_string()).or_default();
            ui.text_edit_singleline(def);
        });
        ui.horizontal(|ui| {
            if parzi_providers::key_entry(pid).is_some() {
                ui.label(if ModelsState::has_key(pid) { "key set" } else { "no key" });
                ui.add(egui::TextEdit::singleline(&mut s.key_input).password(true).hint_text("API key"));
                if ui.small_button("Save key").clicked() {
                    let _ = s.save_key(pid).map_err(|e| s.status = e);
                }
                if ui.small_button("Remove").clicked() {
                    let _ = s.delete_key(pid).map_err(|e| s.status = e);
                }
            } else if *pid == "antigravity" {
                match s.google.status() {
                    SignInStatus::Done => {
                        ui.label("signed in");
                        if ui.small_button("Sign out").clicked() { s.google.sign_out(); }
                    }
                    SignInStatus::WaitingForBrowser => { ui.label("waiting for browser..."); }
                    SignInStatus::Failed(e) => {
                        ui.label(format!("failed: {e}"));
                        if ui.small_button("Retry").clicked() { open_google_sign_in(&s.google); }
                    }
                    SignInStatus::Idle => {
                        if ui.small_button("Sign in with Google").clicked() {
                            open_google_sign_in(&s.google);
                        }
                    }
                }
            }
        });
    }
    ui.separator();
    if ui.button("Save").clicked() { let _ = s.save(); }
    if !s.status.is_empty() { ui.label(s.status.as_str()); }
}

/// Open the consent URL and start the callback worker (returns the URL).
pub fn open_google_sign_in(g: &GoogleSignIn) -> String {
    let url = GoogleSignIn::auth_url();
    parzi_providers::antigravity_oauth::open_browser(&url);
    g.start();
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_normalizes_dedupes_and_completes() {
        let order = normalize_order(&["codex".into(), "codex".into(), "hal".into()]);
        assert_eq!(order, vec!["codex", "claude", "antigravity", "opencode"]);
        assert_eq!(normalize_order(&["anthropic".into()])[0], "claude");
    }

    #[test]
    fn defaults_validate_and_round_trip() {
        let s = ModelsState::default();
        s.validate().expect("defaults valid");
        let mut cfg = ParziConfig::default();
        s.apply_to(&mut cfg);
        let back = ModelsState::from_config(&cfg);
        back.validate().expect("round-trip valid");
        assert_eq!(back.auto_order, normalize_order(&[]));
    }

    #[test]
    fn rejects_bad_order_and_favourites() {
        let mut s = ModelsState::default();
        s.auto_order.push("hal".to_string());
        assert!(s.validate().is_err());
        let mut s = ModelsState::default();
        assert!(s.set_favorite("nope", true).is_err());
        assert!(s.set_favorite("hal/model", true).is_err());
        s.set_favorite("claude/sonnet", true).expect("star");
        assert!(s.favorites.contains(&"claude/sonnet".to_string()));
        s.set_favorite("claude/sonnet", false).expect("unstar");
        assert!(!s.favorites.contains(&"claude/sonnet".to_string()));
    }

    #[test]
    fn auth_url_shape_and_shared_key_slots_without_network() {
        assert_eq!(parzi_providers::key_entry("claude"), Some("anthropic"));
        assert_eq!(parzi_providers::key_entry("codex"), Some("openai"));
        assert_eq!(parzi_providers::key_entry("xai"), Some("xai"));
        assert_eq!(parzi_providers::key_entry("opencode"), Some("opencode"));
        assert_eq!(parzi_providers::key_entry("antigravity"), None);
        let url = GoogleSignIn::auth_url();
        assert!(url.starts_with("https://accounts.google.com/"), "{url}");
        assert!(url.contains("localhost"), "{url}");
        assert_eq!(GoogleSignIn::default().status(), SignInStatus::Idle);
    }

    #[test]
    fn move_provider_reorders() {
        let mut s = ModelsState::default();
        s.move_provider("opencode", -1);
        assert_eq!(s.auto_order[2], "opencode");
        s.move_provider("opencode", 1);
        assert_eq!(s.auto_order[3], "opencode");
    }
}
