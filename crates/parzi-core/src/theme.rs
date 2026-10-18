use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};
use crate::{atomic_write, paths};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Theme {
    #[serde(default)]
    pub font: FontTheme,
    #[serde(default)]
    pub colors: ColorTheme,
    #[serde(default)]
    pub background: BackgroundTheme,
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
    #[serde(default = "d_bg_image")]
    pub image: String,
    #[serde(default = "d_dim")]
    pub dim: f64,
    #[serde(default = "d_vignette")]
    pub vignette: f64,
    #[serde(default = "d_bg_blur")]
    pub blur: f64,
    #[serde(default)]
    pub auto_accent: bool,
    /// Dither over the wallpaper: strength (opacity, 0-1) and tile size in px.
    #[serde(default = "d_grain")]
    pub grain: f64,
    #[serde(default = "d_grain_size")]
    pub grain_size: f64,
    /// How far down the window the wallpaper reaches (0.3-1); it fades out there.
    #[serde(default = "d_bg_height")]
    pub height: f64,
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
// The default look: graphite. Neutral near-black, soft white text and a
// plain grey for the quiet parts; no tint, so nothing glows.
fn c_sidebar() -> String {
    "#0F0F0F".into()
}
fn c_stage() -> String {
    "#161616".into()
}
fn c_accent() -> String {
    "#E8E8E8".into()
}
fn c_text() -> String {
    "#ECECEC".into()
}
fn c_text_dim() -> String {
    "#9B9B9B".into()
}
fn c_bar() -> String {
    "#1C1C1C".into()
}
fn c_border() -> String {
    "#2A2A2A".into()
}

/// Earlier defaults, in the order of `ColorTheme`'s fields. A saved theme
/// still exactly one of these was never chosen: it moves to the new one.
const OLD_DEFAULTS: [[&str; 7]; 2] = [
    [
        "#07070B", "#0B0B10", "#E6E8EE", "#EDEDF2", "#9AA0AE", "#0E0E14", "#20232C",
    ],
    // Slate, the default of 0.1.33 to 0.1.36 (still a theme to pick).
    [
        "#0F1217", "#14181E", "#CBD5E1", "#E2E8F0", "#94A3B8", "#1A1F26", "#283039",
    ],
];

impl ColorTheme {
    fn is_old_default(&self) -> bool {
        let mine = [
            &self.sidebar,
            &self.stage,
            &self.accent,
            &self.text,
            &self.text_dim,
            &self.bar,
            &self.border,
        ];
        OLD_DEFAULTS
            .iter()
            .any(|old| mine.iter().zip(old).all(|(a, b)| a.eq_ignore_ascii_case(b)))
    }
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
fn d_grain() -> f64 {
    0.16
}
fn d_grain_size() -> f64 {
    32.0
}
fn d_bg_height() -> f64 {
    1.0
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
            grain: d_grain(),
            grain_size: d_grain_size(),
            height: d_bg_height(),
        }
    }
}

impl Theme {
    pub fn load() -> Result<Self> {
        let path = paths::theme_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut theme: Theme = toml::from_str(&std::fs::read_to_string(&path)?)?;
        if theme.colors.is_old_default() {
            theme.colors = ColorTheme::default();
        }
        Ok(theme)
    }

    pub fn save(&self) -> Result<()> {
        let t = self.normalized();
        atomic_write(&paths::theme_path()?, toml::to_string(&t)?.as_bytes())
    }

    fn normalized(&self) -> Theme {
        let mut t = self.clone();
        t.background.dim = round2(t.background.dim.clamp(0.0, 1.0));
        t.background.vignette = round2(t.background.vignette.clamp(0.0, 1.0));
        t.background.blur = round2(t.background.blur.clamp(0.0, 40.0));
        t.background.grain = round2(t.background.grain.clamp(0.0, 1.0));
        t.background.grain_size = t.background.grain_size.clamp(16.0, 128.0).round();
        t.background.height = round2(t.background.height.clamp(0.3, 1.0));
        t.font.size = t.font.size.clamp(10, 20);
        t.font.mono_size = t.font.mono_size.clamp(9, 18);
        t
    }

    pub fn to_css_vars(&self) -> String {
        let t = self.normalized();
        let c = &t.colors;
        let b = &t.background;
        format!(
            ":root{{--font:{};--font-size:{}px;--mono:{};--mono-size:{}px;\
            --bg:{};--text:{};--muted:{};--accent:{};\
            --bg-dim:{};--vignette:{};--bg-blur:{}px;\
            --grain:{};--grain-size:{}px;--bg-height:{}%;}}\n",
            css_font_list(&t.font.family),
            t.font.size,
            css_font_list(&t.font.mono),
            t.font.mono_size,
            css_value(&c.stage),
            css_value(&c.text),
            css_value(&c.text_dim),
            css_value(&c.accent),
            num(b.dim),
            num(b.vignette),
            num(b.blur),
            num(b.grain),
            num(b.grain_size),
            num(b.height * 100.0),
        )
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

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

fn css_font_list(s: &str) -> String {
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

pub fn read_user_css() -> Result<String> {
    let p = paths::user_css_path()?;
    if p.exists() {
        Ok(std::fs::read_to_string(p)?)
    } else {
        Ok(String::new())
    }
}

/// Position in the builtin list, or None for a user pack.
fn builtin_rank(name: &str) -> Option<usize> {
    paths::BUILTIN_PACKS.iter().position(|(n, _)| *n == name)
}

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

fn list_packs() -> Result<Vec<String>> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInfo {
    pub name: String,
    pub builtin: bool,
    pub colors: ColorTheme,
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
            builtin: builtin_rank(&name).is_some(),
            name,
            colors: theme.colors,
            has_art,
        });
    }
    out.sort_by_key(|p| {
        let rank = builtin_rank(&p.name).unwrap_or(usize::MAX);
        (!p.builtin, rank, p.name.clone())
    });
    Ok(out)
}

/// `abs` if it is a regular file inside backgrounds/: no `..`, no escape
/// through an absolute theme value, no symlink.
#[must_use]
pub fn checked_background(abs: std::path::PathBuf) -> Option<std::path::PathBuf> {
    let bgs = paths::backgrounds_dir().ok()?;
    if abs
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
        || !abs.starts_with(&bgs)
    {
        return None;
    }
    std::fs::symlink_metadata(&abs)
        .is_ok_and(|m| m.is_file())
        .then_some(abs)
}

/// The file a theme's `background.image` names, when it is a safe one.
#[must_use]
pub fn background_file(image: &str) -> Option<std::path::PathBuf> {
    if image.trim().is_empty() {
        return None;
    }
    checked_background(paths::parzi_dir().ok()?.join(image))
}

/// Two files with the same bytes.
fn same_content(a: &std::path::Path, b: &std::path::Path) -> bool {
    let same_len = std::fs::metadata(a)
        .ok()
        .zip(std::fs::metadata(b).ok())
        .is_some_and(|(x, y)| x.is_file() && x.len() == y.len());
    same_len
        && std::fs::read(a)
            .ok()
            .zip(std::fs::read(b).ok())
            .is_some_and(|(x, y)| x == y)
}

pub fn save_pack(name: &str) -> Result<()> {
    check_pack_name(name)?;
    if builtin_rank(name).is_some() {
        return Err(ParziError::Config(
            "built-in themes can't be overwritten; save under a new name".into(),
        ));
    }
    let dir = themes_dir()?.join(name);
    std::fs::create_dir_all(&dir)?;
    let theme = Theme::load()?;
    atomic_write(
        &dir.join("theme.toml"),
        toml::to_string(&theme.normalized())?.as_bytes(),
    )?;
    if let Some(bg_src) = background_file(&theme.background.image) {
        if let Some(fname) = bg_src.file_name() {
            std::fs::copy(&bg_src, dir.join(fname))?;
        }
    }
    let css_src = paths::user_css_path()?;
    if css_src.exists() {
        std::fs::copy(&css_src, dir.join("user.css"))?;
    }
    Ok(())
}

pub fn rename_pack(old: &str, new: &str) -> Result<()> {
    check_pack_name(old)?;
    check_pack_name(new)?;
    if builtin_rank(old).is_some() {
        return Err(ParziError::Config(
            "built-in themes can't be renamed".into(),
        ));
    }
    let root = themes_dir()?;
    let src = root.join(old);
    if !src.join("theme.toml").exists() {
        return Err(ParziError::Config(format!("no theme named {old}")));
    }
    let dest = root.join(new);
    if dest.exists() {
        return Err(ParziError::Config(format!(
            "a theme named {new} already exists"
        )));
    }
    std::fs::rename(&src, &dest).map_err(ParziError::Io)?;
    Ok(())
}

pub fn delete_pack(name: &str) -> Result<()> {
    check_pack_name(name)?;
    if builtin_rank(name).is_some() {
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

pub fn apply_pack(name: &str) -> Result<Theme> {
    check_pack_name(name)?;
    let dir = themes_dir()?.join(name);
    let mut theme: Theme = toml::from_str(&std::fs::read_to_string(dir.join("theme.toml"))?)?;
    let current = Theme::load().unwrap_or_default();

    // Copies carry the pack's name, so two packs shipping `wall.jpg` never
    // show each other's picture; a changed file is copied again.
    let bgs = paths::backgrounds_dir()?;
    let mut art = vec![];
    for fname in pack_art(&dir) {
        let src = dir.join(&fname);
        if !is_bg_file(&src) {
            continue;
        }
        // A pack saved here carries the wallpaper it was saved with: when
        // that very picture is still in backgrounds/, use it, not a copy.
        if same_content(&bgs.join(&fname), &src) {
            art.push(fname);
            continue;
        }
        let local = if fname.starts_with(&format!("{name}-")) {
            fname
        } else {
            format!("{name}-{fname}")
        };
        let dest = bgs.join(&local);
        let same = std::fs::metadata(&dest)
            .ok()
            .zip(std::fs::metadata(&src).ok())
            .is_some_and(|(d, s)| d.len() == s.len());
        if !same {
            std::fs::create_dir_all(&bgs)?;
            std::fs::copy(&src, &dest)?;
        }
        art.push(local);
    }
    let named_exists = background_file(&theme.background.image).is_some();
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

/// A background is named by one plain file name with an image extension:
/// no separators, no drive prefix (`C:x` would leave backgrounds/), no `..`.
fn check_bg_name(name: &str) -> Result<()> {
    let p = std::path::Path::new(name);
    let mut parts = p.components();
    let plain = matches!(
        (parts.next(), parts.next()),
        (Some(std::path::Component::Normal(n)), None) if n == p.as_os_str()
    );
    let ext = p
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !plain || name.contains([':', '/', '\\']) || !BG_EXTS.contains(&ext.as_str()) {
        return Err(ParziError::Config("bad background name".into()));
    }
    Ok(())
}

pub fn delete_background(name: &str) -> Result<Theme> {
    check_bg_name(name)?;
    let p = paths::backgrounds_dir()?.join(name);
    if p.exists() {
        std::fs::remove_file(&p).map_err(ParziError::Io)?;
    }
    let mut theme = Theme::load()?;
    if theme.background.image == format!("backgrounds/{name}") {
        theme.background.image = String::new();
        theme.save()?;
    }
    Ok(theme)
}

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

pub fn set_background(name: &str) -> Result<Theme> {
    if !name.is_empty() {
        check_bg_name(name)?;
        let p = paths::backgrounds_dir()?.join(name);
        if !is_bg_file(&p) {
            return Err(ParziError::Config(format!("background not found: {name}")));
        }
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

pub fn import_background(name: &str, bytes: &[u8]) -> Result<String> {
    let path = std::path::Path::new(name);
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_lowercase();
    let stem: String = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .collect();
    let stem = if stem.is_empty() {
        "wallpaper".to_string()
    } else {
        stem
    };

    let (w, h) = crate::wallpaper::bytes_size(bytes)?;
    let too_big = w.max(h) > crate::wallpaper::MAX_SOURCE_EDGE || bytes.len() as u64 > BG_MAX_BYTES;
    let (data, ext) = if too_big || !BG_EXTS.contains(&ext.as_str()) {
        (
            crate::wallpaper::shrink_for_import(bytes)?,
            "jpg".to_string(),
        )
    } else {
        (bytes.to_vec(), ext)
    };
    if data.len() as u64 > BG_MAX_BYTES {
        return Err(ParziError::Config(
            "that picture is still over 20 MB after shrinking".into(),
        ));
    }

    let dir = paths::backgrounds_dir()?;
    std::fs::create_dir_all(&dir)?;
    let mut file = format!("{stem}.{ext}");
    for n in 2.. {
        if !dir.join(&file).exists() {
            break;
        }
        file = format!("{stem}-{n}.{ext}");
    }
    std::fs::write(dir.join(&file), data)?;
    Ok(file)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Palette {
    pub accent: String,
    pub average: String,
    pub deep: String,
}

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

pub fn extract_palette(path: &std::path::Path) -> Result<Palette> {
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
    let (mut sr, mut sg, mut sb) = (0u64, 0u64, 0u64);
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
            continue;
        }
        let sat = if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
        let score = *count as f32 * sat * (0.3 + 0.7 * mx);
        let rgb = [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8];
        if best.map(|(_, s)| score > s).unwrap_or(true) {
            best = Some((rgb, score));
        }
    }
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
                [0xE8, 0xE8, 0xE8]
            } else {
                rgb
            }
        }
        None => [0xE8, 0xE8, 0xE8],
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
    fn css_vars_carry_only_what_the_ui_reads() {
        let css = Theme::default().to_css_vars();
        assert!(css.contains("--font:\"Inter\""));
        assert!(css.contains("--bg-dim:0.66"));
        assert!(css.contains("--accent:#E8E8E8"));
        assert!(!css.contains("glass"));
        assert!(!css.contains("--parzi-"));
        let mut t = Theme::default();
        t.background.dim = 1.0;
        assert!(t.to_css_vars().contains("--bg-dim:1;"));
    }

    #[test]
    fn grain_and_wallpaper_height_are_clamped_into_css() {
        let css = Theme::default().to_css_vars();
        assert!(css.contains("--grain:0.16;--grain-size:32px;--bg-height:100%;"));
        let mut t = Theme::default();
        t.background.grain = 3.0;
        t.background.grain_size = 4.0;
        t.background.height = 0.7;
        assert!(t
            .to_css_vars()
            .contains("--grain:1;--grain-size:16px;--bg-height:70%;"));
        // Theme files from before these keys still load, with the defaults.
        let old: Theme = toml::from_str("[background]\nimage = \"\"\ndim = 0.5\n").unwrap();
        let b = old.background;
        assert_eq!((b.grain, b.grain_size, b.height), (0.16, 32.0, 1.0));
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
mod background_name_tests {
    use super::check_bg_name;

    #[test]
    fn only_one_plain_image_name_is_a_background() {
        for ok in ["pic.png", "Dusk-2.JPG", "a b.webp", "x.jpeg"] {
            assert!(check_bg_name(ok).is_ok(), "{ok:?}");
        }
        for bad in [
            "",
            " ",
            "C:x",
            "C:x.png",
            "C:\\x.png",
            "/abs.png",
            "../x.png",
            "..",
            "a/b.png",
            "a\\b.png",
            "notes.txt",
            "noext",
            ".",
            "x.png/",
        ] {
            assert!(check_bg_name(bad).is_err(), "{bad:?}");
        }
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
    fn monochrome_art_falls_back_to_the_neutral_accent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("gray.png");
        solid_png(&p, [120, 120, 120]);
        let pal = extract_palette(&p).unwrap();
        assert_eq!(pal.accent, "#E8E8E8");
        assert_eq!(pal.average, "#787878");
    }

    #[test]
    fn the_untouched_old_default_moves_to_slate_and_a_chosen_look_stays() {
        let mut old = super::ColorTheme::default();
        for (field, hex) in [
            (&mut old.sidebar, "#07070b"),
            (&mut old.stage, "#0B0B10"),
            (&mut old.accent, "#E6E8EE"),
            (&mut old.text, "#EDEDF2"),
            (&mut old.text_dim, "#9AA0AE"),
            (&mut old.bar, "#0E0E14"),
            (&mut old.border, "#20232C"),
        ] {
            *field = hex.to_string();
        }
        assert!(old.is_old_default());
        old.accent = "#7AA2F7".into();
        assert!(!old.is_old_default(), "a changed accent is a choice");
        assert!(!super::ColorTheme::default().is_old_default());
        // Slate, the default before graphite, moves on too.
        let slate = super::ColorTheme {
            sidebar: "#0F1217".into(),
            stage: "#14181E".into(),
            accent: "#CBD5E1".into(),
            text: "#E2E8F0".into(),
            text_dim: "#94A3B8".into(),
            bar: "#1A1F26".into(),
            border: "#283039".into(),
        };
        assert!(slate.is_old_default());
    }

    #[test]
    fn unreadable_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(extract_palette(&dir.path().join("nope.png")).is_err());
    }
}
