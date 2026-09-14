use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};

pub const ARTIFACT_VERSION: u64 = 1;
pub const MAX_ARTIFACT_CHARS: usize = 64_000;
pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_ID_LEN: usize = 64;

const ARTIFACT_KINDS: &[&str] = &[
    "code", "markdown", "html", "svg", "json", "csv", "diff", "text",
];

const CODE_LANGS: &[&str] = &[
    "rust",
    "typescript",
    "javascript",
    "python",
    "toml",
    "json",
    "bash",
    "sh",
    "diff",
    "markdown",
    "md",
    "html",
    "css",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactV1 {
    pub artifact: u64,
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub content: String,
    #[serde(default = "default_version")]
    pub version: u32,
}

fn default_kind() -> String {
    "text".into()
}

fn default_version() -> u32 {
    1
}

/// Slug rule shared with lane/task ids: `[a-z0-9-]{1,64}`.
pub fn slugify_id(raw: &str) -> String {
    let slug: String = raw
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        format!("artifact-{}", &uuid::Uuid::new_v4().to_string()[..8])
    } else {
        slug.chars().take(MAX_ID_LEN).collect()
    }
}

pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn validate_artifact(v: &serde_json::Value) -> Result<ArtifactV1> {
    let mut a: ArtifactV1 = serde_json::from_value(v.clone())
        .map_err(|e| ParziError::Validation(format!("bad artifact: {e}")))?;
    if a.artifact != ARTIFACT_VERSION {
        return Err(ParziError::Validation(format!(
            "artifact version {} unsupported (want {ARTIFACT_VERSION})",
            a.artifact
        )));
    }
    if a.id.trim().is_empty() {
        a.id = slugify_id(&a.title);
    } else {
        a.id = slugify_id(&a.id);
    }
    if !is_valid_id(&a.id) {
        return Err(ParziError::Validation(format!(
            "bad artifact id {:?}",
            a.id
        )));
    }
    if !ARTIFACT_KINDS.contains(&a.kind.as_str()) {
        return Err(ParziError::Validation(format!(
            "unknown artifact kind `{}`",
            a.kind
        )));
    }
    if a.title.chars().count() > MAX_TITLE_CHARS {
        return Err(ParziError::Validation(format!(
            "artifact title exceeds {MAX_TITLE_CHARS} chars"
        )));
    }
    if let Some(lang) = &a.language {
        let l = lang.to_lowercase();
        if !CODE_LANGS.contains(&l.as_str()) {
            return Err(ParziError::Validation(format!(
                "unsupported language `{lang}`"
            )));
        }
    }
    if a.content.chars().count() > MAX_ARTIFACT_CHARS {
        return Err(ParziError::Validation(format!(
            "artifact content exceeds {MAX_ARTIFACT_CHARS} chars"
        )));
    }
    if a.content.trim().is_empty() {
        return Err(ParziError::Validation("artifact content is empty".into()));
    }
    if a.version == 0 {
        a.version = 1;
    }
    Ok(a)
}

/// Next version for `id` given existing versions (pure, tested).
pub fn next_version(id: &str, existing: &[ArtifactV1]) -> u32 {
    let slug = slugify_id(id);
    let max = existing
        .iter()
        .filter(|a| a.id == slug)
        .map(|a| a.version)
        .max()
        .unwrap_or(0);
    max + 1
}

/// Content-hash dedup: same id + same content => no new version.
pub fn is_same_content(id: &str, content: &str, existing: &[ArtifactV1]) -> bool {
    let slug = slugify_id(id);
    existing
        .iter()
        .filter(|a| a.id == slug)
        .max_by_key(|a| a.version)
        .is_some_and(|latest| latest.content == content)
}
