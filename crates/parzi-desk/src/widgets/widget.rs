//! Native `WidgetV1` renderer (stat, progress, list, table, charts,
//! kanban, markdown) from core's validated structs. Payload access goes
//! through `field!` (no `serde_json` dependency here).

use parzi_core::widgets::WidgetV1;

#[cfg(test)]
use parzi_core::widgets::validate_widget;

/// Flat form first, then the nested payload.
macro_rules! field {
    ($w:expr, $key:expr, $m:ident) => {{
        $w.extra
            .get($key)
            .and_then(|v| v.$m())
            .or_else(|| $w.payload.get($key).and_then(|v| v.$m()))
    }};
}

/// Chart flavour for [`WidgetView::Chart`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartKind {
    Line,
    Bar,
}

/// A validated widget ready to render (`Invalid` = error card).
#[derive(Debug, Clone)]
pub enum WidgetView {
    Invalid {
        error: String,
    },
    Stat {
        title: String,
        value: String,
        unit: String,
    },
    Progress {
        title: String,
        value: f64,
    },
    List {
        title: String,
        items: Vec<String>,
    },
    Table {
        title: String,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Chart {
        title: String,
        kind: ChartKind,
        series: Vec<f64>,
    },
    Kanban {
        title: String,
        columns: Vec<(String, Vec<String>)>,
    },
    Markdown {
        title: String,
        text: String,
    },
}

impl WidgetView {
    /// Build from a validation outcome. `Err` (malformed payload, wrong
    /// version, unknown type, over-limit rows/points) becomes `Invalid`.
    /// Never panics: every payload access is `Option`-chained.
    #[must_use]
    pub fn from_result(result: parzi_core::Result<WidgetV1>) -> Self {
        match result {
            Err(e) => Self::Invalid {
                error: e.to_string(),
            },
            Ok(w) => Self::from_valid(&w),
        }
    }

    #[must_use]
    pub const fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid { .. })
    }

    fn from_valid(w: &WidgetV1) -> Self {
        match w.kind.as_str() {
            "stat" => Self::Stat {
                title: w.title.clone(),
                value: text_at(w, "text"),
                unit: text_at(w, "sub"),
            },
            "progress" => Self::Progress {
                title: w.title.clone(),
                value: clamp_unit(field!(w, "value", as_f64).unwrap_or(0.0)),
            },
            "list" => Self::List {
                title: w.title.clone(),
                items: str_array_at(w, "items"),
            },
            "table" => {
                let (columns, rows) = table_at(w);
                Self::Table {
                    title: w.title.clone(),
                    columns,
                    rows,
                }
            }
            "chart-line" => Self::Chart {
                title: w.title.clone(),
                kind: ChartKind::Line,
                series: num_array_at(w, "points"),
            },
            "chart-bar" => Self::Chart {
                title: w.title.clone(),
                kind: ChartKind::Bar,
                series: num_array_at(w, "points"),
            },
            "kanban" => Self::Kanban {
                title: w.title.clone(),
                columns: kanban_at(w),
            },
            _ => Self::Markdown {
                title: w.title.clone(),
                text: text_at(w, "text"),
            },
        }
    }
}

/// Clamp a progress value to 0..=1; non-finite input becomes 0.
#[must_use]
pub fn clamp_unit(v: f64) -> f64 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn text_at(w: &WidgetV1, key: &str) -> String {
    field!(w, key, as_str).unwrap_or("").to_string()
}

/// One JSON scalar as display text.
fn scalar_text(s: Option<&str>, n: Option<f64>, b: Option<bool>) -> Option<String> {
    if let Some(s) = s {
        Some(s.to_string())
    } else if let Some(n) = n {
        Some(if n.fract() == 0.0 {
            format!("{n:.0}")
        } else {
            n.to_string()
        })
    } else {
        b.map(|b| b.to_string())
    }
}

/// Array of strings at `key` (non-strings stringified).
fn str_array_at(w: &WidgetV1, key: &str) -> Vec<String> {
    field!(w, key, as_array).map_or_else(Vec::new, |items| {
        items
            .iter()
            .filter_map(|v| scalar_text(v.as_str(), v.as_f64(), v.as_bool()))
            .collect()
    })
}

/// Finite bare numbers at `key`; anything else is skipped, never fatal.
fn num_array_at(w: &WidgetV1, key: &str) -> Vec<f64> {
    field!(w, key, as_array).map_or_else(Vec::new, |items| {
        items
            .iter()
            .filter_map(|v| v.as_f64())
            .filter(|n| n.is_finite())
            .collect()
    })
}

/// Table grid: explicit `columns` plus array rows (ragged rows padded).
fn table_at(w: &WidgetV1) -> (Vec<String>, Vec<Vec<String>>) {
    let src = field!(w, "rows", as_array).map_or_else(Vec::new, |r| r.iter().collect::<Vec<_>>());
    let mut rows: Vec<Vec<String>> = src
        .iter()
        .map(|r| {
            r.as_array().map_or_else(Vec::new, |cells| {
                cells
                    .iter()
                    .filter_map(|c| scalar_text(c.as_str(), c.as_f64(), c.as_bool()))
                    .collect()
            })
        })
        .collect();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    for r in &mut rows {
        r.resize(width, String::new());
    }
    let mut columns = str_array_at(w, "columns");
    if columns.is_empty() {
        columns = (1..=width).map(|i| format!("c{i}")).collect();
    }
    (columns, rows)
}

/// Kanban board: `columns` array of `{title, cards[]}` maps.
fn kanban_at(w: &WidgetV1) -> Vec<(String, Vec<String>)> {
    field!(w, "columns", as_array).map_or_else(Vec::new, |cols| {
        cols.iter()
            .map(|c| {
                let name = c
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let all = c.get("cards").and_then(|v| v.as_array());
                let cards = all.map_or_else(Vec::new, |arr| {
                    arr.iter()
                        .filter_map(|x| scalar_text(x.as_str(), x.as_f64(), x.as_bool()))
                        .collect()
                });
                (name, cards)
            })
            .collect()
    })
}

/// Thin card renderer. `Invalid` becomes the shared error card.
pub fn show(ui: &mut egui::Ui, view: &mut WidgetView) {
    match view {
        WidgetView::Invalid { error } => super::error_card(ui, "Bad widget", error),
        WidgetView::Stat { title, value, unit } => {
            super::framed(ui, title, |ui| {
                ui.heading(format!("{value} {unit}").trim_end());
            });
        }
        WidgetView::Progress { title, value } => {
            super::framed(ui, title, |ui| {
                #[allow(clippy::cast_possible_truncation)]
                let v = *value as f32;
                ui.add(egui::ProgressBar::new(v).show_percentage());
            });
        }
        WidgetView::List { title, items } => {
            super::framed(ui, title, |ui| {
                if items.is_empty() {
                    ui.weak("empty list");
                }
                for item in items {
                    ui.label(format!("- {item}"));
                }
            });
        }
        WidgetView::Table {
            title,
            columns,
            rows,
        } => {
            super::framed(ui, title, |ui| {
                if rows.is_empty() {
                    ui.weak("empty table");
                    return;
                }
                egui::Grid::new("widget-table")
                    .striped(true)
                    .show(ui, |ui| {
                        for col in &*columns {
                            ui.strong(col);
                        }
                        ui.end_row();
                        for row in &*rows {
                            for cell in row {
                                ui.label(cell);
                            }
                            ui.end_row();
                        }
                    });
            });
        }
        WidgetView::Chart {
            title,
            kind,
            series,
        } => {
            super::framed(ui, title, |ui| {
                if series.is_empty() {
                    ui.weak("no points");
                    return;
                }
                paint_chart(ui, *kind, series);
            });
        }
        WidgetView::Kanban { title, columns } => {
            super::framed(ui, title, |ui| {
                if columns.is_empty() {
                    ui.weak("no columns");
                    return;
                }
                ui.horizontal(|ui| {
                    for (name, cards) in columns {
                        ui.vertical(|ui| {
                            ui.strong(name);
                            for card in cards {
                                ui.label(format!("- {card}"));
                            }
                        });
                    }
                });
            });
        }
        WidgetView::Markdown { title, text } => {
            super::framed(ui, title, |ui| {
                ui.add(egui::Label::new(text.as_str()).selectable(true).wrap());
            });
        }
    }
}

/// Min-max scaled line/bar chart; flat ranges cannot divide by zero.
fn paint_chart(ui: &mut egui::Ui, kind: ChartKind, series: &[f64]) {
    let w = ui.available_width().max(120.0);
    let (resp, painter) = ui.allocate_painter(egui::vec2(w, 110.0), egui::Sense::hover());
    let rect = resp.rect.shrink(6.0);
    let min = series.iter().cloned().fold(f64::INFINITY, f64::min);
    let span = (series.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - min).max(f64::EPSILON);
    let n = series.len();
    let accent = ui.visuals().hyperlink_color;
    let x_at = |i: usize| rect.left() + i as f32 / (n.max(2) - 1) as f32 * rect.width();
    #[allow(clippy::cast_possible_truncation)]
    let y_at = |v: f64| rect.bottom() - ((v - min) / span) as f32 * rect.height();
    if kind == ChartKind::Line {
        for i in 1..n {
            let a = egui::pos2(x_at(i - 1), y_at(series[i - 1]));
            let b = egui::pos2(x_at(i), y_at(series[i]));
            painter.line_segment([a, b], egui::Stroke::new(1.5, accent));
        }
    } else {
        #[allow(clippy::cast_possible_truncation)]
        let bw = (rect.width() / n as f32 * 0.6).max(2.0);
        for (i, v) in series.iter().enumerate() {
            let x = x_at(i);
            let top = y_at(*v);
            painter.line_segment(
                [egui::pos2(x, top), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(bw, accent),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_widget(kind: &str) -> WidgetV1 {
        WidgetV1 {
            widget: 1,
            kind: kind.to_string(),
            title: "t".to_string(),
            payload: Default::default(),
            extra: Default::default(),
        }
    }

    #[test]
    fn malformed_payloads_become_invalid() {
        // Genuine validator failure through the real pipeline: null is not a widget.
        let err = validate_widget(&Default::default()).expect_err("null is not a widget");
        assert!(WidgetView::from_result(Err(err)).is_invalid());
        let bad = parzi_core::ParziError::Validation("bad widget".into());
        let view = WidgetView::from_result(Err(bad));
        assert!(matches!(view, WidgetView::Invalid { .. }));
    }

    #[test]
    fn clamp_unit_and_scalars() {
        assert_eq!(clamp_unit(f64::NAN), 0.0);
        assert_eq!(clamp_unit(2.5), 1.0);
        assert_eq!(clamp_unit(-1.0), 0.0);
        assert_eq!(clamp_unit(0.6), 0.6);
        assert_eq!(scalar_text(Some("a"), None, None), Some("a".to_string()));
        assert_eq!(scalar_text(None, Some(2.0), None), Some("2".to_string()));
        assert_eq!(scalar_text(None, Some(2.5), None), Some("2.5".to_string()));
    }

    #[test]
    fn empty_widgets_render_gracefully() {
        let progress = WidgetView::from_result(Ok(empty_widget("progress")));
        assert!(matches!(progress, WidgetView::Progress { value, .. } if value == 0.0));
        for kind in ["list", "table", "chart-line", "kanban", "markdown"] {
            let view = WidgetView::from_result(Ok(empty_widget(kind)));
            assert!(!view.is_invalid(), "{kind}");
        }
    }
}
