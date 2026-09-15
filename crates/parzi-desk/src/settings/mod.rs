//! Settings: General, Appearance, Models and Connectors sections.
//!
//! Each section owns a plain state struct (`load` / `validate` / `save`)
//! plus a thin egui `show` view. Sections save independently: opening one
//! tab never blocks on another tab's data. Mirrors `Settings.svelte`.

pub mod appearance;
pub mod connectors;
pub mod general;
pub mod models;

pub use appearance::AppearanceState;
pub use connectors::ConnectorsState;
pub use general::GeneralState;
pub use models::ModelsState;

/// Settings tabs, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    Appearance,
    Models,
    Connectors,
}

impl SettingsTab {
    /// All tabs, in display order.
    pub const ALL: [Self; 4] = [Self::General, Self::Appearance, Self::Models, Self::Connectors];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Models => "Models",
            Self::Connectors => "Connectors",
        }
    }
}

/// The whole settings page: tab routing plus one state per section.
#[derive(Debug)]
pub struct SettingsState {
    pub tab: SettingsTab,
    pub general: GeneralState,
    pub appearance: AppearanceState,
    pub models: ModelsState,
    pub connectors: ConnectorsState,
    /// Transient status line (the web UI's sync toast).
    pub notice: String,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            tab: SettingsTab::General,
            general: GeneralState::default(),
            appearance: AppearanceState::default(),
            models: ModelsState::default(),
            connectors: ConnectorsState::default(),
            notice: String::new(),
        }
    }
}

impl SettingsState {
    /// Load every section from disk. A section that fails to load keeps its
    /// defaults so one corrupt file never blanks the whole page.
    #[must_use]
    pub fn load() -> Self {
        Self {
            tab: SettingsTab::General,
            general: GeneralState::load(),
            appearance: AppearanceState::load(),
            models: ModelsState::load(),
            connectors: ConnectorsState::load(),
            notice: String::new(),
        }
    }

    /// Show a transient status line.
    pub fn notify(&mut self, msg: impl Into<String>) {
        self.notice = msg.into();
    }

    /// Validate everything, then write `config.toml` (general/models/
    /// connectors) and `theme.toml` (appearance). Nothing is written when
    /// any section is invalid.
    ///
    /// # Errors
    ///
    /// Returns the first validation or I/O error encountered.
    pub fn save_all(&mut self) -> Result<(), String> {
        self.general.validate()?;
        self.models.validate()?;
        self.connectors.validate()?;
        self.appearance.validate()?;
        let mut cfg =
            parzi_core::config::ParziConfig::load().map_err(|e| format!("load config: {e}"))?;
        self.general.apply_to(&mut cfg);
        self.models.apply_to(&mut cfg);
        self.connectors.apply_to(&mut cfg);
        cfg.save().map_err(|e| format!("save config: {e}"))?;
        self.appearance.save()?;
        self.notice = "All settings saved".to_string();
        Ok(())
    }

    /// Tab bar plus the active section. Sections save themselves; the page
    /// only routes and shows the notice line.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for tab in SettingsTab::ALL {
                ui.selectable_value(&mut self.tab, tab, tab.label());
            }
        });
        ui.separator();
        match self.tab {
            SettingsTab::General => general::show(ui, &mut self.general),
            SettingsTab::Appearance => appearance::show(ui, &mut self.appearance),
            SettingsTab::Models => models::show(ui, &mut self.models),
            SettingsTab::Connectors => connectors::show(ui, &mut self.connectors),
        }
        if !self.notice.is_empty() {
            ui.separator();
            ui.label(self.notice.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tab_is_general() {
        let s = SettingsState::default();
        assert_eq!(s.tab, SettingsTab::General);
        assert!(s.notice.is_empty());
    }

    #[test]
    fn tab_labels_cover_all_tabs() {
        let labels: Vec<_> = SettingsTab::ALL.iter().map(|t| t.label()).collect();
        assert_eq!(labels, vec!["General", "Appearance", "Models", "Connectors"]);
    }

    #[test]
    fn notify_sets_notice() {
        let mut s = SettingsState::default();
        s.notify("Saved theme");
        assert_eq!(s.notice, "Saved theme");
    }

    #[test]
    fn save_all_rejects_invalid_section_first() {
        let mut s = SettingsState::default();
        s.general.default_mode = "sometimes".to_string();
        let err = s.save_all().expect_err("bad mode must fail");
        assert!(err.contains("mode"), "unexpected error: {err}");
    }
}
