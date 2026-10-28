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

/// `ui.offer_replies`: one to three reply buttons under the agent's last
/// reply. Each has a short label and the message a click sends.
pub fn replies(args: &serde_json::Value) -> std::result::Result<serde_json::Value, String> {
    let text = |v: Option<&serde_json::Value>| {
        v.and_then(|s| s.as_str())
            .map(str::trim)
            .unwrap_or("")
            .to_string()
    };
    let list = args
        .get("replies")
        .and_then(|r| r.as_array())
        .ok_or("replies must be a list of {label, message}")?;
    if list.is_empty() || list.len() > 3 {
        return Err("offer one to three replies".into());
    }
    let mut out = vec![];
    for r in list {
        let label = text(r.get("label"));
        if label.is_empty() {
            return Err("every reply needs a label".into());
        }
        if label.chars().count() > 30 {
            return Err(format!("label `{label}` is over 30 characters: one to four words, the detail goes in message"));
        }
        let message = text(r.get("message"));
        if message.chars().count() > 1000 {
            return Err("a reply's message is over 1000 characters".into());
        }
        let message = if message.is_empty() {
            label.clone()
        } else {
            message
        };
        out.push(serde_json::json!({"label": label, "message": message}));
    }
    let question = text(args.get("question"));
    if question.chars().count() > 160 {
        return Err("the question is over 160 characters: one short line".into());
    }
    Ok(serde_json::json!({"question": question, "replies": out}))
}

/// `todo.write`: the session's checklist, replaced whole each time.
pub fn todo(args: &serde_json::Value) -> std::result::Result<serde_json::Value, String> {
    let list = args
        .get("items")
        .and_then(|r| r.as_array())
        .ok_or("items must be a list of {title, state}")?;
    if list.is_empty() || list.len() > 20 {
        return Err("a checklist has one to twenty items".into());
    }
    let mut out = vec![];
    for it in list {
        let title = it
            .get("title")
            .and_then(|s| s.as_str())
            .map(str::trim)
            .unwrap_or("");
        if title.is_empty() {
            return Err("every item needs a title".into());
        }
        if title.chars().count() > 120 {
            return Err(format!(
                "`{title}` is over 120 characters: a short imperative"
            ));
        }
        let state = it
            .get("state")
            .and_then(|s| s.as_str())
            .unwrap_or("pending");
        if !matches!(state, "pending" | "active" | "done") {
            return Err(format!("state `{state}` is not pending, active or done"));
        }
        out.push(serde_json::json!({"title": title, "state": state}));
    }
    Ok(serde_json::json!({"items": out}))
}

/// An agent's own checklist tool (Claude's `TodoWrite`, OpenCode's
/// `todowrite`, Codex's `update_plan`) as the session checklist
/// `todo.write` makes, so the person sees one list whoever keeps it.
#[must_use]
pub fn todo_from_agent(tool: &str, input: &serde_json::Value) -> Option<serde_json::Value> {
    let t = tool.to_ascii_lowercase();
    if !matches!(t.as_str(), "todowrite" | "todo_write" | "update_plan") {
        return None;
    }
    let list = input
        .get("todos")
        .or_else(|| input.get("plan"))
        .and_then(|l| l.as_array())?;
    let items: Vec<serde_json::Value> = list
        .iter()
        .filter_map(|it| {
            let title = ["content", "step", "title"]
                .iter()
                .find_map(|k| it.get(*k).and_then(|v| v.as_str()))?
                .trim();
            let state = match it.get("status").and_then(|s| s.as_str()).unwrap_or("") {
                "in_progress" | "active" => "active",
                "completed" | "done" => "done",
                "cancelled" | "canceled" => return None,
                _ => "pending",
            };
            let title: String = title.chars().take(120).collect();
            (!title.is_empty()).then(|| serde_json::json!({"title": title, "state": state}))
        })
        .take(20)
        .collect();
    (!items.is_empty()).then(|| serde_json::json!({"items": items}))
}

/// `ui.propose_task`: a card the person can start as a side task. The id
/// lets the card remember whether it was started or dismissed.
pub fn task(args: &serde_json::Value) -> std::result::Result<serde_json::Value, String> {
    let text = |k: &str| {
        args.get(k)
            .and_then(|s| s.as_str())
            .map(str::trim)
            .unwrap_or("")
            .to_string()
    };
    let (title, brief, project) = (text("title"), text("brief"), text("project"));
    if title.is_empty() || brief.is_empty() {
        return Err("a task needs a title and a brief".into());
    }
    if title.chars().count() > 80 {
        return Err("the title is over 80 characters".into());
    }
    if brief.chars().count() > 4000 {
        return Err("the brief is over 4000 characters".into());
    }
    Ok(serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "title": title,
        "brief": brief,
        "project": project,
    }))
}

#[cfg(test)]
mod reply_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_agents_own_checklist_reads_as_the_sessions() {
        let claude = json!({"todos": [
            {"content": "Read the code", "status": "completed", "activeForm": "Reading"},
            {"content": "Fix the bug", "status": "in_progress", "activeForm": "Fixing"},
            {"content": "Old idea", "status": "cancelled"},
            {"content": "Run tests", "status": "pending"},
        ]});
        assert_eq!(
            todo_from_agent("TodoWrite", &claude),
            Some(json!({"items": [
                {"title": "Read the code", "state": "done"},
                {"title": "Fix the bug", "state": "active"},
                {"title": "Run tests", "state": "pending"},
            ]}))
        );
        let codex = json!({"plan": [{"step": "Plan it", "status": "completed"}]});
        assert_eq!(
            todo_from_agent("update_plan", &codex),
            Some(json!({"items": [{"title": "Plan it", "state": "done"}]}))
        );
        assert!(todo_from_agent("todowrite", &json!({"todos": []})).is_none());
        assert!(todo_from_agent("Bash", &claude).is_none());
    }

    #[test]
    fn replies_default_their_message_and_refuse_long_labels() {
        let ok = replies(&json!({"replies": [{"label": "Release 0.1.36"}, {"label": "Not yet", "message": "Not yet, more first."}]})).unwrap();
        assert_eq!(ok["replies"][0]["message"], "Release 0.1.36");
        assert_eq!(ok["replies"][1]["message"], "Not yet, more first.");
        assert!(replies(&json!({"replies": []})).is_err());
        assert!(replies(
            &json!({"replies": [{"label": "a label that is far too long to be a button"}]})
        )
        .is_err());
        assert!(replies(
            &json!({"replies": [{"label": "a"}, {"label": "b"}, {"label": "c"}, {"label": "d"}]})
        )
        .is_err());
    }

    #[test]
    fn a_checklist_keeps_titles_and_states_and_refuses_unknown_states() {
        let t = todo(&json!({"items": [{"title": " Read the code ", "state": "done"}, {"title": "Fix it", "state": "active"}]})).unwrap();
        assert_eq!(t["items"][0]["title"], "Read the code");
        assert_eq!(t["items"][1]["state"], "active");
        assert!(todo(&json!({"items": [{"title": "x", "state": "maybe"}]})).is_err());
        assert!(todo(&json!({"items": []})).is_err());
    }

    #[test]
    fn a_task_needs_a_title_and_a_brief_and_gets_an_id() {
        let t = task(&json!({"title": "Same color on the website", "brief": "Change the accent to slate in web/.", "project": "website"})).unwrap();
        assert_eq!(t["project"], "website");
        assert!(!t["id"].as_str().unwrap().is_empty());
        assert!(task(&json!({"title": "No brief"})).is_err());
    }
}
