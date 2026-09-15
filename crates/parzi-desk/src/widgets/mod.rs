//! Native WidgetV1 / DiagramV1 / ArtifactV1 renderers.
//!
//! Every renderer consumes its core validated struct. Malformed payloads
//! become an [`error_card`], never a panic: entry points take
//! `parzi_core::Result<..>` so the validation failure is the input.

pub mod artifact;
pub mod diagram;
pub mod widget;

pub use artifact::ArtifactView;
pub use diagram::DiagramView;
pub use widget::{ChartKind, WidgetView};

/// One card that went wrong: title plus the validation message, in the
/// same frame chrome as a healthy card.
pub fn error_card(ui: &mut egui::Ui, title: &str, detail: &str) {
    framed(ui, title, |ui| {
        ui.label("Could not render this card: showing the error instead of raw source.");
        ui.monospace(detail);
    });
}

/// Shared card chrome: title row plus the body. Used by every renderer so
/// healthy and failed cards share one frame.
pub(crate) fn framed(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().window_stroke)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.strong(if title.trim().is_empty() {
                "untitled"
            } else {
                title
            });
            ui.separator();
            body(ui);
        });
}

/// Any renderable card with its validation outcome attached.
#[derive(Debug, Clone)]
pub enum AnyView {
    Widget(WidgetView),
    Diagram(DiagramView),
    Artifact(ArtifactView),
}

impl AnyView {
    /// Dispatch to the matching renderer (error cards included).
    pub fn show(&mut self, ui: &mut egui::Ui) {
        match self {
            Self::Widget(v) => widget::show(ui, v),
            Self::Diagram(v) => diagram::show(ui, v),
            Self::Artifact(v) => artifact::show(ui, v),
        }
    }

    /// True when this card is a validation failure.
    #[must_use]
    pub const fn is_invalid(&self) -> bool {
        match self {
            Self::Widget(v) => v.is_invalid(),
            Self::Diagram(v) => v.is_invalid(),
            Self::Artifact(v) => v.is_invalid(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_variants_report_invalid() {
        let w = AnyView::Widget(WidgetView::from_result(Err(
            parzi_core::ParziError::Validation("bad widget".into()),
        )));
        let d = AnyView::Diagram(DiagramView::from_result(Err(
            parzi_core::ParziError::Validation("bad diagram".into()),
        )));
        let a = AnyView::Artifact(ArtifactView::from_result(Err(
            parzi_core::ParziError::Validation("bad artifact".into()),
        )));
        assert!(w.is_invalid() && d.is_invalid() && a.is_invalid());
    }
}
