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
    "#E6E8EE".into()
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

impl Theme {
    pub fn load() -> Result<Self> {
        let path = paths::theme_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        Ok(toml::from_str(&std::fs::read_to_string(&path)?)?)
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
            --bg-dim:{};--vignette:{};--bg-blur:{}px;}}\n",
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

const BUILTIN_PACKS: &[&str] = &["ember", "midnight", "grey", "light"];

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

pub fn rename_pack(old: &str, new: &str) -> Result<()> {
    check_pack_name(old)?;
    check_pack_name(new)?;
    if BUILTIN_PACKS.contains(&old) {
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

pub fn apply_pack(name: &str) -> Result<Theme> {
    check_pack_name(name)?;
    let dir = themes_dir()?.join(name);
    let mut theme: Theme = toml::from_str(&std::fs::read_to_string(dir.join("theme.toml"))?)?;
    let current = Theme::load().unwrap_or_default();

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

pub fn delete_background(name: &str) -> Result<Theme> {
    if name.trim().is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(ParziError::Config("bad background name".into()));
    }
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
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(ParziError::Config("bad background name".into()));
        }
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
                [0xE6, 0xE8, 0xEE]
            } else {
                rgb
            }
        }
        None => [0xE6, 0xE8, 0xEE],
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
        assert!(css.contains("--accent:#E6E8EE"));
        assert!(!css.contains("glass"));
        assert!(!css.contains("--parzi-"));
        let mut t = Theme::default();
        t.background.dim = 1.0;
        assert!(t.to_css_vars().contains("--bg-dim:1;"));
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
        assert_eq!(pal.accent, "#E6E8EE");
        assert_eq!(pal.average, "#787878");
    }

    #[test]
    fn unreadable_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(extract_palette(&dir.path().join("nope.png")).is_err());
    }
}
