//! The crate's only unsafe code: one dwmapi FFI call, alone in its own file
//! so the `#[allow(unsafe_code)]` covers nothing else. `forbid` cannot be used
//! at the crate level because `forbid` cannot be lifted anywhere inside the
//! crate, not even for a module like this one — see `[lints.rust]` in
//! Cargo.toml.
#![cfg(target_os = "windows")]
#![allow(unsafe_code, reason = "one dwmapi FFI call, nothing else in here")]

/// E12: the window is frameless so the UI can draw its own title bar, but it
/// stays opaque — a transparent window costs a per-pixel-alpha composition
/// surface for the whole window and bought nothing but the corner radius.
/// Windows 11 rounds the corners for us; on Windows 10 the attribute is
/// unknown and the call fails harmlessly, leaving square corners.
pub fn round_window_corners(window: &tauri::WebviewWindow) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DWM_WINDOW_CORNER_PREFERENCE,
    };
    let Ok(hwnd) = window.hwnd() else { return };
    let pref: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
    let size = u32::try_from(std::mem::size_of_val(&pref)).unwrap_or(4);
    // SAFETY: `hwnd` is the live window handle Tauri just handed us and
    // `pref` outlives the call, which copies `size` bytes out of it.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&pref).cast(),
            size,
        );
    }
}
