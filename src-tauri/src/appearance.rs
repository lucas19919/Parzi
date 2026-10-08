use parzi_core::theme::Theme;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn get_theme_css() -> Result<String, String> {
    let theme = Theme::load().map_err(|e| e.to_string())?;
    Ok(theme_css(&theme))
}

#[tauri::command]
pub async fn get_theme() -> Result<Theme, String> {
    Theme::load().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_theme(theme: Theme) -> Result<(), String> {
    theme.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reset_theme() -> Result<(), String> {
    Theme::default().save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_pack(name: String) -> Result<(), String> {
    parzi_core::theme::save_pack(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_pack_infos() -> Result<Vec<parzi_core::theme::PackInfo>, String> {
    parzi_core::theme::list_pack_infos().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_pack(name: String) -> Result<(), String> {
    parzi_core::theme::delete_pack(&name).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rename_pack(old: String, new: String) -> Result<(), String> {
    parzi_core::theme::rename_pack(&old, &new).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn apply_pack(name: String) -> Result<String, String> {
    let theme = parzi_core::theme::apply_pack(&name).map_err(|e| e.to_string())?;
    Ok(theme_css(&theme))
}

#[tauri::command]
pub async fn delete_background(name: String) -> Result<String, String> {
    let theme = parzi_core::theme::delete_background(&name).map_err(|e| e.to_string())?;
    Ok(theme_css(&theme))
}

fn checked_background(abs: std::path::PathBuf) -> Option<std::path::PathBuf> {
    let bgs = parzi_core::paths::backgrounds_dir().ok()?;
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

#[tauri::command]
pub async fn background_url(app: AppHandle) -> Result<String, String> {
    let theme = Theme::load().map_err(|e| e.to_string())?;
    if theme.background.image.is_empty() {
        return Ok(String::new());
    }
    let abs = parzi_core::paths::parzi_dir()
        .map_err(|e| e.to_string())?
        .join(&theme.background.image);
    let Some(abs) = checked_background(abs) else {
        return Ok(String::new());
    };
    let (sw, sh, scale) = app
        .available_monitors()
        .ok()
        .and_then(|ms| {
            ms.into_iter()
                .max_by_key(|m| u64::from(m.size().width) * u64::from(m.size().height))
        })
        .map(|m| (m.size().width, m.size().height, m.scale_factor()))
        .unwrap_or((1920, 1080, 1.0));
    let cache = parzi_core::paths::cache_dir().map_err(|e| e.to_string())?;
    let (src, blur) = (abs.clone(), theme.background.blur);
    let rendered = tauri::async_runtime::spawn_blocking(move || {
        parzi_core::wallpaper::prepare(&src, &cache, blur, sw, sh, scale)
    })
    .await
    .map_err(|e| e.to_string())?;
    let out = match rendered {
        Ok(p) => p,
        Err(e) => {
            eprintln!("wallpaper render failed, serving the source: {e}");
            abs
        }
    };
    let _ = app.asset_protocol_scope().allow_file(&out);
    Ok(out.to_string_lossy().to_string())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BackgroundFile {
    name: String,
    url: String,
}

#[tauri::command]
pub async fn list_background_urls() -> Result<Vec<BackgroundFile>, String> {
    let names = parzi_core::theme::list_backgrounds().map_err(|e| e.to_string())?;
    let dir = parzi_core::paths::backgrounds_dir().map_err(|e| e.to_string())?;
    Ok(names
        .into_iter()
        .map(|name| {
            let url = dir.join(&name).to_string_lossy().to_string();
            BackgroundFile { name, url }
        })
        .collect())
}

#[tauri::command]
pub async fn set_background(name: String) -> Result<String, String> {
    let theme = parzi_core::theme::set_background(&name).map_err(|e| e.to_string())?;
    Ok(theme_css(&theme))
}

fn decode_b64(input: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    // Strip a data: prefix if present, then standard decode.
    let clean = input.split(',').next_back()?.trim();
    base64::engine::general_purpose::STANDARD
        .decode(clean)
        .ok()
}

#[tauri::command]
pub async fn save_background_data(name: String, base64_data: String) -> Result<String, String> {
    let bytes = decode_b64(&base64_data).ok_or("invalid base64 image data")?;
    let saved = tauri::async_runtime::spawn_blocking(move || {
        parzi_core::theme::import_background(&name, &bytes)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    if let Err(e) = parzi_core::theme::set_background(&saved) {
        if let Ok(dir) = parzi_core::paths::backgrounds_dir() {
            let _ = std::fs::remove_file(dir.join(&saved));
        }
        return Err(e.to_string());
    }
    Ok(saved)
}

#[tauri::command]
pub async fn palette_from_background() -> Result<parzi_core::theme::Palette, String> {
    let theme = Theme::load().map_err(|e| e.to_string())?;
    if theme.background.image.is_empty() {
        return Err("no wallpaper set".into());
    }
    let path = parzi_core::paths::parzi_dir()
        .map_err(|e| e.to_string())?
        .join(&theme.background.image);
    let path = checked_background(path).ok_or("bad background name")?;
    tauri::async_runtime::spawn_blocking(move || {
        parzi_core::theme::extract_palette(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn theme_css(theme: &Theme) -> String {
    let mut css = theme.to_css_vars();
    if let Ok(user) = parzi_core::theme::read_user_css() {
        if !user.trim().is_empty() {
            css.push_str("\n/* user.css */\n");
            css.push_str(&user);
        }
    }
    css
}
