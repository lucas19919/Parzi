//! Windows executable metadata for `parzi-desk`.
//!
//! Embeds the official Parzi icon (`src-tauri/icons/icon.ico`) as the EXE
//! icon via `winres`. No-op on non-Windows targets and whenever the icon
//! file is missing, so `cargo check` stays green on every host.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_owned());
    let base = std::path::PathBuf::from(&manifest);
    // Primary candidate is the repo layout (`crates/parzi-desk` ->
    // `<root>/src-tauri/icons/icon.ico`); the second covers a moved crate.
    let candidates = [
        base.join("../../src-tauri/icons/icon.ico"),
        base.join("src-tauri/icons/icon.ico"),
    ];
    let Some(icon) = candidates.iter().find(|p| p.is_file()) else {
        println!("cargo:warning=parzi-desk: icon.ico not found, skipping winres icon");
        return;
    };
    println!("cargo:rerun-if-changed={}", icon.display());
    let mut res = winres::WindowsResource::new();
    res.set_icon(&icon.to_string_lossy());
    if let Err(e) = res.compile() {
        // Never fail the build from here; the EXE icon is cosmetic.
        println!("cargo:warning=parzi-desk: winres failed ({e}), continuing without icon");
    }
}
