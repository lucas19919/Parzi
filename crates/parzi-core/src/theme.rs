use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::{atomic_write, paths};

/// Everything the UI renders derives from this file. `user.css` loads last
/// and wins over generated variables — real CSS stays possible.
///
/// The UI never reads these values directly: `to_css_vars` emits them as
/// `--parzi-*` inputs and `ui/src/theme.css` derives every role token
/// (text ramp, surfaces, lines, accent tints) from those inputs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Theme {
    #[serde(default)]
    pub font: FontTheme,
    #[serde(default)]
    pub colors: ColorTheme,
    #[serde(default)]
    pub background: BackgroundTheme,
    #[serde(default)]
    pub glass: GlassTheme,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontTheme {
    #[serde(default = "d_ui_font")]
    pub family: String,
    #[serde(default = "d_ui_size")]
    pub size: u32,
    #[serde(default = "d_mono_font")]
    pub mono: String,
    #[serde(default = "d_mono_size")]
    pub mono_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorTheme {
    #[serde(default = "c_sidebar")]
    pub sidebar: String,
    #[serde(default = "c_stage")]
    pub stage: String,
    #[serde(default = "c_accent")]
    pub accent: String,
    #[serde(default = "c_text")]
    pub text: String,
    #[serde(default = "c_text_dim")]
    pub text_dim: String,
    #[serde(default = "c_bar")]
    pub bar: String,
    #[serde(default = "c_border")]
    pub border: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTheme {
    /// Relative to ~/.parzi. Default: the Asuka picture shipped with Parzi.
    #[serde(default = "d_bg_image")]
    pub image: String,
    #[serde(default = "d_dim")]
    pub dim: f64,
    #[serde(default = "d_vignette")]
    pub vignette: f64,
    /// Slight photo blur for mood (px). Static layer, no per-frame cost.
    #[serde(default = "d_bg_blur")]
    pub blur: f64,
    /// When true, picking a wallpaper re-derives the accent from its colors.
    #[serde(default)]
    pub auto_accent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlassTheme {
    #[serde(default = "d_opacity")]
    pub opacity: f64,
    #[serde(default = "d_radius")]
    pub radius: u32,
    #[serde(default = "d_blur")]
    pub blur_px: u32,
    #[serde(default = "d_shadow")]
    pub shadow: bool,
}

fn d_ui_font() -> String {
    "Inter".into()
}
fn d_ui_size() -> u32 {
    14
}
fn d_mono_font() -> String {
    "JetBrains Mono".into()
}
fn d_mono_size() -> u32 {
    13
}
fn c_sidebar() -> String {
    "#07070B".into()
}
fn c_stage() -> String {
    "#0B0B10".into()
}
fn c_accent() -> String {
    "#7C8CFF".into()
}
fn c_text() -> String {
    "#EDEDF2".into()
}
fn c_text_dim() -> String {
    "#9AA0AE".into()
}
fn c_bar() -> String {
    "#0E0E14".into()
}
fn c_border() -> String {
    "#20232C".into()
}
fn d_bg_image() -> String {
    "".into()
}
fn d_dim() -> f64 {
    0.66
}
fn d_vignette() -> f64 {
    0.5
}
fn d_bg_blur() -> f64 {
    3.0
}
fn d_opacity() -> f64 {
    0.85
}
fn d_radius() -> u32 {
    12
}
fn d_blur() -> u32 {
    18
}
fn d_shadow() -> bool {
    true
}

impl Default for FontTheme {
    fn default() -> Self {
        Self {
            family: d_ui_font(),
            size: d_ui_size(),
            mono: d_mono_font(),
            mono_size: d_mono_size(),
        }
    }
}
impl Default for ColorTheme {
    fn default() -> Self {
        Self {
            sidebar: c_sidebar(),
            stage: c_stage(),
            accent: c_accent(),
            text: c_text(),
            text_dim: c_text_dim(),
            bar: c_bar(),
            border: c_border(),
        }
    }
}
impl Default for BackgroundTheme {
    fn default() -> Self {
        Self {
            image: d_bg_image(),
            dim: d_dim(),
            vignette: d_vignette(),
            blur: d_bg_blur(),
            auto_accent: false,
        }
    }
}
impl Default for GlassTheme {
    fn default() -> Self {
        Self {
            opacity: d_opacity(),
            radius: d_radius(),
            blur_px: d_blur(),
            shadow: d_shadow(),
        }
    }
}

impl Theme {
    pub fn load() -> Result<Self> {
        let path = paths::theme_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        Ok(toml::from_str(&std::fs::read_to_string(&path)?)?)
    }

    /// Persist with floats clamped and rounded to two decimals so the TOML
    /// stays legible (no `0.6000000238` from float noise).
    pub fn save(&self) -> Result<()> {
        let t = self.normalized();
        atomic_write(&paths::theme_path()?, toml::to_string(&t)?.as_bytes())
    }

    fn normalized(&self) -> Theme {
        let mut t = self.clone();
        t.background.dim = round2(t.background.dim.clamp(0.0, 1.0));
        t.background.vignette = round2(t.background.vignette.clamp(0.0, 1.0));
        t.background.blur = round2(t.background.blur.clamp(0.0, 40.0));
        t.glass.opacity = round2(t.glass.opacity.clamp(0.0, 1.0));
        t.glass.radius = t.glass.radius.min(32);
        t.glass.blur_px = t.glass.blur_px.min(60);
        t.font.size = t.font.size.clamp(10, 20);
        t.font.mono_size = t.font.mono_size.clamp(9, 18);
        t
    }

    /// Emit the CSS inputs the UI consumes. `user.css` overrides these.
    ///
    /// Percent twins (`*-pct`) exist because `color-mix()` wants a
    /// percentage while `rgba()` alpha wants a number; both stay in sync here.
    /// `--parzi-accent-ink` is the text colour that reads on the accent.
    pub fn to_css_vars(&self) -> String {
        let t = self.normalized();
        let c = &t.colors;
        let b = &t.background;
        let g = &t.glass;
        format!(
            ":root{{--parzi-font:{};--parzi-font-size:{}px;--parzi-mono:{};--parzi-mono-size:{}px;\
            --parzi-sidebar:{};--parzi-stage:{};--parzi-bar:{};--parzi-border:{};\
            --parzi-accent:{};--parzi-accent-ink:{};--parzi-text:{};--parzi-text-dim:{};\
            --parzi-bg-dim:{};--parzi-bg-dim-pct:{};--parzi-vignette:{};--parzi-bg-blur:{}px;\
            --parzi-glass-opacity:{};--parzi-glass-opacity-pct:{};--parzi-glass-radius:{}px;\
            --parzi-glass-blur:{}px;--parzi-glass-shadow:{};}}\n",
            css_font_list(&t.font.family),
            t.font.size,
            css_font_list(&t.font.mono),
            t.font.mono_size,
            css_value(&c.sidebar),
            css_value(&c.stage),
            css_value(&c.bar),
            css_value(&c.border),
            css_value(&c.accent),
            accent_ink(&c.accent),
            css_value(&c.text),
            css_value(&c.text_dim),
            num(b.dim),
            pct(b.dim),
            num(b.vignette),
            num(b.blur),
            num(g.opacity),
            pct(g.opacity),
            g.radius,
            g.blur_px,
            if g.shadow { 1 } else { 0 },
        )
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// `0.66` → "0.66", `1.0` → "1", `0.5` → "0.5".
fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

fn pct(v: f64) -> String {
    format!("{}%", (v.clamp(0.0, 1.0) * 100.0).round() as u32)
}

/// Colour strings are the user's own, but keep them from escaping the
/// declaration they live in.
fn css_value(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ';' | '{' | '}' | '\n'))
        .collect()
}

const GENERIC_FAMILIES: &[&str] = &[
    "system-ui",
    "sans-serif",
    "serif",
    "monospace",
    "ui-monospace",
    "ui-sans-serif",
    "ui-serif",
    "ui-rounded",
    "cursive",
    "fantasy",
    "inherit",
];

/// "JetBrains Mono, monospace" → `"JetBrains Mono", monospace`. Family names
/// get quoted, generic keywords stay bare, empty input falls back to inherit.
/// Mirrored in `ui/src/lib/theme.ts` so live previews match the saved CSS.
pub fn css_font_list(s: &str) -> String {
    let parts: Vec<String> = s
        .split(',')
        .map(|p| p.trim().trim_matches('"').trim_matches('\'').trim())
        .filter(|p| !p.is_empty())
        .map(|p| {
            if GENERIC_FAMILIES.contains(&p.to_ascii_lowercase().as_str()) {
                p.to_string()
            } else {
                format!("\"{}\"", p.replace(['"', '\\', ';', '{', '}'], ""))
            }
        })
        .collect();
    if parts.is_empty() {
        "inherit".into()
    } else {
        parts.join(", ")
    }
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if !h.is_ascii() {
        return None;
    }
    let h: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => h[..6].to_string(),
        _ => return None,
    };
    let v = u32::from_str_radix(&h, 16).ok()?;
    Some([(v >> 16) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8])
}

/// Text colour that reads on the accent: near-black on bright accents,
/// white on deep ones (WCAG relative luminance, break-even at 0.179).
pub fn accent_ink(hex: &str) -> &'static str {
    match parse_hex(hex) {
        Some([r, g, b]) => {
            let lin = |c: u8| {
                let c = c as f64 / 255.0;
                if c <= 0.03928 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            let l = 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
            if l > 0.179 {
                "#0B0D12"
            } else {
                "#FFFFFF"
            }
        }
        None => "#0B0D12",
    }
}

// ---------------------------------------------------------------------------
// user.css: loads after the generated variables and wins.
// ---------------------------------------------------------------------------

const USER_CSS_MAX: usize = 64 * 1024;

pub fn read_user_css() -> Result<String> {
    let p = paths::user_css_path()?;
    if p.exists() {
        Ok(std::fs::read_to_string(p)?)
    } else {
        Ok(String::new())
    }
}

/// Empty input removes the file so the theme is the only source again.
pub fn write_user_css(css: &str) -> Result<()> {
    if css.len() > USER_CSS_MAX {
        return Err(ParziError::Config("user.css: 64 KB max".into()));
    }
    let p = paths::user_css_path()?;
    if css.trim().is_empty() {
        if p.exists() {
            std::fs::remove_file(p)?;
        }
        return Ok(());
    }
    atomic_write(&p, css.as_bytes())
}

// ---------------------------------------------------------------------------
// Appearance packs: named, savable, switchable themes.
// `~/.parzi/themes/<pack>/` holds theme.toml + optional art + user.css.
// ---------------------------------------------------------------------------

/// Shipped packs, in display order. They re-seed when missing and can't be
/// deleted from the UI; user packs sort after them.
pub const BUILTIN_PACKS: &[&str] = &["eva-crosses", "midnight", "grey", "light"];

pub fn themes_dir() -> Result<std::path::PathBuf> {
    Ok(paths::parzi_dir()?.join("themes"))
}

fn check_pack_name(name: &str) -> Result<()> {
    if name.trim().is_empty()
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(ParziError::Config(
            "pack name: letters, numbers, _ and - only".into(),
        ));
    }
    Ok(())
}

/// Sorted pack names present on disk.
pub fn list_packs() -> Result<Vec<String>> {
    let root = themes_dir()?;
    let mut out = vec![];
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            if e.path().is_dir() && e.path().join("theme.toml").exists() {
                out.push(e.file_name().to_string_lossy().to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// What the theme picker shows for a pack: real colours from its file, not a
/// hardcoded preview table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInfo {
    pub name: String,
    pub builtin: bool,
    pub colors: ColorTheme,
    /// Ships its own wallpaper (applying it changes the picture).
    pub has_art: bool,
}

fn pack_art(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| {
                    let ext = n.rsplit('.').next().unwrap_or("").to_lowercase();
                    BG_EXTS.contains(&ext.as_str())
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Built-ins first in shipped order, then user packs alphabetically.
/// Unparsable packs are skipped, never fatal.
pub fn list_pack_infos() -> Result<Vec<PackInfo>> {
    let root = themes_dir()?;
    let mut out = vec![];
    for name in list_packs()? {
        let dir = root.join(&name);
        let Ok(raw) = std::fs::read_to_string(dir.join("theme.toml")) else {
            continue;
        };
        let Ok(theme) = toml::from_str::<Theme>(&raw) else {
            continue;
        };
        let has_art = !pack_art(&dir).is_empty() || !theme.background.image.is_empty();
        out.push(PackInfo {
            builtin: BUILTIN_PACKS.contains(&name.as_str()),
            name,
            colors: theme.colors,
            has_art,
        });
    }
    out.sort_by_key(|p| {
        let rank = BUILTIN_PACKS
            .iter()
            .position(|b| *b == p.name)
            .unwrap_or(usize::MAX);
        (!p.builtin, rank, p.name.clone())
    });
    Ok(out)
}

/// Snapshot current theme + background + user.css into a pack. Overwrites.
pub fn save_pack(name: &str) -> Result<()> {
    check_pack_name(name)?;
    let dir = themes_dir()?.join(name);
    std::fs::create_dir_all(&dir)?;
    let theme = Theme::load()?;
    atomic_write(
        &dir.join("theme.toml"),
        toml::to_string(&theme.normalized())?.as_bytes(),
    )?;
    if !theme.background.image.is_empty() {
        let bg_src = paths::parzi_dir()?.join(&theme.background.image);
        if bg_src.exists() {
            if let Some(fname) = bg_src.file_name() {
                std::fs::copy(&bg_src, dir.join(fname))?;
            }
        }
    }
    let css_src = paths::user_css_path()?;
    if css_src.exists() {
        std::fs::copy(&css_src, dir.join("user.css"))?;
    }
    Ok(())
}

/// Remove a user pack. Built-ins stay (they would re-seed anyway).
pub fn delete_pack(name: &str) -> Result<()> {
    check_pack_name(name)?;
    if BUILTIN_PACKS.contains(&name) {
        return Err(ParziError::Config(
            "built-in themes can't be deleted".into(),
        ));
    }
    let dir = themes_dir()?.join(name);
    if !dir.join("theme.toml").exists() {
        return Err(ParziError::Config(format!("no theme named {name}")));
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// Apply a pack: its colours, type and glass become the live theme.
///
/// Wallpaper rule: a pack that ships art (or names an existing saved
/// background) switches the picture; a pack without art keeps the wallpaper
/// you already chose. `auto_accent` is a user preference and survives too.
pub fn apply_pack(name: &str) -> Result<Theme> {
    check_pack_name(name)?;
    let dir = themes_dir()?.join(name);
    let mut theme: Theme = toml::from_str(&std::fs::read_to_string(dir.join("theme.toml"))?)?;
    let current = Theme::load().unwrap_or_default();

    // Copy shipped art next to the live backgrounds (never overwrite).
    let art = pack_art(&dir);
    for fname in &art {
        let dest = paths::backgrounds_dir()?.join(fname);
        if !dest.exists() {
            std::fs::copy(dir.join(fname), &dest)?;
        }
    }
    let named_exists = !theme.background.image.is_empty()
        && paths::parzi_dir()?.join(&theme.background.image).exists();
    if let Some(first) = art.first() {
        theme.background.image = format!("backgrounds/{first}");
    } else if !named_exists {
        theme.background.image = current.background.image.clone();
    }
    theme.background.auto_accent = current.background.auto_accent;

    let css = dir.join("user.css");
    if css.exists() {
        std::fs::copy(&css, paths::user_css_path()?)?;
    }
    theme.save()?;
    Ok(theme)
}

// ---------------------------------------------------------------------------
// Saved background images. Guarded reads (t3code discipline): small regular
// files with image extensions only; anything else is skipped, never fatal.
// ---------------------------------------------------------------------------

const BG_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp"];
const BG_MAX_BYTES: u64 = 20 * 1024 * 1024;

fn is_bg_file(p: &std::path::Path) -> bool {
    let ext = p
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !BG_EXTS.contains(&ext.as_str()) {
        return false;
    }
    match std::fs::symlink_metadata(p) {
        Ok(m) => m.is_file() && m.len() <= BG_MAX_BYTES,
        Err(_) => false,
    }
}

/// Sorted names of saved background images.
pub fn list_backgrounds() -> Result<Vec<String>> {
    let root = paths::backgrounds_dir()?;
    let mut out = vec![];
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            if is_bg_file(&e.path()) {
                out.push(e.file_name().to_string_lossy().to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Point the live theme at a saved background (or "" for solid).
pub fn set_background(name: &str) -> Result<Theme> {
    if !name.is_empty() {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(ParziError::Config("bad background name".into()));
        }
        let p = paths::backgrounds_dir()?.join(name);
        if !is_bg_file(&p) {
            return Err(ParziError::Config(format!("background not found: {name}")));
        }
        // Refuse here, where a person is choosing, rather than at paint time
        // (the renderer would have to decode it to find out).
        crate::wallpaper::check_source(&p)?;
    }
    let mut theme = Theme::load()?;
    theme.background.image = if name.is_empty() {
        String::new()
    } else {
        format!("backgrounds/{name}")
    };
    theme.save()?;
    Ok(theme)
}

/// Copy an outside image into saved backgrounds. Returns its saved name.
pub fn upload_background(src: &str) -> Result<String> {
    let src_p = std::path::PathBuf::from(src);
    if !is_bg_file(&src_p) {
        return Err(ParziError::Config(
            "not a usable image (png/jpg/webp, ≤20MB)".into(),
        ));
    }
    crate::wallpaper::check_source(&src_p)?;
    let name = src_p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "background.png".into());
    let dest = paths::backgrounds_dir()?.join(&name);
    if !dest.exists() {
        std::fs::copy(&src_p, &dest)?;
    }
    Ok(name)
}

// ---------------------------------------------------------------------------
// Wallpaper color sync: derive UI colors from background art so the accent
// follows the mood of the picture. Zero new runtime deps beyond `image`.
// ---------------------------------------------------------------------------

/// Colors derived from wallpaper art.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Palette {
    /// Most vivid tone — the glow accent.
    pub accent: String,
    /// Mean tone — washes and gradients.
    pub average: String,
    /// Darkened average — sidebar/stage pairing.
    pub deep: String,
}

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Sample `path` down to a thumbnail grid and derive UI colors. Near-black
/// buckets are ignored; when the art is near-monochrome the accent falls
/// back to the default indigo instead of a muddy gray.
pub fn extract_palette(path: &std::path::Path) -> Result<Palette> {
    // Decoder limits, not `image::open`'s defaults (AUDIT C-7).
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| ParziError::Config(format!("unreadable image: {e}")))?
        .with_guessed_format()
        .map_err(|e| ParziError::Config(format!("unreadable image: {e}")))?;
    reader.limits(crate::wallpaper::limits());
    let img = reader
        .decode()
        .map_err(|e| ParziError::Config(format!("unreadable image: {e}")))?;
    let thumb = img.thumbnail(64, 64).to_rgb8();
    let pixels: Vec<[u8; 3]> = thumb.pixels().map(|p| p.0).collect();
    if pixels.is_empty() {
        return Err(ParziError::Config("empty image".into()));
    }
    // Mean tone.
    let (mut sr, mut sg, mut sb) = (0u64, 0u64, 0u64);
    // 4-bit buckets: (count, r_sum, g_sum, b_sum).
    #[allow(clippy::type_complexity)]
    let mut buckets: std::collections::HashMap<(u8, u8, u8), (u64, u64, u64, u64)> =
        std::collections::HashMap::new();
    for [r, g, b] in &pixels {
        sr += *r as u64;
        sg += *g as u64;
        sb += *b as u64;
        let key = (r >> 4, g >> 4, b >> 4);
        let e = buckets.entry(key).or_insert((0, 0, 0, 0));
        e.0 += 1;
        e.1 += *r as u64;
        e.2 += *g as u64;
        e.3 += *b as u64;
    }
    let n = pixels.len() as u64;
    let avg = [(sr / n) as u8, (sg / n) as u8, (sb / n) as u8];
    // Most vivid bucket wins, weighted by coverage.
    let mut best: Option<([u8; 3], f32)> = None;
    for (_, (count, r_sum, g_sum, b_sum)) in &buckets {
        let (r, g, b) = (
            (r_sum / count) as f32 / 255.0,
            (g_sum / count) as f32 / 255.0,
            (b_sum / count) as f32 / 255.0,
        );
        let mx = r.max(g).max(b);
        let mn = r.min(g).min(b);
        if mx < 0.08 {
            continue; // near-black reads as mud, never as accent.
        }
        let sat = if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
        let score = *count as f32 * sat * (0.3 + 0.7 * mx);
        let rgb = [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8];
        if best.map(|(_, s)| score > s).unwrap_or(true) {
            best = Some((rgb, score));
        }
    }
    // Saturation guard: monochrome art keeps the default indigo accent.
    let accent = match best {
        Some((rgb, _)) => {
            let (r, g, b) = (rgb[0] as f32, rgb[1] as f32, rgb[2] as f32);
            let mx = r.max(g).max(b);
            let sat = if mx > 0.0 {
                (mx - r.min(g).min(b)) / mx
            } else {
                0.0
            };
            if sat < 0.18 {
                [0x7C, 0x8C, 0xFF]
            } else {
                rgb
            }
        }
        None => [0x7C, 0x8C, 0xFF],
    };
    Ok(Palette {
        accent: hex(accent[0], accent[1], accent[2]),
        average: hex(avg[0], avg[1], avg[2]),
        deep: hex(
            (avg[0] as f32 * 0.35) as u8,
            (avg[1] as f32 * 0.35) as u8,
            (avg[2] as f32 * 0.35) as u8,
        ),
    })
}

#[cfg(test)]
mod css_tests {
    use super::*;

    #[test]
    fn fonts_are_quoted_and_generics_stay_bare() {
        assert_eq!(
            css_font_list("JetBrains Mono, monospace"),
            "\"JetBrains Mono\", monospace"
        );
        assert_eq!(css_font_list("  \"Inter\" "), "\"Inter\"");
        assert_eq!(css_font_list(""), "inherit");
        assert_eq!(css_font_list("Bad;Name{}"), "\"BadName\"");
    }

    #[test]
    fn accent_ink_flips_on_luminance() {
        assert_eq!(accent_ink("#7C8CFF"), "#0B0D12");
        assert_eq!(accent_ink("#fff"), "#0B0D12");
        assert_eq!(accent_ink("#1E1B4B"), "#FFFFFF");
        assert_eq!(accent_ink("junk"), "#0B0D12");
    }

    #[test]
    fn css_vars_carry_percent_twins_and_shadow_flag() {
        let css = Theme::default().to_css_vars();
        assert!(css.contains("--parzi-font:\"Inter\""));
        assert!(css.contains("--parzi-bg-dim:0.66;--parzi-bg-dim-pct:66%"));
        assert!(css.contains("--parzi-glass-opacity:0.85;--parzi-glass-opacity-pct:85%"));
        assert!(css.contains("--parzi-glass-shadow:1"));
        assert!(css.contains("--parzi-accent-ink:#0B0D12"));
        let mut t = Theme::default();
        t.glass.shadow = false;
        t.background.dim = 1.0;
        let css = t.to_css_vars();
        assert!(css.contains("--parzi-glass-shadow:0"));
        assert!(css.contains("--parzi-bg-dim:1;--parzi-bg-dim-pct:100%"));
    }

    #[test]
    fn numbers_print_without_float_noise() {
        assert_eq!(num(0.6000000238418579), "0.6");
        assert_eq!(num(0.0), "0");
        assert_eq!(num(3.0), "3");
        assert_eq!(num(10.5), "10.5");
    }
}

#[cfg(test)]
mod palette_tests {
    use super::extract_palette;

    fn solid_png(path: &std::path::Path, rgb: [u8; 3]) {
        let img = image::RgbImage::from_pixel(16, 16, image::Rgb(rgb));
        img.save(path).unwrap();
    }

    #[test]
    fn vivid_art_yields_vivid_accent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("red.png");
        solid_png(&p, [220, 30, 30]);
        let pal = extract_palette(&p).unwrap();
        assert_eq!(pal.accent, "#DC1E1E");
        assert_eq!(pal.average, "#DC1E1E");
    }

    #[test]
    fn monochrome_art_falls_back_to_indigo() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("gray.png");
        solid_png(&p, [120, 120, 120]);
        let pal = extract_palette(&p).unwrap();
        assert_eq!(pal.accent, "#7C8CFF");
        assert_eq!(pal.average, "#787878");
    }

    #[test]
    fn unreadable_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(extract_palette(&dir.path().join("nope.png")).is_err());
    }
}
