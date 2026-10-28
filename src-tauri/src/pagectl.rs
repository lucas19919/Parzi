use std::time::{Duration, Instant};

use parzi_runtime::desk::{WAIT_DEFAULT_MS, WAIT_MAX_MS};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Webview};

// A field's typed value is never shown to the agent or matched against when
// it holds a secret: a password (also after a show-password toggle flips its
// type), a one-time code or a card number. Matching would be an oracle.
const SECRET: &str = r#"
  const secret = (e) => e.tagName === "INPUT" && (String(e.type).toLowerCase() === "password" || /password|one-time-code|cc-number|cc-csc/i.test(e.getAttribute("autocomplete") || ""));
  const typed = (e) => (secret(e) ? "" : e.value);
"#;

// The page and its same-origin frames (a cross-origin frame's document is
// out of reach and skipped), and the lookups every action shares.
const DOM: &str = r#"
  const docs = () => {
    const out = [document];
    for (let i = 0; i < out.length && out.length < 20; i++) {
      for (const f of out[i].querySelectorAll("iframe,frame")) {
        try {
          const d = f.contentDocument;
          if (d && d.documentElement) out.push(d);
        } catch (err) {}
      }
    }
    return out;
  };
  const all = (sel) => docs().flatMap((d) => {
    try {
      return [...d.querySelectorAll(sel)];
    } catch (err) {
      return [];
    }
  });
  const view = (el) => (el && el.ownerDocument && el.ownerDocument.defaultView) || window;
  const shown = (el) => !!el && el.getClientRects().length > 0 && view(el).getComputedStyle(el).visibility !== "hidden";
  const name = (el) => (el.innerText || typed(el) || el.getAttribute("aria-label") || el.placeholder || el.title || el.name || "").trim().replace(/\s+/g, " ").slice(0, 80);
  const label = (el) => name(el).toLowerCase();
  const pick = (list, want) => {
    const w = String(want).trim().toLowerCase();
    return list.find((el) => label(el) === w) || list.find((el) => label(el).includes(w));
  };
  const valid = (sel) => {
    try {
      document.querySelector(sel);
      return true;
    } catch (err) {
      return false;
    }
  };
  const bySelector = (sel) => (valid(sel) ? all(sel).find(shown) || all(sel)[0] || null : undefined);
  const deepActive = () => {
    let el = document.activeElement;
    while (el && (el.tagName === "IFRAME" || el.tagName === "FRAME")) {
      try {
        el = el.contentDocument.activeElement;
      } catch (err) {
        break;
      }
    }
    return el;
  };
  const labelled = (el) => {
    const d = el.ownerDocument;
    const by = el.id && d.querySelector(`label[for="${CSS.escape(el.id)}"]`);
    const wrap = el.closest && el.closest("label");
    return ((by && by.innerText) || (wrap && wrap.innerText) || el.getAttribute("aria-label") || "").trim().toLowerCase();
  };
  const field = (sel) => {
    if (a.selector) {
      const el = bySelector(a.selector);
      return el === undefined ? { error: "that selector is not valid CSS" } : { el };
    }
    const list = all(sel).filter(shown);
    if (a.field) {
      const w = String(a.field).trim().toLowerCase();
      return { el: pick(list, a.field) || list.find((el) => labelled(el).includes(w)) || list.find((el) => String(el.name || "").toLowerCase() === w) };
    }
    const focused = deepActive();
    return { el: list.includes(focused) ? focused : list[0] };
  };
  const KEYS = {
    enter: ["Enter", 13], tab: ["Tab", 9], escape: ["Escape", 27], esc: ["Escape", 27],
    backspace: ["Backspace", 8], delete: ["Delete", 46], space: [" ", 32],
    arrowup: ["ArrowUp", 38], arrowdown: ["ArrowDown", 40], arrowleft: ["ArrowLeft", 37], arrowright: ["ArrowRight", 39],
    up: ["ArrowUp", 38], down: ["ArrowDown", 40], left: ["ArrowLeft", 37], right: ["ArrowRight", 39],
    home: ["Home", 36], end: ["End", 35], pageup: ["PageUp", 33], pagedown: ["PageDown", 34],
  };
  const focusNext = (el, back) => {
    const list = [...el.ownerDocument.querySelectorAll("a[href],button,input:not([type=hidden]),select,textarea,[tabindex],[contenteditable=true]")]
      .filter((x) => shown(x) && !x.disabled && x.tabIndex >= 0);
    const i = list.indexOf(el);
    const next = list[(i + (back ? -1 : 1) + list.length) % list.length];
    if (next && next.focus) next.focus();
  };
  // One keystroke as the page sees it, then what the browser itself would
  // do next, which synthetic events never trigger: Enter in a form field
  // submits the form once, and only when no handler took the key.
  const press = (el, spec) => {
    const parts = String(spec).split("+").map((p) => p.trim()).filter(Boolean);
    const raw = parts.length > 1 ? parts.pop() : String(spec).trim();
    const mods = new Set(parts.map((p) => p.toLowerCase()));
    const known = KEYS[raw.toLowerCase()];
    if (!known && [...raw].length !== 1) return { error: `unknown key "${raw}"` };
    const key = known ? known[0] : raw;
    const w = view(el);
    const o = {
      key,
      code: key === " " ? "Space" : known ? key : /^[a-z]$/i.test(raw) ? `Key${raw.toUpperCase()}` : /^[0-9]$/.test(raw) ? `Digit${raw}` : "",
      keyCode: known ? known[1] : raw.toUpperCase().charCodeAt(0),
      which: known ? known[1] : raw.toUpperCase().charCodeAt(0),
      bubbles: true, cancelable: true, composed: true,
      ctrlKey: mods.has("ctrl") || mods.has("control"), shiftKey: mods.has("shift"),
      altKey: mods.has("alt"), metaKey: mods.has("meta") || mods.has("cmd"),
    };
    const chord = o.ctrlKey || o.altKey || o.metaKey;
    const down = el.dispatchEvent(new w.KeyboardEvent("keydown", o));
    let submitted = false;
    if (down && !chord) {
      const printable = key.length === 1 || key === "Enter";
      const kept = printable ? el.dispatchEvent(new w.KeyboardEvent("keypress", o)) : true;
      const text = el.isContentEditable || el.tagName === "TEXTAREA" || (el.tagName === "INPUT" && !["checkbox", "radio", "submit", "button"].includes(String(el.type)));
      if (kept && key === "Enter" && el.tagName === "INPUT" && el.form) {
        if (el.form.requestSubmit) el.form.requestSubmit();
        else el.form.submit();
        submitted = true;
      } else if (kept && (key === "Enter" || key === " ") && (el.tagName === "BUTTON" || el.tagName === "A" || el.getAttribute("role") === "button")) {
        el.click();
      } else if (kept && key === "Tab") {
        focusNext(el, o.shiftKey);
      } else if (kept && text && key.length === 1) {
        el.ownerDocument.execCommand("insertText", false, key);
      } else if (kept && text && key === "Enter" && el.tagName !== "INPUT") {
        el.ownerDocument.execCommand("insertParagraph");
      } else if (kept && text && (key === "Backspace" || key === "Delete")) {
        el.ownerDocument.execCommand(key === "Backspace" ? "delete" : "forwardDelete");
      }
    }
    el.dispatchEvent(new w.KeyboardEvent("keyup", o));
    return { submitted, handled: !down };
  };
"#;

const READ: &str = r#"
  const texts = [];
  let frames = 0;
  docs().forEach((d, i) => {
    if (!d.body) return;
    if (i > 0) {
      frames += 1;
      texts.push(`\n[frame: ${d.title || ""}]\n`);
    }
    texts.push(d.body.innerText || "");
  });
  const full = texts.join("");
  const offset = Math.max(0, Math.floor(Number(a.offset) || 0));
  const limit = Math.min(20000, Math.max(200, Math.floor(Number(a.limit) || 4000)));
  const text = full.slice(offset, offset + limit);
  const controls = all("a[href],button,input:not([type=hidden]),textarea,select,[role=button],[role=link],[role=tab],[role=menuitem],[role=checkbox],[role=option],[role=textbox],[contenteditable=true],[contenteditable='']")
    .filter(shown)
    .slice(0, 150)
    .map((el) => {
      const tag = el.tagName.toLowerCase();
      const kind = tag === "input" && el.type ? `[${el.type}]` : "";
      const id = el.id ? ` #${el.id}` : "";
      return `${tag}${kind}${id} ${name(el)}`.trim();
    });
  return { title: document.title, url: location.href, text, offset, next: offset + text.length, total: full.length, frames, controls };
"#;

const CLICK: &str = r#"
  let el = null;
  if (a.selector) {
    el = bySelector(a.selector);
    if (el === undefined) return { error: "that selector is not valid CSS" };
  } else if (a.text) {
    el = pick(all("a,button,input[type=submit],input[type=button],input[type=checkbox],input[type=radio],[role=button],[role=link],[role=tab],[role=menuitem],[role=option],[role=checkbox],[role=switch],label,summary,[onclick]").filter(shown), a.text);
  } else {
    return { error: "say what to click: its text or a selector" };
  }
  if (!shown(el)) return { error: "nothing on the page matches" };
  el.scrollIntoView({ block: "center", inline: "center" });
  const r = el.getBoundingClientRect();
  const w = view(el);
  const at = { bubbles: true, cancelable: true, composed: true, view: w, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, button: 0 };
  // The pointer and mouse sequence a real click makes: menus and React
  // handlers often act on pointerdown or mousedown, not click.
  const pointer = (t) => el.dispatchEvent(new w.PointerEvent(t, { ...at, pointerId: 1, pointerType: "mouse", isPrimary: true }));
  const mouse = (t) => el.dispatchEvent(new w.MouseEvent(t, at));
  pointer("pointerover");
  mouse("mouseover");
  pointer("pointerdown");
  mouse("mousedown");
  if (el.focus) el.focus({ preventScroll: true });
  pointer("pointerup");
  mouse("mouseup");
  const shownLabel = label(el).slice(0, 60);
  el.click();
  return { done: `clicked ${el.tagName.toLowerCase()}`, label: shownLabel };
"#;

const TYPE: &str = r#"
  const found = field("input:not([type=hidden]):not([type=submit]):not([type=button]):not([type=checkbox]):not([type=radio]),textarea,[contenteditable=true],[contenteditable=''],[role=textbox]");
  if (found.error) return found;
  let el = found.el;
  const takesText = (x) => !!x && (x.isContentEditable || x.tagName === "INPUT" || x.tagName === "TEXTAREA");
  if (el && !takesText(el) && el.querySelector) el = el.querySelector("[contenteditable=true],[contenteditable=''],input,textarea") || el;
  if (!shown(el)) return { error: "no field on the page matches" };
  if (el.tagName === "SELECT") return { error: "that is a dropdown: use select" };
  if (!takesText(el)) return { error: "that element does not take text" };
  el.scrollIntoView({ block: "center" });
  el.focus();
  const w = view(el);
  const text = String(a.text == null ? "" : a.text);
  if (el.isContentEditable) {
    const d = el.ownerDocument;
    const range = d.createRange();
    range.selectNodeContents(el);
    if (a.append) range.collapse(false);
    const sel = d.getSelection();
    sel.removeAllRanges();
    sel.addRange(range);
    // Rich editors (ProseMirror, Draft, Lexical) keep their own model:
    // insertText edits the way typing does and fires a trusted input
    // event they read back; textContent would be lost on their next render.
    if (!d.execCommand("insertText", false, text)) {
      if (!a.append) el.textContent = "";
      el.append(text);
      el.dispatchEvent(new w.InputEvent("input", { bubbles: true, inputType: "insertText", data: text }));
    }
  } else {
    const proto = el.tagName === "TEXTAREA" ? w.HTMLTextAreaElement.prototype : w.HTMLInputElement.prototype;
    // Through the prototype's setter: React tracks the element's own one.
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, a.append ? el.value + text : text);
    el.dispatchEvent(new w.InputEvent("input", { bubbles: true, inputType: "insertText", data: text }));
    el.dispatchEvent(new w.Event("change", { bubbles: true }));
  }
  let how = "";
  if (a.submit) {
    const sent = press(el, "Enter");
    how = sent.submitted ? " and submitted the form" : sent.handled ? " and pressed Enter (the page took it)" : " and pressed Enter";
  }
  return { done: `typed into ${el.tagName.toLowerCase()}${how}`, label: label(el).slice(0, 60) };
"#;

const PRESS: &str = r#"
  let el = null;
  if (a.selector) {
    el = bySelector(a.selector);
    if (el === undefined) return { error: "that selector is not valid CSS" };
    if (!el) return { error: "nothing on the page matches" };
    if (el.focus) el.focus({ preventScroll: true });
  } else {
    el = deepActive() || document.body;
  }
  if (!a.key) return { error: "say which key" };
  const sent = press(el, a.key);
  if (sent.error) return sent;
  return { done: `pressed ${a.key}${sent.submitted ? " and submitted the form" : ""}${sent.handled ? " (the page took it)" : ""}`, label: label(el).slice(0, 60) };
"#;

const SELECT: &str = r#"
  const found = field("select");
  if (found.error) return found;
  const el = found.el;
  if (!shown(el) || el.tagName !== "SELECT") return { error: "no dropdown on the page matches; for a custom menu click the option's text" };
  const want = String(a.option == null ? "" : a.option).trim().toLowerCase();
  const opts = [...el.options];
  const opt = opts.find((o) => o.value.toLowerCase() === want || o.text.trim().toLowerCase() === want) || opts.find((o) => o.text.toLowerCase().includes(want));
  if (!want || !opt) return { error: `no option matches; it offers: ${opts.slice(0, 20).map((o) => o.text.trim()).join(", ")}` };
  el.focus();
  const w = view(el);
  Object.getOwnPropertyDescriptor(w.HTMLSelectElement.prototype, "value").set.call(el, opt.value);
  el.dispatchEvent(new w.Event("input", { bubbles: true }));
  el.dispatchEvent(new w.Event("change", { bubbles: true }));
  return { done: "chose", label: opt.text.trim().slice(0, 60) };
"#;

const SCROLL: &str = r#"
  if (a.selector) {
    const el = bySelector(a.selector);
    if (el === undefined) return { error: "that selector is not valid CSS" };
    if (!el) return { error: "nothing on the page matches" };
    el.scrollIntoView({ block: "center" });
    return { done: "scrolled to", label: label(el).slice(0, 60) };
  }
  const s = document.scrollingElement || document.documentElement;
  const to = String(a.to || "down").toLowerCase();
  if (to === "top") window.scrollTo(0, 0);
  else if (to === "bottom") window.scrollTo(0, s.scrollHeight);
  else if (to === "up" || to === "down") window.scrollBy(0, (to === "up" ? -0.8 : 0.8) * window.innerHeight);
  else return { error: "to is down, up, top or bottom" };
  return { done: `scrolled ${to}: at ${Math.round(window.scrollY)} of ${Math.max(0, Math.round(s.scrollHeight - window.innerHeight))}px` };
"#;

const WAIT: &str = r#"
  if (a.selector) {
    if (!valid(a.selector)) return { error: "that selector is not valid CSS" };
    return { found: all(a.selector).some(shown) };
  }
  if (a.text) {
    const w = String(a.text).toLowerCase();
    return { found: docs().some((d) => d.body && String(d.body.innerText || "").toLowerCase().includes(w)) };
  }
  return { error: "say what to wait for: a selector or a text" };
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

/// Up to 10s for the page to finish loading. Together with the 10s for the
/// tab to come up and the 5s script limit this stays under
/// `parzi_runtime::desk::PAGE_STEP`.
async fn settle(wv: &Webview) {
    for _ in 0..20 {
        let state = run(wv, "document.readyState".into()).await.ok();
        if state.as_ref().and_then(Value::as_str) == Some("complete") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn script(args: &Value, body: &str) -> String {
    // The args go in as JSON, which is a JavaScript literal: page text the
    // agent passes on can never break out of it.
    format!("(() => {{ const a = {args}; {SECRET} {DOM} {body} }})()")
}

fn answer(v: Value) -> Result<Value, String> {
    match v.get("error").and_then(Value::as_str) {
        Some(e) => Err(e.to_string()),
        None if v.is_object() => Ok(v),
        None => Err("the page did not answer".into()),
    }
}

/// Title, URL, a window of the text (`offset`, `limit`) and the controls.
pub async fn read(app: &AppHandle, tab: &str, window: &Value) -> Result<Value, String> {
    let wv = webview(app, tab).await?;
    settle(&wv).await;
    answer(run(&wv, script(window, READ)).await?)
}

/// click, type, press, select or scroll; `{done, label}` on success.
pub async fn act(app: &AppHandle, tab: &str, kind: &str, args: &Value) -> Result<Value, String> {
    let body = match kind {
        "click" => CLICK,
        "type" => TYPE,
        "press" => PRESS,
        "select" => SELECT,
        "scroll" => SCROLL,
        _ => return Err(format!("unknown page action `{kind}`")),
    };
    let wv = webview(app, tab).await?;
    settle(&wv).await;
    answer(run(&wv, script(args, body)).await?)
}

/// Polls until the selector or text shows up (or, with `gone`, leaves).
pub async fn wait(app: &AppHandle, tab: &str, args: &Value) -> Result<Value, String> {
    let limit = Duration::from_millis(
        args.get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(WAIT_DEFAULT_MS)
            .clamp(100, WAIT_MAX_MS),
    );
    let gone = args.get("gone").and_then(Value::as_bool).unwrap_or(false);
    let wv = webview(app, tab).await?;
    let started = Instant::now();
    let probe = script(args, WAIT);
    loop {
        // A page mid-navigation fails the script: that is "not yet".
        if let Ok(v) = run(&wv, probe.clone()).await {
            let v = answer(v)?;
            if v.get("found").and_then(Value::as_bool) == Some(!gone) {
                let secs = started.elapsed().as_secs_f32();
                return Ok(json!({
                    "done": format!("{} after {secs:.1}s", if gone { "gone" } else { "there" }),
                }));
            }
        }
        if started.elapsed() >= limit {
            return Err(format!(
                "{} after {}s",
                if gone { "still there" } else { "not there" },
                limit.as_secs()
            ));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
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

    fn every_script() -> Vec<(&'static str, String)> {
        let a = json!({ "text": "x" });
        [
            ("read", READ),
            ("click", CLICK),
            ("type", TYPE),
            ("press", PRESS),
            ("select", SELECT),
            ("scroll", SCROLL),
            ("wait", WAIT),
        ]
        .into_iter()
        .map(|(n, body)| (n, script(&a, body)))
        .collect()
    }

    #[test]
    fn typed_values_go_through_the_secret_guard() {
        // Every place a field value could reach the agent reads it via typed().
        for (name, s) in every_script() {
            assert!(
                s.contains(r#"const typed = (e) => (secret(e) ? "" : e.value);"#),
                "{name}"
            );
            assert!(s.contains(r#"String(e.type).toLowerCase() === "password""#));
            assert_eq!(s.matches("e.value").count(), 1, "{name}");
        }
        assert!(READ.contains("input:not([type=hidden])"));
    }

    #[test]
    fn rich_editors_get_insert_text_and_a_form_submits_once() {
        let t = TYPE;
        let insert = t.find("execCommand(\"insertText\"").unwrap();
        let fallback = t.find("el.textContent = \"\"").unwrap();
        assert!(insert < fallback, "textContent only when insertText failed");
        assert!(!t.contains("requestSubmit"), "type submits through press()");
        assert!(!t.contains("KeyboardEvent"), "{t}");
        assert_eq!(DOM.matches("requestSubmit()").count(), 1);
        // The submit waits on the keydown and keypress no handler cancelled.
        let submit = DOM.find("el.form.requestSubmit()").unwrap();
        let gate = DOM.find("if (down && !chord)").unwrap();
        assert!(gate < submit);
    }

    #[test]
    fn args_are_a_json_literal() {
        let evil = json!({ "text": "\"}); alert(1); ({\"", "field": "`${x}`</script>" });
        let s = script(&evil, TYPE);
        assert!(s.starts_with("(() => { const a = {"), "{s}");
        assert!(s.contains(&evil.to_string()));
        assert!(s.ends_with("})()"));
    }

    #[test]
    fn scripts_parse_as_javascript() {
        // Where node is installed, every script must at least parse.
        let Ok(out) = std::process::Command::new("node").arg("--version").output() else {
            return;
        };
        if !out.status.success() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("parzi-pagectl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, s) in every_script() {
            let path = dir.join(format!("{name}.js"));
            std::fs::write(&path, &s).unwrap();
            let check = std::process::Command::new("node")
                .arg("--check")
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                check.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&check.stderr)
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
