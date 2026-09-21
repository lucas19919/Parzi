use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};

const WIDGET_VERSION: u64 = 1;
const DIAGRAM_VERSION: u64 = 1;

const WIDGET_TYPES: &[&str] = &[
    "stat",
    "progress",
    "list",
    "table",
    "chart-line",
    "chart-bar",
    "histogram",
    "scatter",
    "kanban",
    "markdown",
];
const MAX_TABLE_ROWS: usize = 50;
const MAX_POINTS: usize = 200;
const MAX_SERIES: usize = 8;
const MAX_NODES: usize = 200;
const MAX_EDGES: usize = 400;
const MAX_TOTAL_BYTES: usize = 65_536;
const MAX_LABEL_CHARS: usize = 120;
const MAX_GROUPS: usize = 20;

const NODE_COLORS: &[&str] = &["accent", "ok", "warn", "bad", "info"];
const NODE_SHAPES: &[&str] = &["flow", "db", "diamond", "actor"];
const EDGE_STYLES: &[&str] = &["solid", "dashed", "dotted", "thick"];

fn shape<'a>(w: &'a WidgetV1, key: &str) -> Option<&'a serde_json::Value> {
    w.extra
        .get(key)
        .or_else(|| w.payload.get(key))
        .filter(|v| !v.is_null())
}

fn valid_numbers(arr: &[serde_json::Value]) -> bool {
    arr.iter().all(|p| {
        p.is_null()
            || p.is_number()
            || p.as_str()
                .is_some_and(|s| !s.trim().is_empty() && s.trim().parse::<f64>().is_ok())
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetV1 {
    pub widget: u64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    /// Nested form: {"widget":1,"type":"table","payload":{"rows":[...]}}.
    #[serde(default)]
    pub payload: serde_json::Value,
    /// Flat form agents actually emit: {"widget":1,"type":"progress","value":0.6}.
    #[serde(flatten, default)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramNode {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub sub: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub shape: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramEdge {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub style: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramGroup {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramV1 {
    pub diagram: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub layout: String,
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
    #[serde(default)]
    pub groups: Vec<DiagramGroup>,
}

/// Schema-checked in core so CLI + GUI + other harnesses agree.
/// Invalid payloads fail safe: callers render a plain code block instead.
pub fn validate_widget(v: &serde_json::Value) -> Result<WidgetV1> {
    if serde_json::to_string(v)
        .map(|s| s.len())
        .unwrap_or(usize::MAX)
        > MAX_TOTAL_BYTES
    {
        return Err(ParziError::Validation(format!(
            "widget exceeds {MAX_TOTAL_BYTES} bytes"
        )));
    }
    let w: WidgetV1 = serde_json::from_value(v.clone())
        .map_err(|e| ParziError::Validation(format!("bad widget: {e}")))?;
    if w.widget != WIDGET_VERSION {
        return Err(ParziError::Validation(format!(
            "widget version {} unsupported (want {WIDGET_VERSION})",
            w.widget
        )));
    }
    if !WIDGET_TYPES.contains(&w.kind.as_str()) {
        return Err(ParziError::Validation(format!(
            "unknown widget type `{}`",
            w.kind
        )));
    }
    match w.kind.as_str() {
        "progress" => {
            if let Some(val) = shape(&w, "value") {
                if !val.is_number() || !val.as_f64().is_some_and(f64::is_finite) {
                    return Err(ParziError::Validation(
                        "progress value must be a finite number".into(),
                    ));
                }
            }
        }
        "list" => {
            if let Some(items) = shape(&w, "items").or_else(|| shape(&w, "list")) {
                if !items.is_array() {
                    return Err(ParziError::Validation("list items must be an array".into()));
                }
            }
        }
        "table" => {
            if let Some(rows) = shape(&w, "rows") {
                if !rows.is_array() {
                    return Err(ParziError::Validation("table rows must be an array".into()));
                }
                if rows.as_array().is_some_and(|r| r.len() > MAX_TABLE_ROWS) {
                    return Err(ParziError::Validation(format!(
                        "table rows exceed max {MAX_TABLE_ROWS}"
                    )));
                }
            }
            if let Some(cols) = shape(&w, "columns") {
                if !cols.is_array() {
                    return Err(ParziError::Validation(
                        "table columns must be an array".into(),
                    ));
                }
            }
        }
        "markdown" => {
            let text = shape(&w, "text").and_then(|t| t.as_str()).unwrap_or("");
            if text.trim().is_empty() {
                return Err(ParziError::Validation("markdown widget is empty".into()));
            }
            if text.chars().count() > 24_000 {
                return Err(ParziError::Validation(
                    "markdown widget exceeds 24000 chars".into(),
                ));
            }
        }
        "chart-line" | "chart-bar" => {
            if let Some(series) = shape(&w, "series") {
                let arr = series.as_array().ok_or_else(|| {
                    ParziError::Validation("chart series must be an array".into())
                })?;
                if arr.len() > MAX_SERIES {
                    return Err(ParziError::Validation(format!(
                        "chart series {} exceed max {MAX_SERIES}",
                        arr.len()
                    )));
                }
                for s in arr {
                    let pts = s.get("points").and_then(|p| p.as_array()).ok_or_else(|| {
                        ParziError::Validation("chart series need a points array".into())
                    })?;
                    if pts.len() > MAX_POINTS {
                        return Err(ParziError::Validation(format!(
                            "chart points {} exceed max {MAX_POINTS}",
                            pts.len()
                        )));
                    }
                    if !valid_numbers(pts) {
                        return Err(ParziError::Validation(
                            "chart points must be numbers".into(),
                        ));
                    }
                }
            } else if let Some(points) = shape(&w, "points") {
                let arr = points.as_array().ok_or_else(|| {
                    ParziError::Validation("chart points must be an array".into())
                })?;
                if arr.len() > MAX_POINTS {
                    return Err(ParziError::Validation(format!(
                        "chart points {} exceed max {MAX_POINTS}",
                        arr.len()
                    )));
                }
                if !valid_numbers(arr) {
                    return Err(ParziError::Validation(
                        "chart points must be numbers".into(),
                    ));
                }
            }
        }
        "histogram" => {
            if let Some(values) = shape(&w, "values") {
                let arr = values.as_array().ok_or_else(|| {
                    ParziError::Validation("histogram values must be an array".into())
                })?;
                if arr.len() > MAX_POINTS * 10 {
                    return Err(ParziError::Validation(format!(
                        "histogram values exceed max {}",
                        MAX_POINTS * 10
                    )));
                }
                if !valid_numbers(arr) {
                    return Err(ParziError::Validation(
                        "histogram values must be numbers".into(),
                    ));
                }
            }
            if let Some(bins) = shape(&w, "bins") {
                let n = bins.as_u64().ok_or_else(|| {
                    ParziError::Validation("histogram bins must be an integer".into())
                })?;
                if !(2..=50).contains(&n) {
                    return Err(ParziError::Validation(
                        "histogram bins out of range 2..=50".into(),
                    ));
                }
            }
        }
        "scatter" => {
            let check_pairs = |key: &str| -> Result<()> {
                if let Some(points) = shape(&w, key) {
                    let arr = points.as_array().ok_or_else(|| {
                        ParziError::Validation("scatter points must be [x, y] pairs".into())
                    })?;
                    if arr.len() > MAX_POINTS {
                        return Err(ParziError::Validation(format!(
                            "scatter points {} exceed max {MAX_POINTS}",
                            arr.len()
                        )));
                    }
                    for p in arr {
                        let pair = p.as_array().ok_or_else(|| {
                            ParziError::Validation("scatter points must be [x, y] pairs".into())
                        })?;
                        if pair.len() != 2
                            || !pair
                                .iter()
                                .all(|v| v.is_number() && v.as_f64().is_some_and(f64::is_finite))
                        {
                            return Err(ParziError::Validation(
                                "scatter points must be finite [x, y] numbers".into(),
                            ));
                        }
                    }
                }
                Ok(())
            };
            check_pairs("points")?;
            if let Some(series) = shape(&w, "series") {
                let arr = series.as_array().ok_or_else(|| {
                    ParziError::Validation("scatter series must be an array".into())
                })?;
                if arr.len() > MAX_SERIES {
                    return Err(ParziError::Validation(format!(
                        "scatter series {} exceed max {MAX_SERIES}",
                        arr.len()
                    )));
                }
                for s in arr {
                    let pts = s.get("points").and_then(|p| p.as_array()).ok_or_else(|| {
                        ParziError::Validation("scatter series need a points array".into())
                    })?;
                    if pts.len() > MAX_POINTS {
                        return Err(ParziError::Validation(format!(
                            "scatter points {} exceed max {MAX_POINTS}",
                            pts.len()
                        )));
                    }
                    for p in pts {
                        let pair = p.as_array().ok_or_else(|| {
                            ParziError::Validation("scatter points must be [x, y] pairs".into())
                        })?;
                        if pair.len() != 2
                            || !pair
                                .iter()
                                .all(|v| v.is_number() && v.as_f64().is_some_and(f64::is_finite))
                        {
                            return Err(ParziError::Validation(
                                "scatter points must be finite [x, y] numbers".into(),
                            ));
                        }
                    }
                }
            }
        }
        "kanban" => {
            if let Some(cols) = shape(&w, "columns") {
                let cols = cols.as_array().ok_or_else(|| {
                    ParziError::Validation("kanban columns must be an array".into())
                })?;
                if cols.iter().any(|c| {
                    !c.is_object()
                        || c.get("cards")
                            .is_some_and(|cards| !cards.is_null() && !cards.is_array())
                }) {
                    return Err(ParziError::Validation(
                        "kanban columns need object shape with array cards".into(),
                    ));
                }
            }
        }
        _ => {}
    }
    Ok(w)
}

pub fn validate_diagram(v: &serde_json::Value) -> Result<DiagramV1> {
    if serde_json::to_string(v)
        .map(|s| s.len())
        .unwrap_or(usize::MAX)
        > MAX_TOTAL_BYTES
    {
        return Err(ParziError::Validation(format!(
            "diagram exceeds {MAX_TOTAL_BYTES} bytes"
        )));
    }
    let d: DiagramV1 = serde_json::from_value(v.clone())
        .map_err(|e| ParziError::Validation(format!("bad diagram: {e}")))?;
    if d.diagram != DIAGRAM_VERSION {
        return Err(ParziError::Validation(format!(
            "diagram version {} unsupported (want {DIAGRAM_VERSION})",
            d.diagram
        )));
    }
    if d.nodes.is_empty() || d.nodes.len() > MAX_NODES {
        return Err(ParziError::Validation(format!(
            "nodes {} out of range 1..={MAX_NODES}",
            d.nodes.len()
        )));
    }
    if d.edges.len() > MAX_EDGES {
        return Err(ParziError::Validation(format!(
            "edges {} exceed max {MAX_EDGES}",
            d.edges.len()
        )));
    }
    let mut ids = std::collections::HashSet::new();
    for n in &d.nodes {
        if n.id.trim().is_empty() || !ids.insert(n.id.clone()) {
            return Err(ParziError::Validation(
                "diagram node ids must be unique and non-empty".into(),
            ));
        }
        for text in [&n.label, &n.sub] {
            if text.chars().count() > MAX_LABEL_CHARS {
                return Err(ParziError::Validation(format!(
                    "diagram labels capped at {MAX_LABEL_CHARS} chars"
                )));
            }
        }
        if !n.color.trim().is_empty() && !NODE_COLORS.contains(&n.color.as_str()) {
            return Err(ParziError::Validation(format!(
                "unknown node color `{}`",
                n.color
            )));
        }
        if !n.shape.trim().is_empty() && !NODE_SHAPES.contains(&n.shape.as_str()) {
            return Err(ParziError::Validation(format!(
                "unknown node shape `{}`",
                n.shape
            )));
        }
    }
    for e in &d.edges {
        if !ids.contains(&e.from) || !ids.contains(&e.to) {
            return Err(ParziError::Validation(format!(
                "diagram edge references unknown node: {} -> {}",
                e.from, e.to
            )));
        }
        if e.label.chars().count() > MAX_LABEL_CHARS {
            return Err(ParziError::Validation(format!(
                "diagram labels capped at {MAX_LABEL_CHARS} chars"
            )));
        }
        if !e.color.trim().is_empty() && !NODE_COLORS.contains(&e.color.as_str()) {
            return Err(ParziError::Validation(format!(
                "unknown edge color `{}`",
                e.color
            )));
        }
        if !e.style.trim().is_empty() && !EDGE_STYLES.contains(&e.style.as_str()) {
            return Err(ParziError::Validation(format!(
                "unknown edge style `{}`",
                e.style
            )));
        }
    }
    if d.groups.len() > MAX_GROUPS {
        return Err(ParziError::Validation(format!(
            "diagram groups exceed max {MAX_GROUPS}"
        )));
    }
    for g in &d.groups {
        if g.id.trim().is_empty() {
            return Err(ParziError::Validation(
                "diagram groups need a non-empty id".into(),
            ));
        }
        if g.label.chars().count() > MAX_LABEL_CHARS {
            return Err(ParziError::Validation(format!(
                "diagram labels capped at {MAX_LABEL_CHARS} chars"
            )));
        }
        if g.nodes.iter().any(|m| !ids.contains(m)) {
            return Err(ParziError::Validation(format!(
                "diagram group `{}` references unknown node",
                g.id
            )));
        }
    }
    Ok(d)
}
