//! parzi-core: types, config, theme, session store, lanes, context, widgets.
//! No network. No GUI. Sync only — async lives in parzi-runtime.

pub mod artifacts;
pub mod capsule;
pub mod config;
pub mod context;
pub mod error;
pub mod journal;
pub mod lanes;
pub mod lease;
pub mod paths;
pub mod plan;
pub mod project;
pub mod prompts;
pub mod rules;
pub mod status;
pub mod store;
pub mod system;
pub mod theme;
pub mod wallpaper;
pub mod widgets;
pub mod workspace;

pub use error::{atomic_write, ParziError, Result};
