use std::collections::HashMap;
use std::io::Write as _;
use std::time::Duration;

use anyhow::{bail, Result};
use parzi_core::text::short_id as short;
use parzi_runtime::desk::{self, DeskError};
use serde_json::{json, Value};

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

pub(crate) async fn ask(op: &str, body: Value) -> Result<Value> {
    ask_with(op, body, &mut |v| say_confirm(v)).await
}

pub(crate) async fn ask_with(
    op: &str,
    body: Value,
    on_event: &mut (dyn FnMut(&Value) + Send),
) -> Result<Value> {
    let wait = desk::deadline(op, &body);
    desk::request_within(op, body, wait, on_event)
        .await
        .map_err(|e| anyhow::anyhow!(e))
}

fn say_confirm(v: &Value) {
    if s(v, "kind") == "confirm" {
        eprintln!("{}", s(v, "text"));
    }
}

pub(crate) fn print_json(v: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

fn put(body: &mut Value, key: &str, value: Option<impl Into<Value>>) {
    if let Some(v) = value {
        body[key] = v.into();
    }
}

pub(crate) fn page_body(tab: Option<String>, args: Value) -> Value {
    let mut body = json!({ "args": args });
    put(&mut body, "tab", tab);
    body
}

pub(crate) async fn page(op: &str, body: Value) -> Result<()> {
    let v = ask(op, body).await?;
    let done = s(&v, "done");
    match s(&v, "label") {
        "" => println!("{done}"),
        label => println!("{done} {label:?}"),
    }
    Ok(())
}

pub(crate) async fn tab_read(
    tab: Option<String>,
    offset: Option<u64>,
    limit: Option<u64>,
    json_out: bool,
) -> Result<()> {
    let mut body = json!({});
    put(&mut body, "tab", tab);
    put(&mut body, "offset", offset);
    put(&mut body, "limit", limit);
    let v = ask("tab.read", body).await?;
    if json_out {
        return print_json(&v);
    }
    println!("{}\n{}\n", s(&v, "title"), s(&v, "url"));
    println!("{}", s(&v, "text").trim_end());
    let controls: Vec<&str> = v
        .get("controls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !controls.is_empty() {
        println!("\ncontrols:");
        for c in controls {
            println!("  {c}");
        }
    }
    let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    if n("next") < n("total") {
        eprintln!(
            "\n{} of {} chars; more with --offset {}",
            n("next"),
            n("total"),
            n("next")
        );
    }
    Ok(())
}

fn save_jpeg(v: &Value, prefix: &str, out: Option<&std::path::Path>) -> Result<()> {
    let bytes = desk::shot_bytes(v).map_err(|e| anyhow::anyhow!(e))?;
    let path = match out {
        Some(p) => {
            std::fs::write(p, &bytes)
                .map_err(|e| anyhow::anyhow!("cannot write {}: {e}", p.display()))?;
            p.to_path_buf()
        }
        None => desk::save_shot("cli", prefix, &bytes).map_err(|e| anyhow::anyhow!(e))?,
    };
    println!("{}", path.display());
    Ok(())
}

pub(crate) async fn tab_shot(tab: Option<String>, out: Option<std::path::PathBuf>) -> Result<()> {
    let v = ask("tab.shot", page_body(tab, json!({}))).await?;
    save_jpeg(&v, "tab", out.as_deref())
}

pub(crate) async fn screenshot(out: Option<std::path::PathBuf>) -> Result<()> {
    let v = ask("desk.screenshot", json!({})).await?;
    save_jpeg(&v, "window", out.as_deref())
}

pub(crate) async fn tab_list(json_out: bool) -> Result<()> {
    let v = ask("tab.list", json!({})).await?;
    if json_out {
        return print_json(&v);
    }
    let rows = v
        .get("tabs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if rows.is_empty() {
        println!("no tabs");
        return Ok(());
    }
    let active = s(&v, "active");
    for row in &rows {
        let mark = if s(row, "id") == active { "*" } else { " " };
        let owner = match s(row, "session_id") {
            "" => String::new(),
            sid if s(row, "kind") == "browser" => format!("  (agent {})", short(sid)),
            _ => String::new(),
        };
        println!(
            "{mark} {}  {}  {}  {}{owner}",
            s(row, "id"),
            s(row, "kind"),
            s(row, "title"),
            s(row, "url")
        );
    }
    Ok(())
}

pub(crate) async fn simple(op: &str, body: Value) -> Result<()> {
    let v = ask(op, body).await?;
    let id = s(&v, "id");
    let done = s(&v, "done");
    match (done, id) {
        ("", "") => println!("ok"),
        ("", id) => println!("{id}"),
        (done, _) => println!("{done}"),
    }
    Ok(())
}

pub(crate) async fn state(json_out: bool) -> Result<()> {
    let v = match desk::request("desk.state", json!({})).await {
        Ok(v) => v,
        Err(DeskError::NotOpen) => {
            if json_out {
                println!("{}", json!({ "open": false }));
            } else {
                println!("{}", DeskError::NotOpen);
            }
            return Ok(());
        }
        Err(e) => bail!(e),
    };
    if json_out {
        return print_json(&v);
    }
    let up = v
        .get("started")
        .and_then(Value::as_u64)
        .map_or_else(String::new, |t| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            format!(", up {}", ago(now.saturating_sub(t)))
        });
    println!(
        "Parzi {} (pid {}{up}), {}",
        s(&v, "version"),
        v.get("pid").and_then(Value::as_u64).unwrap_or(0),
        if v.get("focused").and_then(Value::as_bool) == Some(true) {
            "in front"
        } else {
            "in the background"
        }
    );
    let folder = s(&v, "folder");
    if !folder.is_empty() {
        let project = v
            .pointer("/project/slug")
            .and_then(Value::as_str)
            .map(|p| format!("  (project {p})"))
            .unwrap_or_default();
        println!("folder  {folder}{project}");
    }
    let active = active_tab(&v);
    println!("tabs");
    for t in v
        .get("tabs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mark = if s(t, "id") == active { "*" } else { " " };
        println!(
            "{mark} {}  {}  {}  {}",
            s(t, "id"),
            s(t, "kind"),
            s(t, "title"),
            s(t, "url")
        );
    }
    let running: Vec<&Value> = v
        .get("running")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    if running.is_empty() {
        println!("running  none");
    } else {
        println!("running");
        for r in running {
            println!("  {}  {}", short(s(r, "id")), s(r, "title"));
        }
    }
    Ok(())
}

fn active_tab(v: &Value) -> &str {
    match v.get("active") {
        Some(Value::String(id)) => id,
        Some(a @ Value::Object(_)) => s(a, "id"),
        _ => "",
    }
}

fn ago(secs: u64) -> String {
    match secs {
        0..=119 => format!("{secs}s"),
        120..=7199 => format!("{}m", secs / 60),
        _ => format!("{}h", secs / 3600),
    }
}

pub(crate) fn send_body(
    target: &str,
    message: &str,
    model: Option<&str>,
    cwd: &str,
    yes: bool,
    effort: &str,
) -> Value {
    json!({
        "target": target,
        "message": message,
        "model": model.unwrap_or_default(),
        "cwd": cwd,
        "yes": yes,
        "effort": effort,
        "stream": true,
    })
}

#[derive(Default)]
pub(crate) struct SendView {
    pub session: Option<String>,
    pub printed: bool,
    pub ended_line: bool,
}

impl SendView {
    pub(crate) fn show(&mut self, v: &Value) {
        let kind = s(v, "kind");
        match kind {
            "confirm" => eprintln!("{}", s(v, "text")),
            "started" => {
                self.session = Some(s(v, "session").to_string());
                eprintln!("session {}", s(v, "session"));
            }
            "text" => {
                let text = s(v, "text");
                print!("{text}");
                let _ = std::io::stdout().flush();
                self.printed = true;
                self.ended_line = text.ends_with('\n');
            }
            "tool_call" => {
                self.break_line();
                let label = match s(v, "label") {
                    "" => s(v, "name"),
                    l => l,
                };
                eprintln!("[tool] {label}");
            }
            "tool_result" if v.get("ok").and_then(Value::as_bool) == Some(false) => {
                eprintln!("[tool failed] {}", s(v, "name"));
            }
            "notice" => {
                self.break_line();
                eprintln!("[notice] {}", s(v, "text"));
            }
            "approval" => {
                self.break_line();
                eprintln!(
                    "[waiting for approval] {}: answer in the window, or `parzi approval allow {}`",
                    s(v, "label"),
                    short(s(v, "key"))
                );
            }
            _ => {}
        }
    }

    fn break_line(&mut self) {
        if self.printed && !self.ended_line {
            println!();
            self.ended_line = true;
        }
    }
}

#[derive(Default)]
pub(crate) struct WatchView {
    partial: HashMap<String, String>,
}

impl WatchView {
    fn flush(&mut self, session: &str) -> Vec<String> {
        match self.partial.remove(session) {
            Some(rest) if !rest.trim().is_empty() => vec![format!("{} {}", short(session), rest)],
            _ => vec![],
        }
    }

    pub(crate) fn lines(&mut self, v: &Value) -> Vec<String> {
        let session = s(v, "session").to_string();
        let id = short(&session).to_string();
        let kind = s(v, "kind");
        if kind == "text" {
            let buf = self.partial.entry(session.clone()).or_default();
            buf.push_str(s(v, "text"));
            let mut out = vec![];
            while let Some(at) = buf.find('\n') {
                let line: String = buf.drain(..=at).collect();
                let line = line.trim_end();
                if !line.is_empty() {
                    out.push(format!("{id} {line}"));
                }
            }
            return out;
        }
        let mut out = if session.is_empty() {
            vec![]
        } else {
            self.flush(&session)
        };
        let line = match kind {
            "tool_call" => Some(format!(
                "{id} > {}",
                match s(v, "label") {
                    "" => s(v, "name"),
                    l => l,
                }
            )),
            "tool_result" if v.get("ok").and_then(Value::as_bool) == Some(false) => {
                Some(format!("{id} tool failed: {}", s(v, "name")))
            }
            "notice" => Some(format!("{id} notice: {}", s(v, "text"))),
            "approval" => Some(format!(
                "{id} needs approval: {} (parzi approval allow {})",
                s(v, "label"),
                short(s(v, "key"))
            )),
            "approval_done" => Some(format!("approval {} answered", short(s(v, "key")))),
            "question" => Some(format!("{id} asks: {}", s(v, "question"))),
            "done" => Some(format!("{id} finished")),
            "paused" => Some(format!("{id} paused: {}", s(v, "reason"))),
            "error" => Some(format!("{id} stopped: {}", s(v, "error"))),
            "tabs" => {
                let tabs = v.get("tabs").and_then(Value::as_array);
                let active = s(v, "active");
                let front = tabs
                    .into_iter()
                    .flatten()
                    .find(|t| s(t, "id") == active)
                    .map_or("", |t| s(t, "title"));
                Some(format!(
                    "tabs: {} open, in front: {front}",
                    tabs.map_or(0, Vec::len)
                ))
            }
            "notify" => Some(format!("notice: {}", s(v, "text"))),
            "lagged" => Some(format!(
                "(missed {} events)",
                v.get("missed").and_then(Value::as_u64).unwrap_or(0)
            )),
            _ => None,
        };
        out.extend(line);
        out
    }
}

pub(crate) async fn watch(session: Option<String>, json_out: bool) -> Result<()> {
    let body = json!({ "session": session.unwrap_or_default() });
    let mut view = WatchView::default();
    let got = desk::request_within("subscribe", body, None, &mut |v| {
        if matches!(s(v, "kind"), "ping" | "subscribed") {
            return;
        }
        if json_out {
            println!("{v}");
        } else {
            for line in view.lines(v) {
                println!("{line}");
            }
        }
        let _ = std::io::stdout().flush();
    })
    .await;
    match got {
        Ok(_) => Ok(()),
        Err(DeskError::Closed) => {
            eprintln!("the window closed");
            Ok(())
        }
        Err(e) => bail!(e),
    }
}

pub(crate) async fn approvals(op: &str, body: Value) -> Result<Value> {
    match desk::request(op, body.clone()).await {
        Ok(v) => Ok(v),
        Err(DeskError::NotOpen) => match desk::call_serve(op, body, Duration::from_secs(5)).await {
            Err(DeskError::ServeNotRunning) => bail!("neither Parzi nor parzi serve is running"),
            other => other.map_err(|e| anyhow::anyhow!(e)),
        },
        Err(e) => bail!(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_prints_whole_lines_per_session() {
        let mut w = WatchView::default();
        let text = |sid: &str, t: &str| json!({ "kind": "text", "session": sid, "text": t });
        assert!(w.lines(&text("aaaaaaaaaa", "hel")).is_empty());
        assert!(w.lines(&text("bbbbbbbbbb", "other")).is_empty());
        assert_eq!(
            w.lines(&text("aaaaaaaaaa", "lo\nwor")),
            vec!["aaaaaaaa hello"]
        );
        let done = w.lines(&json!({ "kind": "done", "session": "aaaaaaaaaa", "turns": 1 }));
        assert_eq!(done, vec!["aaaaaaaa wor", "aaaaaaaa finished"]);
        let ask = w.lines(&json!({
            "kind": "approval", "session": "bbbbbbbbbb", "key": "1234567890", "label": "Running `ls`",
        }));
        assert_eq!(
            ask,
            vec![
                "bbbbbbbb other",
                "bbbbbbbb needs approval: Running `ls` (parzi approval allow 12345678)"
            ]
        );
        assert!(w
            .lines(&json!({ "kind": "reasoning", "session": "x", "text": "hm" }))
            .is_empty());
    }

    #[test]
    fn the_tab_in_front_reads_from_either_shape() {
        assert_eq!(active_tab(&json!({"active": "t1"})), "t1");
        assert_eq!(
            active_tab(&json!({"active": {"id": "t2", "kind": "page", "title": "x"}})),
            "t2"
        );
        assert_eq!(active_tab(&json!({"active": null})), "");
    }

    #[test]
    fn send_asks_the_window_to_stream() {
        let body = send_body("new", "hi", None, "C:/src", true, "high");
        assert_eq!(body["stream"], true);
        assert_eq!(body["model"], "", "never `auto`");
        assert_eq!(body["yes"], true);
        let page = page_body(Some("tab-1".into()), json!({ "text": "Go" }));
        assert_eq!(page["tab"], "tab-1");
        assert_eq!(page["args"]["text"], "Go");
        assert!(page_body(None, json!({})).get("tab").is_none());
    }
}
