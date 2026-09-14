//! parzi-core: types, config, theme, session store, lanes, context, widgets.
//! No network. No GUI. Sync only — async lives in parzi-runtime.

pub mod artifacts;
pub mod config;
pub mod context;
pub mod error;
pub mod lanes;
pub mod paths;
pub mod store;
pub mod theme;
pub mod widgets;

pub use error::{atomic_write, ParziError, Result};
