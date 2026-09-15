//! Appearance: every field of `theme.toml` (packs, wallpaper, accent,
//! colours, type, glass). Mirrors `AppearanceSection.svelte`.

use parzi_core::theme::{self, Theme};

/// Swatch shortcuts for the accent picker.
pub const ACCENTS: &[&str] = &[
    "#7C8CFF", "#7AA2F7", "#88C0D0", "#CBA6F7", "#EB6F92", "#F5A97F", "#A6DA95", "#E0DEF4",
];

/// Plain appearance state: load, edit, validate, save.
#[derive(Debug, Clone)]
pub struct AppearanceState {
    pub theme: Theme,
    /// Pack whose colours match the live theme, if any.
    pub pack: String,
    pub packs: Vec<String>,
    /// Last save result, shown under the form.
    pub status: String,
}

impl Default for AppearanceState {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl AppearanceState {
    #[must_use]
    pub fn from_theme(theme: Theme) -> Self {
        let packs = theme::list_packs().unwrap_or_default();
        let pack = packs
            .iter()
            .find(|p| pack_file_matches(p, &theme))
            .cloned()
            .unwrap_or_default();
        Self {
            theme,
            pack,
            packs,
            status: String::new(),
        }
    }

    /// Read `theme.toml`; unreadable files fall back to defaults.
    #[must_use]
    pub fn load() -> Self {
        Self::from_theme(Theme::load().unwrap_or_default())
    }

    /// Validate, then persist `theme.toml`.
    ///
    /// # Errors
    /// Validation or I/O errors; `status` always describes the outcome.
    pub fn save(&mut self) -> Result<(), String> {
        let outcome = self.save_inner();
        self.status = match &outcome {
            Ok(()) => "Saved appearance".to_string(),
            Err(e) => e.clone(),
        };
        outcome
    }

    fn save_inner(&self) -> Result<(), String> {
        self.validate()?;
        self.theme.save().map_err(|e| format!("save theme: {e}"))?;
        Ok(())
    }

    /// Hex colours must parse; numbers must sit inside the ranges core
    /// clamps to (checked first so the user sees a message, not silent
    /// clamping).
    ///
    /// # Errors
    /// A human-readable reason naming the offending field.
    pub fn validate(&self) -> Result<(), String> {
        let c = &self.theme.colors;
        for (name, value) in [
            ("sidebar", c.sidebar.as_str()),
            ("stage", c.stage.as_str()),
            ("accent", c.accent.as_str()),
            ("text", c.text.as_str()),
            ("muted text", c.text_dim.as_str()),
            ("composer", c.bar.as_str()),
            ("border", c.border.as_str()),
        ] {
            if parse_hex_color(value).is_none() {
                return Err(format!("colour {name} `{value}` is not #RGB or #RRGGBB"));
            }
        }
        let (f, b, g) = (&self.theme.font, &self.theme.background, &self.theme.glass);
        if !(10..=20).contains(&f.size) {
            return Err("interface size: 10-20".into());
        }
        if !(9..=18).contains(&f.mono_size) {
            return Err("code size: 9-18".into());
        }
        if !(0.0..=1.0).contains(&b.dim) {
            return Err("wallpaper dim: 0-100%".into());
        }
        if !(0.0..=1.0).contains(&b.vignette) {
            return Err("vignette: 0-100%".into());
        }
        if !(0.0..=40.0).contains(&b.blur) {
            return Err("wallpaper blur: 0-40px".into());
        }
        if !(0.0..=1.0).contains(&g.opacity) {
            return Err("glass opacity: 0-100%".into());
        }
        Ok(())
    }

    /// Apply a named pack; the current wallpaper survives per core rules.
    ///
    /// # Errors
    /// The core error when the pack is missing or unparsable.
    pub fn apply_pack(&mut self, name: &str) -> Result<(), String> {
        self.theme = theme::apply_pack(name).map_err(|e| format!("apply theme: {e}"))?;
        self.pack = name.to_string();
        self.status = format!("Theme: {name}");
        Ok(())
    }

    /// Point the live theme at a saved wallpaper ("" = solid).
    ///
    /// # Errors
    /// The core error for unknown names.
    pub fn set_background(&mut self, name: &str) -> Result<(), String> {
        self.theme = theme::set_background(name).map_err(|e| format!("wallpaper: {e}"))?;
        self.status = if name.is_empty() {
            "Wallpaper off".into()
        } else {
            format!("Wallpaper: {name}")
        };
        Ok(())
    }

    /// Derive the accent from the current wallpaper art.
    ///
    /// # Errors
    /// No wallpaper set, or the art is unreadable.
    pub fn sample_accent(&mut self) -> Result<(), String> {
        if self.theme.background.image.trim().is_empty() {
            return Err("Choose a wallpaper first".to_string());
        }
        let home = parzi_core::paths::parzi_dir().map_err(|e| format!("locate home: {e}"))?;
        let pal = theme::extract_palette(&home.join(&self.theme.background.image))
            .map_err(|e| format!("sample wallpaper: {e}"))?;
        self.theme.colors.accent = pal.accent.clone();
        self.status = format!("Accent from wallpaper: {}", pal.accent);
        Ok(())
    }

    /// Restore defaults and persist them.
    ///
    /// # Errors
    /// The I/O error when the default theme cannot be saved.
    pub fn reset(&mut self) -> Result<(), String> {
        self.theme = Theme::default();
        self.validate()?;
        self.theme.save().map_err(|e| format!("reset theme: {e}"))?;
        self.status = "Appearance reset".to_string();
        Ok(())
    }
}

/// Does the pack file's colour set match the live theme? A tiny line scan
/// (no extra deps): only the seven `key = "value"` colour lines.
fn pack_file_matches(name: &str, theme: &Theme) -> bool {
    let raw = theme::themes_dir()
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join(name).join("theme.toml")).ok())
        .unwrap_or_default();
    if raw.trim().is_empty() {
        return false;
    }
    let c = &theme.colors;
    [
        ("sidebar", c.sidebar.as_str()),
        ("stage", c.stage.as_str()),
        ("bar", c.bar.as_str()),
        ("border", c.border.as_str()),
        ("accent", c.accent.as_str()),
        ("text", c.text.as_str()),
        ("text_dim", c.text_dim.as_str()),
    ]
    .iter()
    .all(|(key, live)| {
        extract_toml_string(&raw, key).is_some_and(|v| v.to_lowercase() == live.to_lowercase())
    })
}

/// First `key = "value"` line in a TOML file. Narrow on purpose: pack
/// matching only needs colour keys.
fn extract_toml_string(raw: &str, key: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(key)?.trim_start();
        let rest = rest.strip_prefix('=')?.trim_start();
        Some(rest.strip_prefix('"')?.split('"').next()?.to_string())
    })
}

/// Parse `#RGB`, `#RRGGBB` or `#RRGGBBAA` (alpha ignored by the picker).
#[must_use]
pub fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        3 => {
            let nib = |i: usize| u8::from_str_radix(h.get(i..=i)?, 16).ok().map(|n| n * 17);
            Some([nib(0)?, nib(1)?, nib(2)?])
        }
        6 | 8 => Some([byte(0)?, byte(2)?, byte(4)?]),
        _ => None,
    }
}

/// Format a picker colour back to `#RRGGBB`.
#[must_use]
pub fn hex_of(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// One colour row: swatch button bound to a hex string field.
fn color_row(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut rgb = parse_hex_color(value).unwrap_or([0, 0, 0]);
        if ui.color_edit_button_srgb(&mut rgb).changed() {
            *value = hex_of(rgb);
        }
        ui.monospace(value.as_str());
    });
}

/// Thin form: packs, wallpaper, accent, colours, type, glass, save/reset.
pub fn show(ui: &mut egui::Ui, s: &mut AppearanceState) {
    ui.heading("Appearance");
    ui.label("Every field of theme.toml. Saves apply instantly, no restart.");
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Pack");
        egui::ComboBox::from_id_salt("appearance-pack")
            .selected_text(if s.pack.is_empty() { "Custom" } else { &s.pack })
            .show_ui(ui, |ui| {
                for name in s.packs.clone() {
                    if ui.selectable_label(s.pack == name, &name).clicked() {
                        let _ = s.apply_pack(&name);
                    }
                }
            });
        if ui.button("Refresh").clicked() {
            s.packs = theme::list_packs().unwrap_or_default();
        }
    });
    ui.collapsing("Wallpaper", |ui| {
        ui.horizontal(|ui| {
            ui.label("File");
            ui.text_edit_singleline(&mut s.theme.background.image);
            if ui.button("Off").clicked() {
                let _ = s.set_background("");
            }
        });
        ui.add(egui::Slider::new(&mut s.theme.background.dim, 0.0..=1.0).text("Dim"));
        ui.add(egui::Slider::new(&mut s.theme.background.blur, 0.0..=40.0).text("Blur (px)"));
        ui.add(egui::Slider::new(&mut s.theme.background.vignette, 0.0..=1.0).text("Vignette"));
        if ui.button("Sample accent from wallpaper").clicked() {
            let _ = s.sample_accent().map_err(|e| s.status = e);
        }
    });
    ui.collapsing("Accent", |ui| {
        ui.horizontal_wrapped(|ui| {
            for hex in ACCENTS {
                let rgb = parse_hex_color(hex).unwrap_or([124, 140, 255]);
                let btn =
                    egui::Button::new("  ").fill(egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                if ui.add(btn).clicked() {
                    s.theme.colors.accent = (*hex).to_string();
                }
            }
        });
        color_row(ui, "Custom", &mut s.theme.colors.accent);
    });
    ui.collapsing("Colours", |ui| {
        let c = &mut s.theme.colors;
        color_row(ui, "Sidebar", &mut c.sidebar);
        color_row(ui, "Stage", &mut c.stage);
        color_row(ui, "Composer", &mut c.bar);
        color_row(ui, "Border", &mut c.border);
        color_row(ui, "Text", &mut c.text);
        color_row(ui, "Muted", &mut c.text_dim);
    });
    ui.collapsing("Type and glass", |ui| {
        ui.horizontal(|ui| {
            ui.label("Interface");
            ui.text_edit_singleline(&mut s.theme.font.family);
            ui.add(egui::Slider::new(&mut s.theme.font.size, 10..=20).text("px"));
        });
        ui.horizontal(|ui| {
            ui.label("Code");
            ui.text_edit_singleline(&mut s.theme.font.mono);
            ui.add(egui::Slider::new(&mut s.theme.font.mono_size, 9..=18).text("px"));
        });
        ui.add(egui::Slider::new(&mut s.theme.glass.opacity, 0.0..=1.0).text("Opacity"));
        ui.add(egui::Slider::new(&mut s.theme.glass.blur_px, 0..=60).text("Blur (px)"));
        ui.add(egui::Slider::new(&mut s.theme.glass.radius, 0..=32).text("Radius (px)"));
        ui.checkbox(&mut s.theme.glass.shadow, "Shadows");
    });
    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            let _ = s.save();
        }
        if ui.button("Reset").clicked() {
            let _ = s.reset().map_err(|e| s.status = e);
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
    fn hex_round_trip() {
        assert_eq!(parse_hex_color("#fff"), Some([255, 255, 255]));
        assert_eq!(parse_hex_color("#0B0B10"), Some([11, 11, 16]));
        assert_eq!(parse_hex_color("#7C8CFF80"), Some([124, 140, 255]));
        assert_eq!(parse_hex_color("7C8CFF"), None);
        assert_eq!(parse_hex_color("#zzzzzz"), None);
        assert_eq!(hex_of([124, 140, 255]), "#7C8CFF");
    }

    #[test]
    fn defaults_validate() {
        AppearanceState::default()
            .validate()
            .expect("defaults valid");
    }

    #[test]
    fn rejects_bad_colour_and_ranges() {
        let mut s = AppearanceState::default();
        s.theme.colors.accent = "nope".to_string();
        assert!(s.validate().expect_err("bad colour").contains("accent"));
        let mut s = AppearanceState::default();
        s.theme.font.size = 99;
        assert!(s.validate().is_err());
        let mut s = AppearanceState::default();
        s.theme.background.dim = 2.0;
        assert!(s.validate().is_err());
    }

    #[test]
    fn toml_scan_reads_colours() {
        let raw = "[colors]\nsidebar = \"#07070B\"\ntext = \"#EDEDF2\"\ntext_dim = \"#9AA0AE\"\n";
        assert_eq!(
            extract_toml_string(raw, "sidebar"),
            Some("#07070B".to_string())
        );
        assert_eq!(
            extract_toml_string(raw, "text"),
            Some("#EDEDF2".to_string())
        );
        assert_eq!(extract_toml_string(raw, "missing"), None);
    }

    #[test]
    fn sample_accent_without_wallpaper_fails_cleanly() {
        let mut s = AppearanceState::default();
        s.theme.background.image.clear();
        assert!(s
            .sample_accent()
            .expect_err("needs wallpaper")
            .contains("wallpaper"));
    }
}
