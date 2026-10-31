//! Parzi-Setup.exe: one small window, no wizard. It carries the NSIS installer and
//! runs it silently, so installs, upgrades and shortcuts stay in code the in-app
//! updater also uses. Where WebView2 is missing it runs with no window at all, and
//! the installer puts WebView2 on the machine.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

#[cfg(windows)]
mod window;

#[cfg(windows)]
fn main() {
    window::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Parzi Setup only runs on Windows.");
}
