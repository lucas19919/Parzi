use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};

const ARTIFACT_VERSION: u64 = 1;
const MAX_ARTIFACT_CHARS: usize = 64_000;
const MAX_TITLE_CHARS: usize = 120;
const MAX_ID_LEN: usize = 64;

const ARTIFACT_KINDS: &[&str] = &[
    "code", "markdown", "html", "svg", "json", "csv", "diff", "text", "image", "preview",
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

fn kind_named(raw: &str) -> String {
    let k = raw.trim().to_ascii_lowercase();
    match k.as_str() {
        "doc" | "document" | "md" | "report" | "note" | "notes" | "readme" => "markdown".into(),
        "txt" | "plain" | "plaintext" => "text".into(),
        "page" | "htm" => "html".into(),
        "patch" => "diff".into(),
        "table" => "csv".into(),
        _ => k,
    }
}

fn default_kind() -> String {
    "text".into()
}

fn default_version() -> u32 {
    1
}

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

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn preview_variants(component: &str) -> Option<&'static [u64]> {
    match component {
        "omnibar" => Some(&[1, 2]),
        _ => None,
    }
}

fn validate_preview(content: &str) -> Result<()> {
    let v: serde_json::Value = serde_json::from_str(content).map_err(|_| {
        ParziError::Validation(
            "preview content must be JSON like {\"component\":\"omnibar\",\"variant\":1}".into(),
        )
    })?;
    let component = v
        .get("component")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .trim();
    let Some(variants) = preview_variants(component) else {
        return Err(ParziError::Validation(
            "unknown preview component. Known: omnibar (variant 1 agent, 2 web)".into(),
        ));
    };
    let variant_ok = v
        .get("variant")
        .and_then(|n| n.as_u64())
        .is_some_and(|n| variants.contains(&n));
    if !variant_ok {
        return Err(ParziError::Validation(
            "omnibar variant must be 1 (agent) or 2 (web)".into(),
        ));
    }
    if let Some(tokens) = v.get("tokens") {
        if !tokens.is_object() {
            return Err(ParziError::Validation(
                "preview tokens must be an object of bg, text, accent, panel as #hex and radius, space as Npx".into(),
            ));
        }
    }
    Ok(())
}

fn mentions_live_ui(title: &str, content: &str) -> bool {
    format!("{title}\n{content}")
        .to_ascii_lowercase()
        .contains("omnibar")
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
    a.kind = kind_named(&a.kind);
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
        let l = lang.trim().to_ascii_lowercase();
        let named = !l.is_empty()
            && l.len() <= 24
            && l.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '-' | '_' | '.'));
        if !named {
            return Err(ParziError::Validation(format!(
                "unsupported language `{lang}`"
            )));
        }
        a.language = Some(l);
    }
    if a.content.chars().count() > MAX_ARTIFACT_CHARS {
        return Err(ParziError::Validation(format!(
            "artifact content exceeds {MAX_ARTIFACT_CHARS} chars"
        )));
    }
    if a.content.trim().is_empty() {
        return Err(ParziError::Validation("artifact content is empty".into()));
    }
    if a.kind == "html" || a.kind == "json" {
        if validate_preview(&a.content).is_ok() {
            a.kind = "preview".into();
        } else if mentions_live_ui(&a.title, &a.content) {
            return Err(ParziError::Validation(
                "Parzi UI renders as kind preview, not html or json. content is {\"component\":\"omnibar\",\"variant\":1} (1 agent, 2 web). tokens restyle the real component: bg, text, accent, panel as #hex, radius and space as Npx.".into(),
            ));
        }
    }
    if a.kind == "preview" {
        validate_preview(&a.content)?;
    }
    if a.version == 0 {
        a.version = 1;
    }
    Ok(a)
}

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

pub fn is_same_content(id: &str, content: &str, existing: &[ArtifactV1]) -> bool {
    let slug = slugify_id(id);
    existing
        .iter()
        .filter(|a| a.id == slug)
        .max_by_key(|a| a.version)
        .is_some_and(|latest| latest.content == content)
}
