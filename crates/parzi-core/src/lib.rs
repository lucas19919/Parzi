pub mod artifacts;
pub mod brain;
pub mod config;
pub mod connectors;
pub mod context;
pub mod device;
pub mod error;
pub mod hash;
pub mod hooks;
pub mod paths;
pub mod plan;
pub mod store;
pub mod system;
pub mod text;
pub mod theme;
pub mod urls;
pub mod wallpaper;
pub mod widgets;

pub use error::{atomic_write, atomic_write_private, ParziError, Result};

/// Milliseconds since the Unix epoch (0 when the clock is before it).
#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
