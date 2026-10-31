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
    #[serde(default)]
    pub panel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTheme {
    #[serde(default = "d_bg_image")]
    pub image: String,
    #[serde(default = "d_bg_height")]
    pub height: f64,
    #[serde(default = "d_bg_y")]
    pub y: f64,
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

const OLD_DEFAULTS: [[&str; 7]; 2] = [
    [
        "#07070B", "#0B0B10", "#E6E8EE", "#EDEDF2", "#9AA0AE", "#0E0E14", "#20232C",
    ],
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
fn d_bg_height() -> f64 {
    0.34
}
fn d_bg_y() -> f64 {
    0.5
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
            panel: String::new(),
        }
    }
}
impl Default for BackgroundTheme {
    fn default() -> Self {
        Self {
            image: d_bg_image(),
            height: d_bg_height(),
            y: d_bg_y(),
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
        t.colors.panel = t.colors.panel.trim().to_string();
        t.background.height = t.background.height.clamp(0.2, 0.6);
        t.background.y = t.background.y.clamp(0.0, 1.0);
        t.font.size = t.font.size.clamp(10, 20);
        t.font.mono_size = t.font.mono_size.clamp(9, 18);
        t
    }

    pub fn to_css_vars(&self) -> String {
        let t = self.normalized();
        let c = &t.colors;
        let panel = if c.panel.trim().is_empty() {
            "color-mix(in srgb, var(--text) 6%, var(--bg))".to_string()
        } else {
            css_value(&c.panel)
        };
        format!(
            ":root{{--font:{};--font-size:{}px;--mono:{};--mono-size:{}px;\
            --bg:{};--text:{};--accent:{};--panel:{};--bg-height:{}%;--bg-y:{}%;}}\n",
            css_font_list(&t.font.family),
            t.font.size,
            css_font_list(&t.font.mono),
            t.font.mono_size,
            css_value(&c.stage),
            css_value(&c.text),
            css_value(&c.accent),
            panel,
            (t.background.height * 100.0).round(),
            (t.background.y * 100.0).round(),
        )
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

#[must_use]
pub fn background_file(image: &str) -> Option<std::path::PathBuf> {
    if image.trim().is_empty() {
        return None;
    }
    checked_background(paths::parzi_dir().ok()?.join(image))
}

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

    let bgs = paths::backgrounds_dir()?;
    let mut art = vec![];
    for fname in pack_art(&dir) {
        let src = dir.join(&fname);
        if !is_bg_file(&src) {
            continue;
        }
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
        assert!(css.contains("--accent:#E8E8E8"));
        assert!(css.contains("--panel:color-mix(in srgb, var(--text) 6%, var(--bg));"));
        assert!(css.contains("--bg-height:34%;"));
        assert!(css.contains("--bg-y:50%;"));
        assert!(!css.contains("--muted:"));
        assert!(!css.contains("--bg-dim:"));
        assert!(!css.contains("--glass"));
        assert!(!css.contains("--parzi-"));
    }

    fn channel(hex: &str) -> (f64, f64, f64) {
        let h = hex.trim().trim_start_matches('#');
        let n = u32::from_str_radix(h, 16).unwrap();
        (
            ((n >> 16) & 0xff) as f64,
            ((n >> 8) & 0xff) as f64,
            (n & 0xff) as f64,
        )
    }

    fn mix(a: (f64, f64, f64), b: (f64, f64, f64), t: f64) -> (f64, f64, f64) {
        (
            b.0 + (a.0 - b.0) * t,
            b.1 + (a.1 - b.1) * t,
            b.2 + (a.2 - b.2) * t,
        )
    }

    fn lin(c: f64) -> f64 {
        let s = c / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }

    fn contrast(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
        let lum = |c: (f64, f64, f64)| 0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2);
        let (x, y) = (lum(a), lum(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn derived_muted_clears_body_text_on_the_default_theme() {
        let text = channel("#ECECEC");
        let bg = channel("#161616");
        let muted = mix(text, bg, 0.78);
        let panel = mix(text, bg, 0.06);
        assert!(contrast(muted, bg) >= 4.5, "{}", contrast(muted, bg));
        assert!(contrast(muted, panel) >= 4.5, "{}", contrast(muted, panel));
    }

    #[test]
    fn a_chosen_panel_is_emitted_trimmed() {
        let mut t = Theme::default();
        t.colors.panel = " #1C1C1E ".into();
        let css = t.to_css_vars();
        assert!(css.contains("--panel:#1C1C1E;"), "{css}");
        let old: Theme =
            toml::from_str("[background]\nimage = \"backgrounds/a.png\"\ndim = 0.5\n").unwrap();
        assert_eq!(old.background.image, "backgrounds/a.png");
        assert!(old.colors.panel.is_empty());
        let low: Theme = toml::from_str("[background]\nimage = \"\"\ny = 2.0\n").unwrap();
        assert!(low.to_css_vars().contains("--bg-y:100%;"));
        let tall: Theme = toml::from_str("[background]\nimage = \"\"\nheight = 1.0\n").unwrap();
        assert!(tall.to_css_vars().contains("--bg-height:60%;"));
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
mod default_migration_tests {
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
        let slate = super::ColorTheme {
            sidebar: "#0F1217".into(),
            stage: "#14181E".into(),
            accent: "#CBD5E1".into(),
            text: "#E2E8F0".into(),
            text_dim: "#94A3B8".into(),
            bar: "#1A1F26".into(),
            border: "#283039".into(),
            panel: String::new(),
        };
        assert!(slate.is_old_default());
    }
}
