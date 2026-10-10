use serde::{Deserialize, Serialize};

use crate::error::{ParziError, Result};

const WIDGET_VERSION: u64 = 1;

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
const MAX_TOTAL_BYTES: usize = 65_536;

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
            // "NaN" and "inf" parse as f64 but cannot be plotted.
            || p.as_str()
                .is_some_and(|s| s.trim().parse::<f64>().is_ok_and(f64::is_finite))
    })
}

fn check_number_list(arr: &[serde_json::Value], what: &str, max: usize) -> Result<()> {
    if arr.len() > max {
        return Err(ParziError::Validation(format!(
            "{what} {n} exceed max {max}",
            n = arr.len()
        )));
    }
    if !valid_numbers(arr) {
        return Err(ParziError::Validation(format!("{what} must be numbers")));
    }
    Ok(())
}

fn check_cartesian(w: &WidgetV1) -> Result<()> {
    if let Some(series) = shape(w, "series") {
        let arr = series
            .as_array()
            .ok_or_else(|| ParziError::Validation("chart series must be an array".into()))?;
        if arr.len() > MAX_SERIES {
            return Err(ParziError::Validation(format!(
                "chart series {} exceed max {MAX_SERIES}",
                arr.len()
            )));
        }
        for s in arr {
            let pts = s
                .get("points")
                .and_then(|p| p.as_array())
                .ok_or_else(|| ParziError::Validation("chart series need a points array".into()))?;
            check_number_list(pts, "chart points", MAX_POINTS)?;
        }
    } else if let Some(points) = shape(w, "points") {
        let arr = points
            .as_array()
            .ok_or_else(|| ParziError::Validation("chart points must be an array".into()))?;
        check_number_list(arr, "chart points", MAX_POINTS)?;
    }
    Ok(())
}

fn check_histogram(w: &WidgetV1) -> Result<()> {
    if let Some(values) = shape(w, "values") {
        let arr = values
            .as_array()
            .ok_or_else(|| ParziError::Validation("histogram values must be an array".into()))?;
        check_number_list(arr, "histogram values", MAX_POINTS * 10)?;
    }
    if let Some(bins) = shape(w, "bins") {
        let n = bins
            .as_u64()
            .ok_or_else(|| ParziError::Validation("histogram bins must be an integer".into()))?;
        if !(2..=50).contains(&n) {
            return Err(ParziError::Validation(
                "histogram bins out of range 2..=50".into(),
            ));
        }
    }
    Ok(())
}

fn check_pair_list(arr: &[serde_json::Value]) -> Result<()> {
    if arr.len() > MAX_POINTS {
        return Err(ParziError::Validation(format!(
            "scatter points {} exceed max {MAX_POINTS}",
            arr.len()
        )));
    }
    for p in arr {
        let pair = p
            .as_array()
            .ok_or_else(|| ParziError::Validation("scatter points must be [x, y] pairs".into()))?;
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
    Ok(())
}

fn check_scatter(w: &WidgetV1) -> Result<()> {
    if let Some(points) = shape(w, "points") {
        let arr = points
            .as_array()
            .ok_or_else(|| ParziError::Validation("scatter points must be [x, y] pairs".into()))?;
        check_pair_list(arr)?;
    }
    if let Some(series) = shape(w, "series") {
        let arr = series
            .as_array()
            .ok_or_else(|| ParziError::Validation("scatter series must be an array".into()))?;
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
            check_pair_list(pts)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetV1 {
    pub widget: u64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(flatten, default)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

pub fn validate_widget(v: &serde_json::Value) -> Result<WidgetV1> {
    if serde_json::to_string(v).map_or(usize::MAX, |s| s.len()) > MAX_TOTAL_BYTES {
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
    check_kind(&w)?;
    Ok(w)
}

fn check_kind(w: &WidgetV1) -> Result<()> {
    match w.kind.as_str() {
        "progress" => {
            if let Some(val) = shape(w, "value") {
                if !val.is_number() || !val.as_f64().is_some_and(f64::is_finite) {
                    return Err(ParziError::Validation(
                        "progress value must be a finite number".into(),
                    ));
                }
            }
        }
        "list" => {
            if let Some(items) = shape(w, "items").or_else(|| shape(w, "list")) {
                if !items.is_array() {
                    return Err(ParziError::Validation("list items must be an array".into()));
                }
            }
        }
        "table" => {
            if let Some(rows) = shape(w, "rows") {
                if !rows.is_array() {
                    return Err(ParziError::Validation("table rows must be an array".into()));
                }
                if rows.as_array().is_some_and(|r| r.len() > MAX_TABLE_ROWS) {
                    return Err(ParziError::Validation(format!(
                        "table rows exceed max {MAX_TABLE_ROWS}"
                    )));
                }
            }
            if let Some(cols) = shape(w, "columns") {
                if !cols.is_array() {
                    return Err(ParziError::Validation(
                        "table columns must be an array".into(),
                    ));
                }
            }
        }
        "markdown" => {
            let text = shape(w, "text").and_then(|t| t.as_str()).unwrap_or("");
            if text.trim().is_empty() {
                return Err(ParziError::Validation("markdown widget is empty".into()));
            }
            if text.chars().count() > 24_000 {
                return Err(ParziError::Validation(
                    "markdown widget exceeds 24000 chars".into(),
                ));
            }
        }
        "chart-line" | "chart-bar" => check_cartesian(w)?,
        "histogram" => check_histogram(w)?,
        "scatter" => check_scatter(w)?,
        "kanban" => {
            if let Some(cols) = shape(w, "columns") {
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
    Ok(())
}
