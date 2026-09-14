use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParziError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("toml serialize: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("toml parse: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("config: {0}")]
    Config(String),
    #[error("store: {0}")]
    Store(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("provider `{0}`: {1}")]
    Provider(String, String),
    #[error("tool `{0}`: {1}")]
    Tool(String, String),
}

pub type Result<T> = std::result::Result<T, ParziError>;

/// Atomic write: tmp file + rename. Never half-write user data.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
