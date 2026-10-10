use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Manager, Webview};

// A field's typed value is never shown to the agent or matched against when
// it holds a secret: a password (also after a show-password toggle flips its
// type), a one-time code or a card number. Matching would be an oracle.
const SECRET: &str = r#"
  const secret = (e) => e.tagName === "INPUT" && (String(e.type).toLowerCase() === "password" || /password|one-time-code|cc-number|cc-csc/i.test(e.getAttribute("autocomplete") || ""));
  const typed = (e) => (secret(e) ? "" : e.value);
"#;

const READ: &str = r#"
  const shown = (e) => e.getClientRects().length > 0 && getComputedStyle(e).visibility !== "hidden";
  const name = (e) => (e.innerText || typed(e) || e.getAttribute("aria-label") || e.placeholder || e.title || e.name || "").trim().replace(/\s+/g, " ").slice(0, 80);
  const controls = [...document.querySelectorAll("a[href],button,input:not([type=hidden]),textarea,select,[role=button],[role=link],[contenteditable=true]")]
    .filter(shown)
    .slice(0, 80)
    .map((e) => `${e.tagName.toLowerCase()}${e.type ? `[${e.type}]` : ""} ${name(e)}`.trim());
  return {
    title: document.title,
    url: location.href,
    text: (document.body ? document.body.innerText : "").slice(0, 20000),
    controls,
  };
"#;

const FIND: &str = r#"
  const shown = (e) => e && e.getClientRects().length > 0 && getComputedStyle(e).visibility !== "hidden";
  const label = (e) => (e.innerText || typed(e) || e.getAttribute("aria-label") || e.placeholder || e.title || e.name || "").trim().toLowerCase();
  const pick = (all, want) => {
    const w = want.trim().toLowerCase();
    return all.find((e) => label(e) === w) || all.find((e) => label(e).includes(w));
  };
  const bySelector = (s) => {
    try {
      return document.querySelector(s);
    } catch {
      return undefined;
    }
  };
"#;

const CLICK: &str = r#"
  let el = null;
  if (a.selector) {
    el = bySelector(a.selector);
    if (el === undefined) return { error: "that selector is not valid CSS" };
  } else if (a.text) {
    const all = [...document.querySelectorAll("a,button,input[type=submit],input[type=button],[role=button],[role=link],[role=tab],[role=menuitem],[role=checkbox],label,summary")].filter(shown);
    el = pick(all, a.text);
  }
  if (!shown(el)) return { error: "nothing on the page matches" };
  el.scrollIntoView({ block: "center" });
  if (el.focus) el.focus();
  el.click();
  return { done: `clicked ${el.tagName.toLowerCase()} ${label(el).slice(0, 60)}`.trim() };
"#;

const TYPE: &str = r#"
  let el = null;
  if (a.selector) {
    el = bySelector(a.selector);
    if (el === undefined) return { error: "that selector is not valid CSS" };
  } else {
    const all = [...document.querySelectorAll("input:not([type=hidden]):not([type=submit]):not([type=button]),textarea,[contenteditable=true]")].filter(shown);
    const labelled = (e) => {
      const l = e.id && document.querySelector(`label[for="${CSS.escape(e.id)}"]`);
      return l ? l.innerText.trim().toLowerCase() : "";
    };
    if (a.field) {
      const w = a.field.trim().toLowerCase();
      el = pick(all, a.field) || all.find((e) => labelled(e).includes(w));
    } else {
      el = all.includes(document.activeElement) ? document.activeElement : all[0];
    }
  }
  if (!shown(el)) return { error: "no field on the page matches" };
  el.scrollIntoView({ block: "center" });
  el.focus();
  if (el.isContentEditable) {
    el.textContent = a.text;
  } else {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, a.text);
  }
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
  if (a.submit) {
    for (const type of ["keydown", "keypress", "keyup"]) {
      el.dispatchEvent(new KeyboardEvent(type, { key: "Enter", code: "Enter", keyCode: 13, which: 13, bubbles: true }));
    }
    if (el.form) el.form.requestSubmit ? el.form.requestSubmit() : el.form.submit();
  }
  return { done: `typed into ${el.tagName.toLowerCase()} ${label(el).slice(0, 60)}`.trim() };
"#;

async fn webview(app: &AppHandle, tab: &str) -> Result<Webview, String> {
    let label = crate::browser::page_label(tab)?;
    if app.get_webview(&label).is_none() {
        crate::browser::revive(app, tab);
    }
    for _ in 0..40 {
        if let Some(wv) = app.get_webview(&label) {
            return Ok(wv);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err("the tab did not load".into())
}

async fn run(wv: &Webview, script: String) -> Result<Value, String> {
    let wv = wv.clone();
    let raw = tauri::async_runtime::spawn_blocking(move || script_result(&wv, script))
        .await
        .map_err(|e| e.to_string())??;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn script_result(wv: &Webview, script: String) -> Result<String, String> {
    crate::dwm::run_page_script(wv, script)
}

#[cfg(not(windows))]
fn script_result(_: &Webview, _: String) -> Result<String, String> {
    Err("page control needs Windows".into())
}

async fn settle(wv: &Webview) {
    for _ in 0..20 {
        let state = run(wv, "document.readyState".into()).await.ok();
        if state.as_ref().and_then(Value::as_str) == Some("complete") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub async fn read(app: &AppHandle, tab: &str) -> Result<Value, String> {
    let wv = webview(app, tab).await?;
    settle(&wv).await;
    run(&wv, read_script()).await
}

fn read_script() -> String {
    format!("(() => {{ {SECRET} {READ} }})()")
}

fn act_script(args: &Value, body: &str) -> String {
    format!("(() => {{ const a = {args}; {SECRET} {FIND} {body} }})()")
}

pub async fn act(app: &AppHandle, tab: &str, kind: &str, args: &Value) -> Result<String, String> {
    let body = match kind {
        "click" => CLICK,
        "type" => TYPE,
        _ => return Err(format!("unknown page action `{kind}`")),
    };
    let wv = webview(app, tab).await?;
    settle(&wv).await;
    let answer = run(&wv, act_script(args, body)).await?;
    match (answer.get("done"), answer.get("error")) {
        (Some(done), _) => Ok(done.as_str().unwrap_or("done").to_string()),
        (None, Some(e)) => Err(e.as_str().unwrap_or("failed").to_string()),
        _ => Err("the page did not answer".into()),
    }
}

pub async fn shot(app: &AppHandle, tab: &str) -> Result<String, String> {
    let wv = webview(app, tab).await?;
    settle(&wv).await;
    #[cfg(windows)]
    {
        use base64::Engine as _;
        let jpeg = tauri::async_runtime::spawn_blocking(move || crate::dwm::capture_page(&wv))
            .await
            .map_err(|e| e.to_string())??;
        Ok(base64::engine::general_purpose::STANDARD.encode(jpeg))
    }
    #[cfg(not(windows))]
    {
        let _ = wv;
        Err("screenshots need Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_values_go_through_the_secret_guard() {
        // Every place a field value could reach the agent reads it via typed().
        for script in [read_script(), act_script(&serde_json::json!({}), CLICK)] {
            assert!(script.contains(r#"const typed = (e) => (secret(e) ? "" : e.value);"#));
            assert!(script.contains(r#"String(e.type).toLowerCase() === "password""#));
            assert_eq!(script.matches("e.value").count(), 1, "{script}");
        }
        assert!(READ.contains("input:not([type=hidden])"));
    }
}
