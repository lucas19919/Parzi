//! General settings: default provider, lane policy and runtime limits.
//!
//! Mirrors the General section of the web UI: every control edits a plain
//! [`GeneralState`], validation rejects bad values before they reach the
//! config file, and `save` writes the same TOML the CLI reads.

use parzi_core::config::ParziConfig;

/// Lane execution modes accepted by the runtime approver.
pub const MODES: &[&str] = &["auto", "ask", "deny"];
/// Upper bound for lane steps.
pub const MAX_STEPS_LIMIT: u32 = 256;
/// Upper bound for concurrent runs.
pub const CONCURRENT_LIMIT: usize = 16;
/// Upper bound for MCP idle eviction, in seconds.
pub const IDLE_KILL_LIMIT: u64 = 3600;

/// Plain general-settings state: load, edit, validate, save.
#[derive(Debug, Clone)]
pub struct GeneralState {
    pub default_provider: String,
    pub default_mode: String,
    pub max_steps: u32,
    pub max_concurrent: usize,
    pub queue_when_busy: bool,
    pub mcp_idle_kill_secs: u64,
    /// Last save result, shown under the form.
    pub status: String,
}

impl Default for GeneralState {
    fn default() -> Self {
        Self::from_config(&ParziConfig::default())
    }
}

impl GeneralState {
    /// Snapshot the general slice of a loaded config.
    #[must_use]
    pub fn from_config(cfg: &ParziConfig) -> Self {
        Self {
            default_provider: cfg.default_provider.clone(),
            default_mode: cfg.lanes.default_mode.clone(),
            max_steps: cfg.lanes.max_steps,
            max_concurrent: cfg.orchestrator.max_concurrent,
            queue_when_busy: cfg.orchestrator.queue_when_busy,
            mcp_idle_kill_secs: cfg.orchestrator.mcp_idle_kill_secs,
            status: String::new(),
        }
    }

    /// Read from `config.toml`; unreadable files fall back to defaults so
    /// settings always opens.
    #[must_use]
    pub fn load() -> Self {
        Self::from_config(&ParziConfig::load().unwrap_or_default())
    }

    /// Write the general slice back into a config (caller saves the file).
    pub fn apply_to(&self, cfg: &mut ParziConfig) {
        cfg.default_provider = self.default_provider.clone();
        cfg.lanes.default_mode = self.default_mode.clone();
        cfg.lanes.max_steps = self.max_steps;
        cfg.orchestrator.max_concurrent = self.max_concurrent;
        cfg.orchestrator.queue_when_busy = self.queue_when_busy;
        cfg.orchestrator.mcp_idle_kill_secs = self.mcp_idle_kill_secs;
    }

    /// Reject values the runtime would choke on before they reach the file.
    ///
    /// # Errors
    ///
    /// Returns a human-readable reason when any field is out of range.
    pub fn validate(&self) -> Result<(), String> {
        if parzi_providers::canonical_id(&self.default_provider).is_none() {
            return Err(format!("unknown provider `{}`", self.default_provider));
        }
        if !MODES.contains(&self.default_mode.as_str()) {
            return Err(format!(
                "mode `{}` invalid (auto | ask | deny)",
                self.default_mode
            ));
        }
        if self.max_steps == 0 || self.max_steps > MAX_STEPS_LIMIT {
            return Err(format!("max steps: 1-{MAX_STEPS_LIMIT}"));
        }
        if self.max_concurrent == 0 || self.max_concurrent > CONCURRENT_LIMIT {
            return Err(format!("max concurrent: 1-{CONCURRENT_LIMIT}"));
        }
        if self.mcp_idle_kill_secs == 0 || self.mcp_idle_kill_secs > IDLE_KILL_LIMIT {
            return Err(format!("idle eviction: 1-{IDLE_KILL_LIMIT} seconds"));
        }
        Ok(())
    }

    /// Validate, merge into the on-disk config and save.
    ///
    /// # Errors
    ///
    /// Returns validation or I/O errors; `status` always describes the outcome.
    pub fn save(&mut self) -> Result<(), String> {
        let outcome = self.save_inner();
        self.status = match &outcome {
            Ok(()) => "Saved general settings".to_string(),
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

    /// Drop edits and re-read the file.
    pub fn revert(&mut self) {
        *self = Self::load();
    }

    /// Set the lane mode (`auto` | `ask` | `deny`).
    ///
    /// # Errors
    ///
    /// Rejects anything outside [`MODES`].
    pub fn set_mode(&mut self, mode: &str) -> Result<(), String> {
        if !MODES.contains(&mode) {
            return Err(format!("mode `{mode}` invalid (auto | ask | deny)"));
        }
        self.default_mode = mode.to_string();
        Ok(())
    }

    /// Set the default provider; legacy ids fold to the canonical roster id.
    ///
    /// # Errors
    ///
    /// Rejects ids outside the provider roster.
    pub fn set_provider(&mut self, id: &str) -> Result<(), String> {
        let canon =
            parzi_providers::canonical_id(id).ok_or_else(|| format!("unknown provider `{id}`"))?;
        self.default_provider = canon.to_string();
        Ok(())
    }
}

/// Thin form: identity, lane policy, runtime limits, save/revert, status.
pub fn show(ui: &mut egui::Ui, s: &mut GeneralState) {
    ui.heading("General");
    ui.label("Identity, lane policy and runtime limits. Saves to config.toml.");
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Default provider");
        egui::ComboBox::from_id_salt("general-provider")
            .selected_text(parzi_providers::display_name(&s.default_provider))
            .show_ui(ui, |ui| {
                for id in parzi_providers::PROVIDERS {
                    let current = s.default_provider.clone();
                    let selected = parzi_providers::canonical_id(&current) == Some(*id);
                    if ui
                        .selectable_label(selected, parzi_providers::display_name(id))
                        .clicked()
                    {
                        s.default_provider = (*id).to_string();
                    }
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label("Lane mode");
        for mode in MODES {
            ui.selectable_value(&mut s.default_mode, (*mode).to_string(), *mode);
        }
    });
    ui.label("auto runs tools silently; ask prompts; deny blocks.");
    ui.add(egui::Slider::new(&mut s.max_steps, 1..=MAX_STEPS_LIMIT).text("Max steps per run"));
    ui.add(egui::Slider::new(&mut s.max_concurrent, 1..=CONCURRENT_LIMIT).text("Concurrent runs"));
    ui.add(
        egui::Slider::new(&mut s.mcp_idle_kill_secs, 1..=IDLE_KILL_LIMIT)
            .text("MCP idle eviction (s)"),
    );
    ui.checkbox(&mut s.queue_when_busy, "Queue runs when busy");
    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            let _ = s.save();
        }
        if ui.button("Revert").clicked() {
            s.revert();
        }
    });
    if !s.status.is_empty() {
        ui.label(s.status.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate() {
        GeneralState::default().validate().expect("defaults valid");
    }

    #[test]
    fn rejects_unknown_provider_bad_mode_and_ranges() {
        let mut s = GeneralState::default();
        s.default_provider = "hal".to_string();
        assert!(s.validate().is_err());
        s = GeneralState::default();
        s.default_mode = "sometimes".to_string();
        assert!(s.set_mode("sometimes").is_err());
        assert!(s.validate().is_err());
        for bad in [0, MAX_STEPS_LIMIT + 1] {
            let mut v = GeneralState::default();
            v.max_steps = bad;
            assert!(v.validate().is_err(), "steps {bad}");
        }
        let mut v = GeneralState::default();
        v.max_concurrent = 0;
        assert!(v.validate().is_err());
        let mut v = GeneralState::default();
        v.mcp_idle_kill_secs = IDLE_KILL_LIMIT + 1;
        assert!(v.validate().is_err());
    }

    #[test]
    fn provider_alias_folds_to_canonical() {
        let mut s = GeneralState::default();
        s.set_provider("anthropic").expect("alias accepted");
        assert_eq!(s.default_provider, "claude");
        assert!(s.set_provider("hal").is_err());
    }

    #[test]
    fn from_config_apply_to_round_trip() {
        let mut cfg = ParziConfig::default();
        let mut s = GeneralState::from_config(&cfg);
        s.set_mode("auto").expect("valid mode");
        s.max_steps = 12;
        s.max_concurrent = 2;
        s.queue_when_busy = false;
        s.apply_to(&mut cfg);
        let back = GeneralState::from_config(&cfg);
        assert_eq!(back.default_mode, "auto");
        assert_eq!(back.max_steps, 12);
        assert_eq!(back.max_concurrent, 2);
        assert!(!back.queue_when_busy);
        back.validate().expect("round-trip valid");
    }
}
