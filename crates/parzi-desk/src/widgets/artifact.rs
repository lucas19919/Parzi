//! Native `ArtifactV1` renderer: header card (id, kind, version, lines)
//! plus a clamped preview with an expander. Mirrors `ArtifactCard.svelte`:
//! empty content is a validation failure and renders as an error card.

use parzi_core::artifacts::ArtifactV1;

#[cfg(test)]
use parzi_core::artifacts::validate_artifact;

/// Preview lines before the expander appears (matches the web UI's 30).
pub const PREVIEW_LINES: usize = 30;
/// Preview bytes cap: the validator allows 64k chars; the card shows less.
pub const PREVIEW_CHARS: usize = 8_000;

/// A validated artifact ready to render, or the validation failure.
#[derive(Debug, Clone)]
pub enum ArtifactView {
    Valid {
        artifact: ArtifactV1,
        expanded: bool,
    },
    Invalid {
        error: String,
    },
}

impl ArtifactView {
    /// Build from a validation outcome. Empty content, unknown kinds and
    /// bad ids become `Invalid`: an error card, never a panic.
    #[must_use]
    pub fn from_result(result: parzi_core::Result<ArtifactV1>) -> Self {
        match result {
            Err(e) => Self::Invalid {
                error: e.to_string(),
            },
            Ok(a) => Self::Valid {
                artifact: a,
                expanded: false,
            },
        }
    }

    #[must_use]
    pub const fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid { .. })
    }

    /// Flip the preview expander (no-op on `Invalid`).
    pub fn toggle_expanded(&mut self) {
        if let Self::Valid { expanded, .. } = self {
            *expanded = !*expanded;
        }
    }

    /// Line count of the content (0 when invalid).
    #[must_use]
    pub fn line_count(&self) -> usize {
        match self {
            Self::Invalid { .. } => 0,
            Self::Valid { artifact, .. } => {
                if artifact.content.is_empty() {
                    0
                } else {
                    artifact.content.lines().count()
                }
            }
        }
    }

    /// Save-as extension from kind + language (same map as the web UI).
    #[must_use]
    pub fn ext(&self) -> &str {
        match self {
            Self::Invalid { .. } => "txt",
            Self::Valid { artifact, .. } => match artifact.kind.as_str() {
                "markdown" => "md",
                "svg" | "html" | "diff" | "json" | "csv" => artifact.kind.as_str(),
                "code" => artifact
                    .language
                    .as_deref()
                    .filter(|l| !l.is_empty())
                    .unwrap_or("txt"),
                _ => "txt",
            },
        }
    }

    /// Body text: the full content when short/expanded, else the first
    /// [`PREVIEW_LINES`] lines.
    #[must_use]
    pub fn preview(&self) -> String {
        match self {
            Self::Invalid { .. } => String::new(),
            Self::Valid { artifact, expanded } => {
                if *expanded || artifact.content.lines().count() <= PREVIEW_LINES {
                    return artifact.content.chars().take(PREVIEW_CHARS).collect();
                }
                artifact
                    .content
                    .lines()
                    .take(PREVIEW_LINES)
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    }

    /// True when the expander button should show.
    #[must_use]
    pub fn is_clamped(&self) -> bool {
        match self {
            Self::Invalid { .. } => false,
            Self::Valid { artifact, expanded } => {
                !expanded && artifact.content.lines().count() > PREVIEW_LINES
            }
        }
    }
}

/// Thin card: header meta, clamped body, expander.
pub fn show(ui: &mut egui::Ui, view: &mut ArtifactView) {
    let (title, kind, version, lines, ext) = match view {
        ArtifactView::Invalid { error } => {
            super::error_card(ui, "Bad artifact", error);
            return;
        }
        ArtifactView::Valid { artifact, .. } => {
            let lines = if artifact.content.is_empty() {
                0
            } else {
                artifact.content.lines().count()
            };
            (
                artifact.title.clone(),
                artifact.kind.clone(),
                artifact.version,
                lines,
                ext_of(artifact),
            )
        }
    };
    super::framed(ui, &title, |ui| {
        ui.horizontal(|ui| {
            ui.label(kind);
            ui.label(format!("v{version}"));
            ui.label(format!("{lines} lines - .{ext}"));
        });
        let body = view.preview();
        let mono =
            matches!(view, ArtifactView::Valid { artifact, .. } if artifact.kind != "markdown");
        let text = egui::RichText::new(body);
        let text = if mono {
            text.monospace().small()
        } else {
            text.small()
        };
        ui.add(egui::Label::new(text).selectable(true).wrap());
        if view.is_clamped() {
            if ui
                .small_button(format!("show full ({lines} lines)"))
                .clicked()
            {
                view.toggle_expanded();
            }
        } else if lines > PREVIEW_LINES && ui.small_button("collapse").clicked() {
            view.toggle_expanded();
        }
    });
}

/// Save-as extension for a validated artifact (shared by `ext` and `show`).
fn ext_of(artifact: &ArtifactV1) -> String {
    match artifact.kind.as_str() {
        "markdown" => "md".to_string(),
        "svg" | "html" | "diff" | "json" | "csv" => artifact.kind.clone(),
        "code" => artifact
            .language
            .clone()
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "txt".to_string()),
        _ => "txt".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact_of(kind: &str, content: &str) -> ArtifactV1 {
        ArtifactV1 {
            artifact: 1,
            id: "demo".to_string(),
            title: "Demo".to_string(),
            kind: kind.to_string(),
            language: None,
            content: content.to_string(),
            version: 1,
        }
    }

    #[test]
    fn malformed_artifacts_become_invalid() {
        // Genuine validator failure: null is not an artifact.
        let err = validate_artifact(&Default::default()).expect_err("null rejected");
        let view = ArtifactView::from_result(Err(err));
        assert!(view.is_invalid());
        assert_eq!(view.line_count(), 0);
        assert_eq!(view.preview(), "");
        // A degenerate-but-constructed struct still never panics the renderer.
        let empty = ArtifactView::Valid {
            artifact: artifact_of("text", ""),
            expanded: false,
        };
        assert_eq!(empty.line_count(), 0);
        assert_eq!(empty.preview(), "");
    }

    #[test]
    fn extensions_follow_kind_and_language() {
        let md = ArtifactView::from_result(Ok(artifact_of("markdown", "x")));
        assert_eq!(md.ext(), "md");
        let mut a = artifact_of("code", "x");
        a.language = Some("rust".to_string());
        assert_eq!(ArtifactView::from_result(Ok(a.clone())).ext(), "rust");
        a.language = None;
        assert_eq!(ArtifactView::from_result(Ok(a.clone())).ext(), "txt");
        a.kind = "svg".to_string();
        assert_eq!(ArtifactView::from_result(Ok(a)).ext(), "svg");
    }

    #[test]
    fn preview_clamps_long_content() {
        let content: String = (1..=50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut view = ArtifactView::from_result(Ok(artifact_of("code", &content)));
        assert_eq!(view.line_count(), 50);
        assert!(view.is_clamped());
        assert_eq!(view.preview().lines().count(), PREVIEW_LINES);
        view.toggle_expanded();
        assert!(!view.is_clamped());
        let short = ArtifactView::from_result(Ok(artifact_of("markdown", "hi")));
        assert!(!short.is_clamped());
        assert_eq!(short.preview(), "hi");
    }
}
