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
    "kanban",
    "markdown",
];
const MAX_TABLE_ROWS: usize = 50;
const MAX_POINTS: usize = 200;
const MAX_NODES: usize = 200;
const MAX_EDGES: usize = 400;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramEdge {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramV1 {
    pub diagram: u64,
    #[serde(default)]
    pub title: String,
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
}

/// Schema-checked in core so CLI + GUI + other harnesses agree.
/// Invalid payloads fail safe: callers render a plain code block instead.
pub fn validate_widget(v: &serde_json::Value) -> Result<WidgetV1> {
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
    if w.kind == "table" {
        let nested = w
            .payload
            .get("rows")
            .and_then(|r| r.as_array())
            .map_or(0, |r| r.len());
        let flat = w
            .extra
            .get("rows")
            .and_then(|r| r.as_array())
            .map_or(0, |r| r.len());
        let rows = nested.max(flat);
        if rows > MAX_TABLE_ROWS {
            return Err(ParziError::Validation(format!(
                "table rows {rows} exceed max {MAX_TABLE_ROWS}"
            )));
        }
    }
    if w.kind == "markdown" {
        let text = w
            .extra
            .get("text")
            .and_then(|t| t.as_str())
            .or_else(|| w.payload.get("text").and_then(|t| t.as_str()))
            .unwrap_or("");
        if text.trim().is_empty() {
            return Err(ParziError::Validation("markdown widget is empty".into()));
        }
        if text.chars().count() > 24_000 {
            return Err(ParziError::Validation(
                "markdown widget exceeds 24000 chars".into(),
            ));
        }
    }
    if w.kind == "chart-line" || w.kind == "chart-bar" {
        let n = w
            .extra
            .get("points")
            .and_then(|p| p.as_array())
            .or_else(|| w.payload.get("points").and_then(|p| p.as_array()))
            .map_or(0, |p| p.len());
        if n > MAX_POINTS {
            return Err(ParziError::Validation(format!(
                "chart points {n} exceed max {MAX_POINTS}"
            )));
        }
    }
    Ok(w)
}

pub fn validate_diagram(v: &serde_json::Value) -> Result<DiagramV1> {
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
    Ok(d)
}
