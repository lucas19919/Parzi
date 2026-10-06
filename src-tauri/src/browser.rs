use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde_json::json;
use tauri::webview::{NewWindowResponse, WebviewBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewUrl};

const BROWSER: &str = "parzi://browser";
const PAGE_PREFIX: &str = "page-";
const OFFSCREEN: f64 = -30_000.0;

static OP: Mutex<()> = Mutex::new(());
static GEN: AtomicU64 = AtomicU64::new(0);
static TABS: Mutex<BTreeMap<String, (String, Url)>> = Mutex::new(BTreeMap::new());
static REFIT: Mutex<Option<Fit>> = Mutex::new(None);
static ARMED: AtomicBool = AtomicBool::new(false);
static SHOWN: Mutex<String> = Mutex::new(String::new());

struct Fit {
    gen: u64,
    label: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
}

fn locked<T>(m: &'static Mutex<T>) -> MutexGuard<'static, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn stale(gen: u64) -> bool {
    GEN.load(Ordering::Relaxed) != gen
}

fn tell(app: &AppHandle, event: &str, payload: serde_json::Value) {
    let _ = app.emit_to("main", event, payload);
}

pub fn page_label(tab: &str) -> Result<String, String> {
    if tab.is_empty() {
        return Err("no tab".into());
    }
    let safe: String = tab
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(format!("{PAGE_PREFIX}{safe}"))
}

fn http_page(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|e| e.to_string())?;
    if url.scheme() == "http" || url.scheme() == "https" {
        Ok(url)
    } else {
        Err("only http and https".into())
    }
}

fn page_bounds(
    window: &tauri::Window,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Option<(f64, f64, f64, f64)> {
    let scale = window.scale_factor().unwrap_or(1.0).max(0.01);
    let inner = window.inner_size().ok()?;
    let max_w = f64::from(inner.width) / scale;
    let max_h = f64::from(inner.height) / scale;
    let x = x.clamp(0.0, max_w);
    let y = y.clamp(0.0, max_h);
    // Allow a 2px bleed past the window edge: the UI overscans the slot
    // rect outward so no background sliver shows around the page.
    let width = width.min((max_w - x + 2.0).max(0.0));
    let height = height.min((max_h - y + 2.0).max(0.0));
    if width < 8.0 || height < 8.0 {
        None
    } else {
        Some((x, y, width, height))
    }
}

fn backdrop() -> tauri::window::Color {
    let hex = parzi_core::theme::Theme::load()
        .map(|t| t.colors.stage)
        .unwrap_or_default();
    let digits = hex.trim().trim_start_matches('#');
    let channel = |i: usize| {
        digits
            .get(i..i + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
    };
    match (digits.len(), channel(0), channel(2), channel(4)) {
        (6 | 8, Some(r), Some(g), Some(b)) => tauri::window::Color(r, g, b, 255),
        _ => tauri::window::Color(11, 11, 16, 255),
    }
}

fn hide_pages(app: &AppHandle, keep: &str) {
    for (label, wv) in app.webviews() {
        if label.starts_with(PAGE_PREFIX) && label != keep {
            let _ = wv.hide();
        }
    }
}

#[tauri::command]
pub fn browser_show(
    app: AppHandle,
    tab: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    url: &str,
) -> Result<(), String> {
    let label = page_label(&tab)?;
    let gen = GEN.fetch_add(1, Ordering::Relaxed) + 1;
    let Some(window) = app.get_window("main") else {
        tracing::debug!("browser_show: no main window");
        return Ok(());
    };
    let Some((x, y, width, height)) = page_bounds(&window, x, y, width, height) else {
        hide_pages(&app, "");
        return Ok(());
    };
    if app.get_webview(&label).is_none() {
        let url = http_page(url).inspect_err(|_| hide_pages(&app, ""))?;
        locked(&TABS).entry(label.clone()).or_insert((tab, url));
    }
    let job = Fit {
        gen,
        label,
        x,
        y,
        width,
        height,
        scale: window.scale_factor().unwrap_or(1.0),
    };
    std::thread::spawn(move || mount_page(&app, &window, job));
    Ok(())
}

#[tauri::command]
pub fn browser_prepare(app: AppHandle, tab: String, url: &str) -> Result<(), String> {
    let label = page_label(&tab)?;
    if app.get_webview(&label).is_some() {
        return Ok(());
    }
    let url = http_page(url)?;
    locked(&TABS).entry(label.clone()).or_insert((tab, url));
    let Some(window) = app.get_window("main") else {
        return Ok(());
    };
    std::thread::spawn(move || {
        let _op = locked(&OP);
        if app.get_webview(&label).is_some() || !locked(&TABS).contains_key(&label) {
            return;
        }
        let scale = window.scale_factor().unwrap_or(1.0).max(0.01);
        let Ok(inner) = window.inner_size() else {
            return;
        };
        let job = Fit {
            gen: GEN.load(Ordering::Relaxed),
            label,
            x: OFFSCREEN,
            y: OFFSCREEN,
            width: f64::from(inner.width) / scale,
            height: f64::from(inner.height) / scale,
            scale,
        };
        if let Some(wv) = open_page(&app, &window, &job) {
            if *locked(&SHOWN) != job.label {
                let _ = wv.hide();
            }
        }
    });
    Ok(())
}

fn mount_page(app: &AppHandle, window: &tauri::Window, job: Fit) {
    let _op = locked(&OP);
    if stale(job.gen) {
        return;
    }
    let wv = if let Some(wv) = app.get_webview(&job.label) {
        wv
    } else {
        let Some(wv) = open_page(app, window, &job) else {
            return;
        };
        if stale(job.gen) {
            let _ = wv.hide();
            return;
        }
        wv
    };
    if fit_page(&wv, &job) {
        hide_pages(app, &job.label);
        let mut shown = locked(&SHOWN);
        if *shown != job.label {
            shown.clone_from(&job.label);
            let _ = wv.set_focus();
        }
        drop(shown);
        schedule_page_refit(app.clone(), job);
    }
}

fn open_page(app: &AppHandle, window: &tauri::Window, job: &Fit) -> Option<Webview> {
    let (tab, url) = locked(&TABS).get(&job.label).cloned()?;
    let builder = page_builder(app, &job.label, &tab, url);
    #[cfg(windows)]
    let builder = share_page_environment(app, builder);
    let wv = match window.add_child(
        builder,
        LogicalPosition::new(job.x, job.y),
        LogicalSize::new(job.width, job.height),
    ) {
        Ok(wv) => wv,
        Err(e) => {
            tracing::debug!("page webview was not created: {e}");
            return None;
        }
    };
    if !locked(&TABS).contains_key(&job.label) {
        let _ = wv.close();
        return None;
    }
    tell(app, BROWSER, json!({ "tab": tab, "loading": true }));
    #[cfg(windows)]
    watch_page(app, &wv, tab);
    Some(wv)
}

fn page_builder(app: &AppHandle, label: &str, tab: &str, url: Url) -> WebviewBuilder<tauri::Wry> {
    let (opener, open_tab, title_tab) = (app.clone(), tab.to_string(), tab.to_string());
    let builder = WebviewBuilder::new(label, WebviewUrl::External(url))
        .devtools(false)
        .background_color(backdrop())
        .on_navigation(|url| matches!(url.scheme(), "http" | "https" | "about" | "blob" | "data"))
        .on_new_window(move |url, features| {
            if features.size().is_some() {
                return NewWindowResponse::Allow;
            }
            if url.scheme() == "http" || url.scheme() == "https" {
                tell(
                    &opener,
                    "parzi://browser-open",
                    json!({ "tab": open_tab, "url": url.as_str() }),
                );
            }
            NewWindowResponse::Deny
        })
        .on_document_title_changed(move |wv, title| {
            tell(
                wv.app_handle(),
                BROWSER,
                json!({ "tab": title_tab, "title": title }),
            );
        });
    #[cfg(not(windows))]
    let builder = {
        let tab = tab.to_string();
        builder.on_page_load(move |wv, payload| {
            let body = match payload.event() {
                tauri::webview::PageLoadEvent::Started => json!({ "tab": tab, "loading": true }),
                tauri::webview::PageLoadEvent::Finished => json!({
                    "tab": tab,
                    "loading": false,
                    "url": payload.url().as_str(),
                    "canGoBack": false,
                    "canGoForward": false,
                }),
            };
            tell(wv.app_handle(), BROWSER, body);
        })
    };
    builder
}

#[cfg(windows)]
fn watch_page(app: &AppHandle, wv: &Webview, tab: String) {
    use crate::dwm::PageSignal;
    let app = app.clone();
    let hooked = crate::dwm::watch_page(wv, move |signal| match signal {
        PageSignal::Loading => tell(
            &app,
            BROWSER,
            json!({ "tab": tab, "loading": true, "blocked": 0 }),
        ),
        PageSignal::Blocked(count) => tell(&app, BROWSER, json!({ "tab": tab, "blocked": count })),
        PageSignal::Background(color) => tell(&app, BROWSER, json!({ "tab": tab, "bg": color })),
        PageSignal::Loaded { url, back, forward } => tell(
            &app,
            BROWSER,
            json!({
                "tab": tab,
                "loading": false,
                "url": url,
                "canGoBack": back,
                "canGoForward": forward,
            }),
        ),
        PageSignal::Fullscreen(on) => {
            tell(&app, BROWSER, json!({ "tab": tab, "fullscreen": on }));
        }
        PageSignal::Key(key) => {
            tell(&app, "parzi://browser-key", json!({ "key": key }));
            if key != "f11" {
                if let Some(shell) = app.get_webview("main") {
                    std::thread::spawn(move || {
                        let _ = shell.set_focus();
                    });
                }
            }
        }
    });
    if let Err(e) = hooked {
        tracing::debug!("page events not attached: {e}");
    }
}

#[cfg(windows)]
fn share_page_environment(
    app: &AppHandle,
    builder: WebviewBuilder<tauri::Wry>,
) -> WebviewBuilder<tauri::Wry> {
    let Some(main) = app.get_webview("main") else {
        tracing::debug!("page environment: no shell webview");
        return builder;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let posted = main.window().run_on_main_thread(move || {
        let ok = main
            .with_webview(|wv| {
                crate::dwm::stash_page_environment(wv.environment());
            })
            .is_ok();
        let _ = tx.send(ok);
    });
    if let Err(e) = posted {
        tracing::debug!("page environment: could not reach the UI thread: {e}");
        return builder;
    }
    let shared = match rx.recv_timeout(std::time::Duration::from_secs(2)) {
        Ok(true) => crate::dwm::take_page_environment(),
        Ok(false) => {
            tracing::debug!("page environment: shell webview unavailable");
            None
        }
        Err(_) => {
            tracing::debug!("page environment: timed out");
            None
        }
    };
    match shared {
        Some(env) => builder.with_environment(env),
        None => builder,
    }
}

fn fit_page(wv: &Webview, job: &Fit) -> bool {
    if let Err(e) = place_page(wv, job) {
        tracing::debug!("page placement failed: {e}");
        return false;
    }
    if stale(job.gen) {
        let _ = wv.hide();
        return false;
    }
    true
}

fn place_page(wv: &Webview, job: &Fit) -> Result<(), String> {
    #[cfg(windows)]
    {
        let scale = job.scale.max(0.01);
        // Overscan 1 device px per side: CSS/device rounding otherwise
        // leaves a hairline of Parzi background around the page.
        let px = |v: f64| (v * scale).round() as i32;
        crate::dwm::fit_page_surface(wv, px(job.x) - 1, px(job.y) - 1, px(job.width) + 2, px(job.height) + 2)
    }
    #[cfg(not(windows))]
    {
        wv.show().map_err(|e| e.to_string())?;
        wv.set_position(LogicalPosition::new(job.x, job.y))
            .map_err(|e| e.to_string())?;
        wv.set_size(LogicalSize::new(job.width, job.height))
            .map_err(|e| e.to_string())
    }
}

fn schedule_page_refit(app: AppHandle, job: Fit) {
    *locked(&REFIT) = Some(job);
    arm_page_refit(app);
}

fn arm_page_refit(app: AppHandle) {
    if ARMED.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let job = locked(&REFIT).take();
        ARMED.store(false, Ordering::Release);
        if let Some(job) = job {
            let _op = locked(&OP);
            if !stale(job.gen) {
                if let Some(wv) = app.get_webview(&job.label) {
                    fit_page(&wv, &job);
                }
            }
        }
        if locked(&REFIT).is_some() {
            arm_page_refit(app);
        }
    });
}

#[tauri::command]
pub async fn browser_snapshot(app: AppHandle, tab: String) -> Result<String, String> {
    let wv = app.get_webview(&page_label(&tab)?).ok_or("no such tab")?;
    #[cfg(windows)]
    {
        use base64::Engine as _;
        let jpeg = tauri::async_runtime::spawn_blocking(move || crate::dwm::capture_page(&wv))
            .await
            .map_err(|e| e.to_string())??;
        Ok(format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(jpeg)
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = wv;
        Err("snapshots need Windows".into())
    }
}

#[tauri::command]
pub fn browser_hide(app: AppHandle) {
    GEN.fetch_add(1, Ordering::Relaxed);
    locked(&SHOWN).clear();
    hide_pages(&app, "");
}

#[tauri::command]
pub fn browser_close(app: AppHandle, tab: &str) -> Result<(), String> {
    let label = page_label(tab)?;
    locked(&TABS).remove(&label);
    match app.get_webview(&label) {
        Some(wv) => wv.close().map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

#[tauri::command]
pub fn browser_navigate(app: AppHandle, tab: &str, url: &str) -> Result<(), String> {
    let label = page_label(tab)?;
    let url = http_page(url)?;
    if let Some(wv) = app.get_webview(&label) {
        return wv.navigate(url).map_err(|e| e.to_string());
    }
    match locked(&TABS).get_mut(&label) {
        Some(slot) => {
            slot.1 = url;
            Ok(())
        }
        None => Err("no such tab".into()),
    }
}

#[tauri::command]
pub fn browser_nav(app: AppHandle, tab: &str, action: String) -> Result<(), String> {
    let wv = app.get_webview(&page_label(tab)?).ok_or("no such tab")?;
    let js = match action.as_str() {
        "back" => "history.back()",
        "forward" => "history.forward()",
        "reload" => "location.reload()",
        "stop" => "window.stop()",
        _ => return Err(format!("unknown action {action}")),
    };
    #[cfg(windows)]
    {
        let _ = js;
        crate::dwm::steer_page(&wv, action)
    }
    #[cfg(not(windows))]
    {
        wv.eval(js).map_err(|e| e.to_string())
    }
}
