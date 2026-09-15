//! `theme.toml` (parzi-core) mapped onto egui `Visuals`, the text styles, and
//! our own colour tokens for everything egui has no slot for.

use std::path::PathBuf;

use egui::{Color32, Context, CornerRadius, FontFamily, FontId, Stroke, Style, TextStyle, Visuals};
use epaint::Shadow;
use parzi_core::theme::Theme;

use crate::fonts;

/// Named text style: the serif wordmark and hero line only.
pub const HERO: &str = "hero";
/// Named text style: Inter 500 for labels and controls.
pub const LABEL: &str = "label";

/// Resolved theme: colours as `Color32`, numbers clamped, paths absolute.
#[derive(Debug, Clone)]
pub struct Tokens {
    pub sidebar: Color32,
    pub stage: Color32,
    pub bar: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub text: Color32,
    pub text_dim: Color32,

    pub glass_opacity: f32,
    pub glass_radius: u8,
    pub glass_shadow: bool,

    /// Absolute path of the wallpaper, if one is configured.
    pub bg_image: Option<PathBuf>,
    pub bg_dim: f32,
    pub bg_vignette: f32,
    /// Blur sigma in points at 1x; scaled by the display factor when applied.
    pub bg_blur: f32,

    pub body_size: f32,
    pub mono_size: f32,
}

/// Read `~/.parzi/theme.toml`; an unreadable file falls back to defaults so
/// the window always opens.
pub fn load() -> Tokens {
    let theme = Theme::load().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "theme.toml unreadable, using defaults");
        Theme::default()
    });
    Tokens::from_theme(&theme)
}

impl Tokens {
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)] // clamped before the cast
    pub fn from_theme(t: &Theme) -> Self {
        let d = Theme::default();
        let c = &t.colors;
        let dc = &d.colors;
        let bg_image = if t.background.image.trim().is_empty() {
            None
        } else {
            parzi_core::paths::parzi_dir()
                .ok()
                .map(|home| home.join(&t.background.image))
        };
        Self {
            sidebar: hex(&c.sidebar).unwrap_or_else(|| hex(&dc.sidebar).unwrap_or(Color32::BLACK)),
            stage: hex(&c.stage).unwrap_or_else(|| hex(&dc.stage).unwrap_or(Color32::BLACK)),
            bar: hex(&c.bar).unwrap_or_else(|| hex(&dc.bar).unwrap_or(Color32::DARK_GRAY)),
            border: hex(&c.border).unwrap_or_else(|| hex(&dc.border).unwrap_or(Color32::GRAY)),
            accent: hex(&c.accent)
                .unwrap_or_else(|| hex(&dc.accent).unwrap_or(Color32::LIGHT_BLUE)),
            text: hex(&c.text).unwrap_or_else(|| hex(&dc.text).unwrap_or(Color32::WHITE)),
            text_dim: hex(&c.text_dim)
                .unwrap_or_else(|| hex(&dc.text_dim).unwrap_or(Color32::GRAY)),

            glass_opacity: t.glass.opacity.clamp(0.0, 1.0) as f32,
            glass_radius: t.glass.radius.min(32) as u8,
            glass_shadow: t.glass.shadow,

            bg_image,
            bg_dim: t.background.dim.clamp(0.0, 1.0) as f32,
            bg_vignette: t.background.vignette.clamp(0.0, 1.0) as f32,
            bg_blur: t.background.blur.clamp(0.0, 40.0) as f32,

            body_size: t.font.size.clamp(10, 20) as f32,
            mono_size: t.font.mono_size.clamp(9, 18) as f32,
        }
    }

    /// A panel colour at the glass opacity.
    pub fn glass_fill(&self, base: Color32) -> Color32 {
        with_alpha(base, self.glass_opacity)
    }

    pub fn hairline(&self) -> Stroke {
        Stroke::new(1.0, self.border)
    }

    pub fn shadow(&self) -> Shadow {
        if self.glass_shadow {
            Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(96),
            }
        } else {
            Shadow::NONE
        }
    }
}

/// Install visuals and text styles on the context. Called once at start-up
/// and again whenever the theme file changes (M4).
pub fn apply(ctx: &Context, t: &Tokens) {
    ctx.set_visuals(visuals(t));
    ctx.all_styles_mut(|style| text_styles(style, t));
}

fn visuals(t: &Tokens) -> Visuals {
    let mut v = Visuals::dark();
    v.text_options.font_hinting = true;
    v.text_options.subpixel_binning = true;

    v.override_text_color = Some(t.text);
    v.weak_text_color = Some(t.text_dim);
    v.panel_fill = t.stage;
    v.window_fill = t.bar;
    v.extreme_bg_color = t.sidebar;
    v.faint_bg_color = with_alpha(t.bar, 0.5);
    v.code_bg_color = t.bar;
    v.text_edit_bg_color = Some(t.sidebar);
    v.hyperlink_color = t.accent;
    v.selection.bg_fill = with_alpha(t.accent, 0.35);
    v.selection.stroke = Stroke::new(1.0, t.accent);

    let radius = CornerRadius::same(t.glass_radius);
    let small = CornerRadius::same(6);
    v.window_stroke = t.hairline();
    v.window_corner_radius = radius;
    v.menu_corner_radius = radius;
    v.window_shadow = t.shadow();
    v.popup_shadow = t.shadow();

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.stage;
    w.noninteractive.weak_bg_fill = t.stage;
    w.noninteractive.bg_stroke = t.hairline();
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.text_dim);
    w.noninteractive.corner_radius = small;

    w.inactive.bg_fill = t.bar;
    w.inactive.weak_bg_fill = with_alpha(t.bar, 0.6);
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, t.text);
    w.inactive.corner_radius = small;

    w.hovered.bg_fill = lift(t.bar, 0.08);
    w.hovered.weak_bg_fill = lift(t.bar, 0.08);
    w.hovered.bg_stroke = t.hairline();
    w.hovered.fg_stroke = Stroke::new(1.0, t.text);
    w.hovered.corner_radius = small;

    w.active.bg_fill = with_alpha(t.accent, 0.35);
    w.active.weak_bg_fill = with_alpha(t.accent, 0.25);
    w.active.bg_stroke = Stroke::new(1.0, t.accent);
    w.active.fg_stroke = Stroke::new(1.0, t.text);
    w.active.corner_radius = small;

    w.open = w.hovered;
    v
}

fn text_styles(style: &mut Style, t: &Tokens) {
    let body = t.body_size;
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(body - 2.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(body, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(body, fonts::family(fonts::INTER_MEDIUM)),
        ),
        (
            TextStyle::Monospace,
            FontId::new(t.mono_size, FontFamily::Monospace),
        ),
        (
            TextStyle::Heading,
            FontId::new(body + 7.0, fonts::family(fonts::INTER_SEMIBOLD)),
        ),
        (
            TextStyle::Name(LABEL.into()),
            FontId::new(body, fonts::family(fonts::INTER_MEDIUM)),
        ),
        (
            TextStyle::Name(HERO.into()),
            FontId::new(32.0, fonts::family(fonts::SERIF_ITALIC)),
        ),
    ]
    .into();
}

/// `#RGB`, `#RRGGBB` or `#RRGGBBAA`.
pub fn hex(s: &str) -> Option<Color32> {
    let s = s.trim().strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    match s.len() {
        3 => {
            let nib = |i: usize| u8::from_str_radix(s.get(i..=i)?, 16).ok().map(|n| n * 17);
            Some(Color32::from_rgb(nib(0)?, nib(1)?, nib(2)?))
        }
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            byte(6)?,
        )),
        _ => None,
    }
}

/// The same colour at a different opacity (0..=1).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped
pub fn with_alpha(c: Color32, alpha: f32) -> Color32 {
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// Nudge a colour toward white by `amount` (0..=1); hover states on dark glass.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped
pub fn lift(c: Color32, amount: f32) -> Color32 {
    let k = amount.clamp(0.0, 1.0);
    let ch = |v: u8| (f32::from(v) + (255.0 - f32::from(v)) * k).round() as u8;
    Color32::from_rgba_unmultiplied(ch(c.r()), ch(c.g()), ch(c.b()), c.a())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parses_short_long_and_alpha() {
        assert_eq!(hex("#fff"), Some(Color32::from_rgb(255, 255, 255)));
        assert_eq!(hex("#0B0B10"), Some(Color32::from_rgb(11, 11, 16)));
        assert_eq!(
            hex("#7C8CFF80"),
            Some(Color32::from_rgba_unmultiplied(124, 140, 255, 128))
        );
        assert_eq!(hex("7C8CFF"), None);
        assert_eq!(hex("#zzzzzz"), None);
    }

    #[test]
    fn defaults_resolve_without_a_theme_file() {
        let t = Tokens::from_theme(&Theme::default());
        assert_eq!(t.stage, Color32::from_rgb(0x0B, 0x0B, 0x10));
        assert!(t.bg_image.is_none());
        assert!((t.glass_opacity - 0.85).abs() < f32::EPSILON);
        assert_eq!(t.glass_radius, 12);
    }
}
