//! Native `DiagramV1` renderer: layered node-link drawing from core's
//! validated structs. Layout is pure (BFS layers from roots) and tested;
//! the view maps layers/rows to pixels with one shared function.

use parzi_core::widgets::DiagramV1;

#[cfg(test)]
use parzi_core::widgets::validate_diagram;

/// Longest node label drawn (chars).
pub const LABEL_LIMIT: usize = 24;
/// Horizontal pitch per layer, in points.
pub const LAYER_W: f32 = 170.0;
/// Vertical pitch per row, in points.
pub const ROW_H: f32 = 30.0;

/// A node placed in layer/row space (pixel mapping lives in [`node_pos`]).
#[derive(Debug, Clone)]
pub struct PlacedNode {
    pub id: String,
    pub label: String,
    pub layer: usize,
    pub row: usize,
}

/// Layered layout: roots (no incoming edges) in layer 0, children one
/// layer right of their shallowest parent. Nodes unreachable from any root
/// (cycles, all-linked graphs) keep input order in trailing layers so no
/// node is ever dropped.
#[must_use]
pub fn layout(d: &DiagramV1) -> Vec<PlacedNode> {
    let index_of = |id: &str| d.nodes.iter().position(|n| n.id == id);
    let mut layer_of: Vec<Option<usize>> = vec![None; d.nodes.len()];
    for (i, node) in d.nodes.iter().enumerate() {
        if !d.edges.iter().any(|e| e.to == node.id) {
            layer_of[i] = Some(0);
        }
    }
    // Relax edges longest-path style (bounded passes: converges fast, and
    // the bound keeps adversarial graphs cheap).
    for _ in 0..d.nodes.len() {
        let mut changed = false;
        for e in &d.edges {
            if let (Some(a), Some(b)) = (index_of(&e.from), index_of(&e.to)) {
                if let Some(la) = layer_of[a] {
                    if layer_of[b].is_none_or(|lb| la + 1 > lb) {
                        layer_of[b] = Some(la + 1);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    // Anything still unplaced (cycles with no root) trails after, in order.
    let mut trailing = layer_of.iter().filter_map(|l| *l).max().unwrap_or(0) + 1;
    for slot in layer_of.iter_mut() {
        if slot.is_none() {
            *slot = Some(trailing);
            trailing += 1;
        }
    }
    let mut rows: Vec<usize> = vec![0; trailing + 1];
    d.nodes
        .iter()
        .zip(layer_of)
        .map(|(n, layer)| {
            let layer = layer.unwrap_or(0);
            let row = rows[layer];
            rows[layer] += 1;
            let label = if n.label.is_empty() { &n.id } else { &n.label };
            PlacedNode {
                id: n.id.clone(),
                label: truncate_label(label),
                layer,
                row,
            }
        })
        .collect()
}

fn truncate_label(s: &str) -> String {
    let head: String = s.chars().take(LABEL_LIMIT - 3).collect();
    if s.chars().count() <= LABEL_LIMIT {
        s.to_string()
    } else {
        head + "..."
    }
}

/// Map a placed node to canvas pixels: layers grow right, rows grow down.
#[must_use]
pub fn node_pos(n: &PlacedNode, origin: egui::Pos2) -> egui::Pos2 {
    #[allow(clippy::cast_precision_loss)]
    let pos = egui::pos2(
        origin.x + n.layer as f32 * LAYER_W,
        origin.y + n.row as f32 * ROW_H,
    );
    pos
}

/// A validated diagram ready to render, or the validation failure.
#[derive(Debug, Clone)]
pub enum DiagramView {
    Valid {
        diagram: DiagramV1,
        nodes: Vec<PlacedNode>,
    },
    Invalid {
        error: String,
    },
}

impl DiagramView {
    /// Build from a validation outcome. `Err` becomes `Invalid`, which
    /// renders as an error card instead of panicking.
    #[must_use]
    pub fn from_result(result: parzi_core::Result<DiagramV1>) -> Self {
        match result {
            Err(e) => Self::Invalid {
                error: e.to_string(),
            },
            Ok(d) => {
                let nodes = layout(&d);
                Self::Valid { diagram: d, nodes }
            }
        }
    }

    #[must_use]
    pub const fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid { .. })
    }

    /// Deepest layer index (0 for a single column).
    #[must_use]
    pub fn max_layer(&self) -> usize {
        match self {
            Self::Invalid { .. } => 0,
            Self::Valid { nodes, .. } => nodes.iter().map(|n| n.layer).max().unwrap_or(0),
        }
    }
}

/// Thin renderer: node boxes with centred labels, straight edges between
/// box centres. Unknown edge endpoints are skipped, never fatal.
pub fn show(ui: &mut egui::Ui, view: &mut DiagramView) {
    let nodes = match view {
        DiagramView::Invalid { error } => {
            super::error_card(ui, "Bad diagram", error);
            return;
        }
        DiagramView::Valid { nodes, .. } => nodes.clone(),
    };
    if nodes.is_empty() {
        ui.weak("empty diagram");
        return;
    }
    let depth = nodes.iter().map(|n| n.row).max().unwrap_or(0) + 1;
    let width = nodes.iter().map(|n| n.layer).max().unwrap_or(0) + 1;
    #[allow(clippy::cast_precision_loss)]
    let size = egui::vec2(width as f32 * LAYER_W + 20.0, depth as f32 * ROW_H + 20.0);
    let (resp, painter) = ui.allocate_painter(size, egui::Sense::hover());
    let origin = resp.rect.min + egui::vec2(10.0, 10.0);
    let at = |id: &str| {
        nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| node_pos(n, origin))
    };
    let ink = ui.visuals().text_color();
    let dim = ui.visuals().weak_text_color();
    if let DiagramView::Valid { diagram, .. } = view {
        for e in &diagram.edges {
            if let (Some(a), Some(b)) = (at(&e.from), at(&e.to)) {
                painter.line_segment([a, b], egui::Stroke::new(1.0, dim));
            }
        }
    }
    for n in &nodes {
        let c = node_pos(n, origin);
        painter.rect_filled(
            egui::Rect::from_center_size(c, egui::vec2(LAYER_W - 30.0, ROW_H - 8.0)),
            egui::CornerRadius::same(6),
            ui.visuals().extreme_bg_color,
        );
        painter.text(
            c,
            egui::Align2::CENTER_CENTER,
            &n.label,
            egui::FontId::proportional(12.0),
            ink,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parzi_core::widgets::{DiagramEdge, DiagramNode};

    fn chain() -> DiagramV1 {
        DiagramV1 {
            diagram: 1,
            title: "t".to_string(),
            nodes: ["a", "b", "c"]
                .iter()
                .map(|id| DiagramNode {
                    id: (*id).to_string(),
                    label: (*id).to_string(),
                })
                .collect(),
            edges: [("a", "b"), ("b", "c")]
                .iter()
                .map(|(f, t)| DiagramEdge {
                    from: (*f).to_string(),
                    to: (*t).to_string(),
                    label: String::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn chain_layers_in_order() {
        let layers: Vec<usize> = layout(&chain()).iter().map(|n| n.layer).collect();
        assert_eq!(layers, vec![0, 1, 2]);
    }

    #[test]
    fn cycles_still_place_every_node() {
        let mut d = chain();
        d.edges.push(DiagramEdge {
            from: "c".into(),
            to: "a".into(),
            label: String::new(),
        });
        let nodes = layout(&d);
        assert_eq!(nodes.len(), 3);
        // No root: trailing layers keep input order, nothing dropped.
        let layers: Vec<usize> = nodes.iter().map(|n| n.layer).collect();
        assert_eq!(layers, vec![1, 2, 3]);
    }

    #[test]
    fn positions_grow_right_and_stay_finite() {
        let nodes = layout(&chain());
        let origin = egui::pos2(10.0, 10.0);
        let pts: Vec<_> = nodes.iter().map(|n| node_pos(n, origin)).collect();
        assert!(pts[0].x < pts[1].x && pts[1].x < pts[2].x);
        assert!(pts.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
    }

    #[test]
    fn malformed_diagrams_become_invalid() {
        // Genuine validator failure: no nodes.
        let err = validate_diagram(&Default::default()).expect_err("empty diagram rejected");
        let view = DiagramView::from_result(Err(err));
        assert!(view.is_invalid());
        assert_eq!(view.max_layer(), 0);
        let ok = DiagramView::from_result(Ok(chain()));
        assert!(!ok.is_invalid());
        assert_eq!(ok.max_layer(), 2);
    }
}
