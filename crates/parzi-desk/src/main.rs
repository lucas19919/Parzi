//! Parzi native desktop UI on egui/eframe.
//!
//! Milestone 0 (`docs/native-ui/PLAN.md` §5): a frameless, transparent window
//! with its own title bar, three translucent panels over a pre-blurred
//! wallpaper, bundled hinted fonts, `theme.toml` mapped onto `Visuals`, and
//! persisted window state. No runtime, no transcript yet.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod bridge;
mod chrome;
mod composer;
mod fonts;
mod inspector;
mod settings;
mod theme;
mod transcript;
mod wallpaper;
mod widgets;

use std::time::Instant;

fn main() -> eframe::Result {
    let started = Instant::now();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("parzi_desk=info")),
        )
        .init();

    // Window geometry lives next to the rest of Parzi state, not in %APPDATA%.
    let persistence_path = parzi_core::paths::ensure_dirs()
        .map(|home| home.join("desk-window.ron"))
        .map_err(|e| tracing::warn!(error = %e, "window state will not persist"))
        .ok();

    // wgpu (DX12/Vulkan) by default; glow (OpenGL) is the fallback for
    // machines where wgpu misbehaves, and the cheaper option on iGPUs.
    let renderer = match std::env::var("PARZI_DESK_RENDERER").as_deref() {
        Ok("glow") => eframe::Renderer::Glow,
        _ => eframe::Renderer::Wgpu,
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Parzi")
        .with_app_id("parzi-desk")
        .with_decorations(false)
        .with_transparent(true)
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([960.0, 640.0]);
    // Taskbar / alt-tab icon. Missing or undecodable art falls back to the
    // OS default instead of failing the start-up.
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        renderer,
        viewport,
        centered: true,
        persist_window: true,
        persistence_path,
        ..Default::default()
    };

    eframe::run_native(
        "Parzi",
        options,
        Box::new(move |cc| Ok(Box::new(app::DeskApp::new(cc, started)))),
    )
}

/// Runtime window icon decoded from `src-tauri/icons/icon.png` (512x512).
/// `None` when the bundled art cannot be decoded; callers keep the OS
/// default icon in that case.
fn load_icon() -> Option<std::sync::Arc<egui::IconData>> {
    let bytes = include_bytes!("../../../src-tauri/icons/icon.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(std::sync::Arc::new(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    }))
}
