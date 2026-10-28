use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::webview::{NewWindowFeatures, NewWindowResponse, WebviewBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewUrl};

const BROWSER: &str = "parzi://browser";
const ASK: &str = "parzi://browser-ask";
const PAGE_PREFIX: &str = "page-";
const POPUP_PREFIX: &str = "popup-";
const OFFSCREEN: f64 = -30_000.0;

// Live page renderers are the app's biggest memory cost (~50-150MB each).
// Hidden pages give memory back first (a low memory target, then a
// suspend); only past this many does the least recently shown one close,
// keeping its URL so reopening restores it.
const PAGE_CAP: usize = 20;
const TRIM_AFTER: Duration = Duration::from_secs(30);
const SLEEP_AFTER: Duration = Duration::from_mins(5);
const REFIT_AFTER: Duration = Duration::from_millis(300);
const SWEEP_EVERY: Duration = Duration::from_secs(10);
// A page asking to open another app again this soon is ignored.
const EXTERNAL_EVERY: Duration = Duration::from_secs(1);

static GEN: AtomicU64 = AtomicU64::new(0);
static SEQ: AtomicU64 = AtomicU64::new(0);
static TABS: Mutex<BTreeMap<String, Slot>> = Mutex::new(BTreeMap::new());
static SHOWN: Mutex<String> = Mutex::new(String::new());
static LRU: Mutex<Vec<String>> = Mutex::new(Vec::new());
static RESTING: Mutex<BTreeMap<String, Rest>> = Mutex::new(BTreeMap::new());
static OFFERS: Mutex<BTreeMap<String, Offer>> = Mutex::new(BTreeMap::new());
static ASKING: Mutex<BTreeMap<String, (String, String)>> = Mutex::new(BTreeMap::new());
static WORKER: OnceLock<Sender<Work>> = OnceLock::new();

#[derive(Clone)]
struct Slot {
    tab: String,
    url: Url,
    /// Opened for an agent session: may get its own profile.
    agent: bool,
}

/// A hidden page and what was done to save its memory.
struct Rest {
    since: Instant,
    trimmed: bool,
    slept: bool,
}

/// A page asked to open a link in another app; the user decides.
struct Offer {
    tab: String,
    url: String,
    at: Instant,
}

struct Fit {
    gen: u64,
    label: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
    focus: bool,
}

/// Page work runs on one thread, in order: a resize sends ~60 shows a
/// second and only the newest matters.
enum Work {
    Show(AppHandle, Fit),
    Open(AppHandle, String, bool),
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

fn work(job: Work) {
    let tx = WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        if let Err(e) = std::thread::Builder::new()
            .name("parzi-pages".into())
            .spawn(move || run_worker(&rx))
        {
            tracing::error!("page worker did not start: {e}");
        }
        tx
    });
    if tx.send(job).is_err() {
        tracing::debug!("page worker is gone");
    }
}

fn run_worker(rx: &Receiver<Work>) {
    let mut refit: Option<(AppHandle, Fit, Instant)> = None;
    let mut app: Option<AppHandle> = None;
    let mut sweep = Instant::now() + SWEEP_EVERY;
    loop {
        let due = refit.as_ref().map_or(sweep, |(_, _, at)| (*at).min(sweep));
        match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
            Ok(first) => {
                let mut show = None;
                let mut opens = Vec::new();
                for job in std::iter::once(first).chain(rx.try_iter()) {
                    match job {
                        Work::Show(a, fit) => show = Some((a, fit)),
                        Work::Open(a, label, evict) => opens.push((a, label, evict)),
                    }
                }
                if let Some((a, fit)) = show {
                    if mount_page(&a, &fit) {
                        refit = Some((a.clone(), fit, Instant::now() + REFIT_AFTER));
                    }
                    app = Some(a);
                }
                for (a, label, evict) in opens {
                    open_background(&a, label, evict);
                    app = Some(a);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        if refit.as_ref().is_some_and(|(_, _, at)| *at <= now) {
            if let Some((a, fit, _)) = refit.take() {
                if !stale(fit.gen) {
                    if let Some(wv) = a.get_webview(&fit.label) {
                        fit_page(&wv, &fit);
                    }
                }
            }
        }
        if now >= sweep {
            sweep = now + SWEEP_EVERY;
            if let Some(a) = &app {
                rest_pages(a);
            }
        }
    }
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
    if !(url.scheme() == "http" || url.scheme() == "https") {
        return Err("only http and https".into());
    }
    if app_origin(&url) {
        return Err("that address belongs to Parzi itself".into());
    }
    Ok(url)
}

/// Tauri serves the app UI, IPC and assets on `http(s)://<scheme>.localhost`
/// (Windows) and `<scheme>://` elsewhere. A tab there would run as the app's
/// own local origin, which app commands trust.
fn app_origin(url: &Url) -> bool {
    const OWN: &[&str] = &["tauri", "ipc", "asset", "isolation"];
    if OWN.contains(&url.scheme()) {
        return true;
    }
    let host = url
        .host_str()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    host.strip_suffix(".localhost").is_some_and(|sub| {
        OWN.iter()
            .any(|own| sub == *own || sub.ends_with(&format!(".{own}")))
    })
}

fn page_navigation(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "about" | "blob" | "data") && !app_origin(url) && {
        // blob:http://tauri.localhost/... carries the app origin inside.
        url.scheme() != "blob" || Url::parse(url.path()).map_or(true, |inner| !app_origin(&inner))
    }
}

/// Where a link may go.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// Loads in the page.
    Page,
    /// Another app's link (mailto:, tel:, zoommtg:, vscode:...): opened by
    /// the OS once the user says yes.
    External,
    /// Never: local files, script URLs, Windows handlers known to run code,
    /// Parzi's own origins.
    Refuse,
}

const NEVER: &[&str] = &[
    "file",
    "javascript",
    "vbscript",
    "view-source",
    "ms-msdt",
    "search-ms",
    "search",
    "ms-search",
    "ms-officecmd",
    "ms-appinstaller",
    "ms-cxh",
    "ms-cxh-full",
    "ms-settings",
    "hcp",
    "its",
    "ms-its",
    "mk",
    "mhtml",
    "res",
    "shell",
    "jar",
    "chrome",
    "chrome-extension",
    "edge",
    "devtools",
    "intent",
    "parzi",
    "tauri",
    "ipc",
    "asset",
    "isolation",
];

pub(crate) fn classify(url: &Url) -> Target {
    let scheme = url.scheme();
    if matches!(scheme, "http" | "https" | "about" | "blob" | "data") {
        return if page_navigation(url) {
            Target::Page
        } else {
            Target::Refuse
        };
    }
    let plain = scheme.len() <= 32
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    let tidy = url.as_str().len() <= 2048
        && !url
            .as_str()
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"');
    if plain && tidy && !NEVER.contains(&scheme) {
        Target::External
    } else {
        Target::Refuse
    }
}

/// Same document, ignoring the #fragment (and URL spelling differences).
pub(crate) fn same_page(a: &str, b: &str) -> bool {
    let bare = |raw: &str| match Url::parse(raw) {
        Ok(mut url) => {
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => raw.split('#').next().unwrap_or(raw).to_string(),
    };
    bare(a) == bare(b)
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
            #[cfg(windows)]
            let _ = crate::dwm::hide_page_surface(&wv);
            let _ = wv.hide();
            locked(&RESTING).entry(label).or_insert_with(|| Rest {
                since: Instant::now(),
                trimmed: false,
                slept: false,
            });
        }
    }
}

/// Hidden pages past their time get a low memory target, later a suspend.
/// Agent pages are left awake: their session drives them in the background.
fn rest_pages(app: &AppHandle) {
    let shown = locked(&SHOWN).clone();
    let awake: Vec<String> = locked(&TABS)
        .iter()
        .filter(|(_, slot)| slot.agent)
        .map(|(label, _)| label.clone())
        .collect();
    let mut due = Vec::new();
    {
        let mut resting = locked(&RESTING);
        resting.retain(|label, _| *label != shown && app.get_webview(label).is_some());
        for (label, rest) in resting.iter_mut() {
            if awake.contains(label) {
                continue;
            }
            let idle = rest.since.elapsed();
            let deep = idle >= SLEEP_AFTER && !rest.slept;
            if deep || (idle >= TRIM_AFTER && !rest.trimmed) {
                rest.trimmed = true;
                rest.slept |= deep;
                due.push((label.clone(), deep));
            }
        }
    }
    for (label, deep) in due {
        if let Some(wv) = app.get_webview(&label) {
            #[cfg(windows)]
            crate::dwm::rest_page(&wv, deep);
            #[cfg(not(windows))]
            let _ = (wv, deep);
        }
    }
}

fn touch_lru(label: &str) {
    let mut lru = locked(&LRU);
    if let Some(i) = lru.iter().position(|l| l == label) {
        lru.remove(i);
    }
    lru.insert(0, label.to_string());
}

fn live_pages(app: &AppHandle) -> Vec<String> {
    app.webviews()
        .into_keys()
        .filter(|l| l.starts_with(PAGE_PREFIX))
        .collect()
}

/// Remembers where a page is, so a page made again (evicted, crashed)
/// opens there and not at the address it was first opened with.
fn keep_url(label: &str, url: &Url) {
    if (url.scheme() == "http" || url.scheme() == "https") && !app_origin(url) {
        if let Some(slot) = locked(&TABS).get_mut(label) {
            slot.url = url.clone();
        }
    }
}

/// Closes least-recently-shown pages past the cap, never one in `keep`.
/// Counted once up front: a closing webview may still be listed.
fn evict_lru(app: &AppHandle, keep: &[&str]) {
    let mut live = live_pages(app);
    let mut excess = live.len().saturating_sub(PAGE_CAP);
    while excess > 0 {
        let victim = {
            let lru = locked(&LRU);
            lru.iter()
                .rev()
                .find(|l| !keep.contains(&l.as_str()) && live.contains(l))
                .cloned()
                .or_else(|| live.iter().find(|l| !keep.contains(&l.as_str())).cloned())
        };
        let Some(label) = victim else {
            return;
        };
        live.retain(|l| l != &label);
        excess -= 1;
        if let Some(wv) = app.get_webview(&label) {
            if let Ok(url) = wv.url() {
                keep_url(&label, &url);
            }
            let _ = wv.close();
        }
        locked(&LRU).retain(|l| l != &label);
        locked(&RESTING).remove(&label);
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "a Tauri command's arguments")]
pub fn browser_show(
    app: AppHandle,
    tab: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    url: &str,
    owner: Option<String>,
    focus: Option<bool>,
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
        let agent = owner.is_some_and(|o| !o.is_empty());
        locked(&TABS)
            .entry(label.clone())
            .or_insert(Slot { tab, url, agent });
    }
    touch_lru(&label);
    let job = Fit {
        gen,
        label,
        x,
        y,
        width,
        height,
        scale: window.scale_factor().unwrap_or(1.0),
        focus: focus.unwrap_or(false),
    };
    work(Work::Show(app, job));
    Ok(())
}

/// UI warm-up of a background tab.
#[tauri::command]
pub fn browser_prepare(
    app: AppHandle,
    tab: String,
    url: &str,
    owner: Option<String>,
) -> Result<(), String> {
    let agent = owner.is_some_and(|o| !o.is_empty());
    prepare(app, tab, url, false, agent)
}

/// A background tab an agent is about to drive: it has to be live.
pub fn prepare_for_agent(app: AppHandle, tab: String, url: &str) -> Result<(), String> {
    prepare(app, tab, url, true, true)
}

/// Brings back an agent's tab whose page was evicted since it last drove it.
pub fn revive(app: &AppHandle, tab: &str) {
    let Ok(label) = page_label(tab) else {
        return;
    };
    if app.get_webview(&label).is_none() && locked(&TABS).contains_key(&label) {
        work(Work::Open(app.clone(), label, true));
    }
}

fn prepare(app: AppHandle, tab: String, url: &str, evict: bool, agent: bool) -> Result<(), String> {
    let label = page_label(&tab)?;
    if app.get_webview(&label).is_some() {
        return Ok(());
    }
    let url = http_page(url)?;
    locked(&TABS)
        .entry(label.clone())
        .or_insert(Slot { tab, url, agent });
    work(Work::Open(app, label, evict));
    Ok(())
}

fn open_background(app: &AppHandle, label: String, evict: bool) {
    let Some(window) = app.get_window("main") else {
        return;
    };
    if app.get_webview(&label).is_some() || !locked(&TABS).contains_key(&label) {
        return;
    }
    // A warm-up past the cap only records the entry (the page is made on
    // first show); an agent's page evicts the stalest background one.
    if !evict && live_pages(app).len() >= PAGE_CAP {
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
        focus: false,
    };
    let Some(wv) = open_page(app, &window, &job) else {
        return;
    };
    let shown = locked(&SHOWN).clone();
    if shown != job.label {
        #[cfg(windows)]
        let _ = crate::dwm::hide_page_surface(&wv);
        let _ = wv.hide();
        locked(&RESTING).entry(job.label.clone()).or_insert(Rest {
            since: Instant::now(),
            trimmed: false,
            slept: false,
        });
    }
    if evict {
        touch_lru(&job.label);
        evict_lru(app, &[&shown, &job.label]);
    }
}

fn mount_page(app: &AppHandle, job: &Fit) -> bool {
    if stale(job.gen) {
        return false;
    }
    let Some(window) = app.get_window("main") else {
        return false;
    };
    let wv = if let Some(wv) = app.get_webview(&job.label) {
        wv
    } else {
        let Some(wv) = open_page(app, &window, job) else {
            return false;
        };
        if stale(job.gen) {
            #[cfg(windows)]
            let _ = crate::dwm::hide_page_surface(&wv);
            let _ = wv.hide();
            return false;
        }
        wv
    };
    if !fit_page(&wv, job) {
        return false;
    }
    hide_pages(app, &job.label);
    let rested = locked(&RESTING).remove(&job.label);
    if rested.is_some_and(|r| r.trimmed) {
        #[cfg(windows)]
        crate::dwm::wake_page(&wv);
    }
    evict_lru(app, &[&job.label]);
    let mut shown = locked(&SHOWN);
    if *shown != job.label {
        shown.clone_from(&job.label);
        // Only when the UI says nobody is typing in it: closing the address
        // suggestions must not hand the keyboard to the page mid-edit.
        if job.focus {
            let _ = wv.set_focus();
        }
    }
    true
}

/// Where pages an agent opens keep their cookies when they must not see
/// the user's sign-ins.
#[cfg(windows)]
fn agent_profile(app: &AppHandle) -> Option<std::path::PathBuf> {
    Some(app.path().app_local_data_dir().ok()?.join("agent-pages"))
}

fn open_page(app: &AppHandle, window: &tauri::Window, job: &Fit) -> Option<Webview> {
    let slot = locked(&TABS).get(&job.label).cloned()?;
    let isolated = slot.agent && !crate::siteprefs::agent_sign_ins();
    let seed = slot.url.to_string();
    let builder = page_builder(app, &job.label, &slot.tab, slot.url, isolated);
    #[cfg(windows)]
    let builder = if isolated {
        let Some(dir) = agent_profile(app) else {
            // Never fall back to the user's profile: that is the point.
            tracing::warn!("no folder for the agent page profile; the page was not opened");
            return None;
        };
        builder.data_directory(dir)
    } else {
        share_page_environment(app, builder)
    };
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
    tell(app, BROWSER, json!({ "tab": slot.tab, "loading": true }));
    #[cfg(windows)]
    watch_page(app, &wv, slot.tab, seed);
    #[cfg(not(windows))]
    let _ = seed;
    Some(wv)
}

fn allow_navigation(app: &AppHandle, tab: &str, url: &Url) -> bool {
    match classify(url) {
        Target::Page => true,
        Target::External => {
            offer_external(app, tab, url.as_str());
            false
        }
        Target::Refuse => false,
    }
}

/// A link that opens a new window: a tab next to its page (the UI decides
/// foreground or, for an agent's page, background), a real window for
/// sign-in popups that need their opener, the OS for other apps' links.
fn new_window(
    app: &AppHandle,
    tab: &str,
    url: &Url,
    features: NewWindowFeatures,
) -> NewWindowResponse<tauri::Wry> {
    #[cfg(windows)]
    if !crate::dwm::user_opened(url.as_str()) {
        tell(
            app,
            BROWSER,
            json!({ "tab": tab, "popupBlocked": url.as_str() }),
        );
        return NewWindowResponse::Deny;
    }
    match classify(url) {
        Target::Refuse => NewWindowResponse::Deny,
        Target::External => {
            offer_external(app, tab, url.as_str());
            NewWindowResponse::Deny
        }
        Target::Page => {
            let web = matches!(url.scheme(), "http" | "https");
            if web && features.size().is_none() {
                tell(
                    app,
                    "parzi://browser-open",
                    json!({ "tab": tab, "url": url.as_str() }),
                );
                NewWindowResponse::Deny
            } else if web || matches!(url.scheme(), "about" | "blob") {
                match open_popup(app, tab, features) {
                    Some(window) => NewWindowResponse::Create { window },
                    None => NewWindowResponse::Deny,
                }
            } else {
                NewWindowResponse::Deny
            }
        }
    }
}

/// A window the page opened with `window.open` and a size, typically a
/// sign-in: it shares the page's profile (the opener must reach it) and
/// gets the same filtering, downloads and prompts.
fn open_popup(
    app: &AppHandle,
    tab: &str,
    features: NewWindowFeatures,
) -> Option<tauri::WebviewWindow> {
    let label = format!("{POPUP_PREFIX}{}", SEQ.fetch_add(1, Ordering::Relaxed) + 1);
    let sized = features.size().is_some();
    let (nav_app, nav_tab) = (app.clone(), tab.to_string());
    let (open_app, open_tab) = (app.clone(), tab.to_string());
    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::External(Url::parse("about:blank").ok()?),
    )
    .window_features(features)
    .title("Parzi")
    .background_color(backdrop())
    .on_navigation(move |url| allow_navigation(&nav_app, &nav_tab, url))
    .on_new_window(move |url, features| new_window(&open_app, &open_tab, &url, features))
    .on_document_title_changed(|window, title| {
        let _ = window.set_title(&title);
    });
    if !sized {
        builder = builder.inner_size(900.0, 700.0);
    }
    let window = match builder.build() {
        Ok(window) => window,
        Err(e) => {
            tracing::debug!("popup window was not created: {e}");
            return None;
        }
    };
    #[cfg(windows)]
    watch_popup(app, &window, tab.to_string());
    Some(window)
}

fn page_builder(
    app: &AppHandle,
    label: &str,
    tab: &str,
    url: Url,
    isolated: bool,
) -> WebviewBuilder<tauri::Wry> {
    let (nav_app, nav_tab) = (app.clone(), tab.to_string());
    let (open_app, open_tab, title_tab) = (app.clone(), tab.to_string(), tab.to_string());
    let builder = WebviewBuilder::new(label, WebviewUrl::External(url))
        .devtools(crate::siteprefs::devtools())
        // Ctrl+plus/minus/0 and Ctrl+wheel zoom a page, as in any browser
        // (Tauri turns them off by default).
        .zoom_hotkeys_enabled(true)
        .background_color(backdrop())
        .on_navigation(move |url| allow_navigation(&nav_app, &nav_tab, url))
        .on_new_window(move |url, features| new_window(&open_app, &open_tab, &url, features))
        .on_document_title_changed(move |wv, title| {
            tell(
                wv.app_handle(),
                BROWSER,
                json!({ "tab": title_tab, "title": title }),
            );
        });
    #[cfg(windows)]
    let _ = isolated;
    #[cfg(not(windows))]
    let builder = {
        // No separate profile folder on WebKit: an agent's page gets a
        // private data store instead (signed out, forgotten on close).
        let builder = builder.incognito(isolated);
        let (load_app, load_tab, load_label) = (app.clone(), tab.to_string(), label.to_string());
        let builder = builder.on_page_load(move |_wv, payload| {
            let body = match payload.event() {
                tauri::webview::PageLoadEvent::Started => {
                    json!({ "tab": load_tab, "loading": true })
                }
                tauri::webview::PageLoadEvent::Finished => {
                    let url = payload.url().as_str().to_string();
                    let (back, forward) = guess_history(&load_label, &url);
                    json!({
                        "tab": load_tab,
                        "loading": false,
                        "url": url,
                        "canGoBack": back,
                        "canGoForward": forward,
                    })
                }
            };
            tell(&load_app, BROWSER, body);
        });
        let (dl_app, dl_tab) = (app.clone(), tab.to_string());
        builder.on_download(move |_wv, event| webkit_download(&dl_app, &dl_tab, event))
    };
    builder
}

#[cfg(not(windows))]
fn webkit_download(app: &AppHandle, tab: &str, event: tauri::webview::DownloadEvent<'_>) -> bool {
    use crate::downloads::State;
    match event {
        tauri::webview::DownloadEvent::Requested { url, destination } => {
            let suggested = destination
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .filter(|n| !n.is_empty())
                .or_else(|| {
                    url.path_segments()
                        .and_then(Iterator::last)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let Some(dir) = crate::downloads::downloads_dir(app) else {
                return false;
            };
            let path = crate::downloads::target(&dir, &suggested);
            let id = crate::downloads::new_id();
            crate::downloads::started(app, &id, tab, url.as_str(), &path, 0);
            *destination = path;
            true
        }
        tauri::webview::DownloadEvent::Finished { url, path, success } => {
            if let Some(id) = crate::downloads::running_id(url.as_str()) {
                let state = if success { State::Done } else { State::Failed };
                let path = path.as_ref().and_then(|p| p.to_str());
                crate::downloads::finished(app, &id, state, path);
            }
            true
        }
        _ => true,
    }
}

/// `WebKit` reports no back/forward state here: this follows the page's own
/// loads and the toolbar's back and forward to guess it.
#[derive(Default, Debug)]
#[cfg_attr(
    windows,
    allow(dead_code, reason = "WebKit only, tested on every platform")
)]
pub(crate) struct NavGuess {
    stack: Vec<String>,
    at: usize,
    pending: Option<&'static str>,
}

#[cfg_attr(
    windows,
    allow(dead_code, reason = "WebKit only, tested on every platform")
)]
impl NavGuess {
    pub(crate) fn step(&mut self, action: &str) {
        self.pending = match action {
            "back" => Some("back"),
            "forward" => Some("forward"),
            "reload" => Some("reload"),
            _ => None,
        };
    }

    /// Records a finished load; returns (can go back, can go forward).
    pub(crate) fn loaded(&mut self, url: &str) -> (bool, bool) {
        match self.pending.take() {
            Some("back") if self.at > 0 => self.at -= 1,
            Some("forward") if self.at + 1 < self.stack.len() => self.at += 1,
            Some("reload") if !self.stack.is_empty() => {}
            _ if self.stack.get(self.at).map(String::as_str) == Some(url) => {}
            _ => {
                if self.stack.is_empty() {
                    self.at = 0;
                } else {
                    self.stack.truncate(self.at + 1);
                    self.at += 1;
                }
                self.stack.push(url.to_string());
            }
        }
        (self.at > 0, self.at + 1 < self.stack.len())
    }
}

#[cfg(not(windows))]
static GUESSES: Mutex<BTreeMap<String, NavGuess>> = Mutex::new(BTreeMap::new());

#[cfg(not(windows))]
fn guess_history(label: &str, url: &str) -> (bool, bool) {
    locked(&GUESSES)
        .entry(label.to_string())
        .or_default()
        .loaded(url)
}

#[cfg(windows)]
fn hook_for(
    app: &AppHandle,
    tab: String,
    seed: String,
    role: crate::dwm::Role,
) -> crate::dwm::Hook {
    crate::dwm::Hook {
        tab,
        seed,
        role,
        downloads: crate::downloads::downloads_dir(app),
        devtools: crate::siteprefs::devtools(),
    }
}

#[cfg(windows)]
fn watch_page(app: &AppHandle, wv: &Webview, tab: String, seed: String) {
    let label = wv.label().to_string();
    let hook = hook_for(app, tab.clone(), seed, crate::dwm::Role::Page);
    let app = app.clone();
    let hooked = crate::dwm::watch_page(wv, hook, move |signal| {
        on_signal(&app, &label, &tab, signal);
    });
    if let Err(e) = hooked {
        tracing::debug!("page events not attached: {e}");
    }
}

#[cfg(windows)]
fn watch_popup(app: &AppHandle, window: &tauri::WebviewWindow, tab: String) {
    let label = window.label().to_string();
    let hook = hook_for(
        app,
        tab.clone(),
        "about:blank".into(),
        crate::dwm::Role::Popup,
    );
    let app = app.clone();
    let hooked = crate::dwm::watch_page(window.as_ref(), hook, move |signal| {
        on_signal(&app, &label, &tab, signal);
    });
    if let Err(e) = hooked {
        tracing::debug!("popup events not attached: {e}");
    }
}

#[cfg(windows)]
fn on_signal(app: &AppHandle, label: &str, tab: &str, signal: crate::dwm::PageSignal) {
    use crate::dwm::PageSignal;
    match signal {
        PageSignal::Loading => tell(
            app,
            BROWSER,
            json!({ "tab": tab, "loading": true, "blocked": 0 }),
        ),
        PageSignal::Blocked(count) => tell(app, BROWSER, json!({ "tab": tab, "blocked": count })),
        PageSignal::Background(color) => tell(app, BROWSER, json!({ "tab": tab, "bg": color })),
        PageSignal::Loaded {
            url,
            back,
            forward,
            ok,
        } => {
            if let Ok(parsed) = Url::parse(&url) {
                keep_url(label, &parsed);
            }
            tell(
                app,
                BROWSER,
                json!({
                    "tab": tab,
                    "loading": false,
                    "url": url,
                    "canGoBack": back,
                    "canGoForward": forward,
                    "ok": ok,
                }),
            );
        }
        PageSignal::Fullscreen(on) => {
            tell(app, BROWSER, json!({ "tab": tab, "fullscreen": on }));
        }
        PageSignal::Crashed(what) => {
            tell(app, BROWSER, json!({ "tab": tab, "crashed": what }));
        }
        PageSignal::Zoom(factor) => {
            tell(app, BROWSER, json!({ "tab": tab, "zoom": factor }));
        }
        PageSignal::Favicon(icon) => {
            tell(app, BROWSER, json!({ "tab": tab, "favicon": icon }));
        }
        PageSignal::Permission { id, origin, kind } => {
            locked(&ASKING).insert(id.clone(), (origin.clone(), kind.to_string()));
            tell(
                app,
                ASK,
                json!({ "kind": "permission", "id": id, "tab": tab, "origin": origin, "permission": kind }),
            );
        }
        PageSignal::DownloadStarted {
            id,
            url,
            path,
            total,
        } => crate::downloads::started(app, &id, tab, &url, &path, total),
        PageSignal::DownloadProgress {
            id,
            received,
            total,
        } => crate::downloads::progress(app, &id, received, total),
        PageSignal::DownloadFinished { id, state, path } => {
            crate::downloads::finished(app, &id, state, Some(&path));
        }
        PageSignal::CloseRequested => {
            let (app, label) = (app.clone(), label.to_string());
            // Not from inside the WebView2 handler that asked.
            std::thread::spawn(move || {
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.close();
                }
            });
        }
        PageSignal::Key(key) => {
            tell(app, "parzi://browser-key", json!({ "key": key }));
            if key != "f11" {
                if let Some(shell) = app.get_webview("main") {
                    std::thread::spawn(move || {
                        let _ = shell.set_focus();
                    });
                }
            }
        }
    }
}

fn offer_external(app: &AppHandle, tab: &str, url: &str) {
    let id = {
        let mut offers = locked(&OFFERS);
        offers.retain(|_, o| o.at.elapsed() < Duration::from_secs(600));
        if offers
            .values()
            .any(|o| o.tab == tab && o.at.elapsed() < EXTERNAL_EVERY)
        {
            return;
        }
        offers.retain(|_, o| o.tab != tab);
        let id = format!("x{}", SEQ.fetch_add(1, Ordering::Relaxed) + 1);
        offers.insert(
            id.clone(),
            Offer {
                tab: tab.to_string(),
                url: url.to_string(),
                at: Instant::now(),
            },
        );
        id
    };
    tell(
        app,
        ASK,
        json!({ "kind": "external", "id": id, "tab": tab, "url": url }),
    );
}

/// The user's answer to "Open this link in another app?".
#[tauri::command]
pub async fn browser_external(id: String, open: bool) -> Result<(), String> {
    let offer = locked(&OFFERS)
        .remove(&id)
        .ok_or("that link is no longer waiting")?;
    if !open {
        return Ok(());
    }
    let url = Url::parse(&offer.url).map_err(|e| e.to_string())?;
    if classify(&url) != Target::External {
        return Err("that link can't be opened".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        crate::files::launch(&crate::files::open_with_default(url.as_str().into()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The user's answer to a page's permission prompt, remembered for the site.
#[tauri::command]
pub async fn browser_permission(
    app: AppHandle,
    id: String,
    allow: bool,
    remember: bool,
) -> Result<(), String> {
    let asked = locked(&ASKING).remove(&id);
    if let (true, Some((origin, kind))) = (remember, asked) {
        crate::siteprefs::remember(origin, kind, allow).await?;
    }
    #[cfg(windows)]
    {
        app.run_on_main_thread(move || crate::dwm::answer_permission(&id, allow))
            .map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (app, id);
        Ok(())
    }
}

/// Settings turned page devtools on or off: live pages follow at once.
pub fn apply_devtools(app: &AppHandle, on: bool) {
    for (label, wv) in app.webviews() {
        if label.starts_with(PAGE_PREFIX) || label.starts_with(POPUP_PREFIX) {
            #[cfg(windows)]
            crate::dwm::set_devtools(&wv, on);
            #[cfg(not(windows))]
            let _ = (wv, on);
        }
    }
}

/// Loads what page handlers read on the UI thread before a page needs it.
pub fn warm_up() {
    std::thread::spawn(|| {
        let _ = crate::siteprefs::get();
        let _ = crate::downloads::browser_downloads();
    });
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
        crate::dwm::fit_page_surface(
            wv,
            px(job.x) - 1,
            px(job.y) - 1,
            px(job.width) + 2,
            px(job.height) + 2,
        )
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

/// Hands the keyboard to the shown page (after Enter in the address bar).
#[tauri::command]
pub fn browser_focus(app: AppHandle, tab: &str) -> Result<(), String> {
    let label = page_label(tab)?;
    if *locked(&SHOWN) != label {
        return Ok(());
    }
    match app.get_webview(&label) {
        Some(wv) => wv.set_focus().map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

/// "This page crashed" › Reload: a fresh page at the last address. The
/// next show makes it.
#[tauri::command]
pub fn browser_recover(app: AppHandle, tab: &str) -> Result<(), String> {
    let label = page_label(tab)?;
    locked(&LRU).retain(|l| l != &label);
    locked(&RESTING).remove(&label);
    if *locked(&SHOWN) == label {
        locked(&SHOWN).clear();
    }
    match app.get_webview(&label) {
        Some(wv) => wv.close().map_err(|e| e.to_string()),
        None => Ok(()),
    }
}

#[tauri::command]
pub fn browser_close(app: AppHandle, tab: &str) -> Result<(), String> {
    let label = page_label(tab)?;
    locked(&TABS).remove(&label);
    locked(&LRU).retain(|l| l != &label);
    locked(&RESTING).remove(&label);
    locked(&OFFERS).retain(|_, o| o.tab != tab);
    #[cfg(not(windows))]
    locked(&GUESSES).remove(&label);
    #[cfg(windows)]
    {
        let tab = tab.to_string();
        let _ = app.run_on_main_thread(move || crate::dwm::drop_permissions(&tab));
    }
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
            slot.url = url;
            Ok(())
        }
        None => Err("no such tab".into()),
    }
}

#[tauri::command]
pub fn browser_nav(app: AppHandle, tab: &str, action: String) -> Result<(), String> {
    let label = page_label(tab)?;
    let wv = app.get_webview(&label).ok_or("no such tab")?;
    if action == "zoom-reset" {
        return wv.set_zoom(1.0).map_err(|e| e.to_string());
    }
    let js = match action.as_str() {
        "back" => "history.back()",
        "forward" => "history.forward()",
        "reload" => "location.reload()",
        "stop" => "window.stop()",
        _ => return Err(format!("unknown action {action}")),
    };
    #[cfg(windows)]
    {
        let _ = (js, label);
        crate::dwm::steer_page(&wv, action)
    }
    #[cfg(not(windows))]
    {
        locked(&GUESSES).entry(label).or_default().step(&action);
        wv.eval(js).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_never_load_the_app_origin() {
        for bad in [
            "http://tauri.localhost/",
            "https://tauri.localhost/index.html",
            "http://TAURI.localhost./",
            "http://ipc.localhost/browser_show",
            "http://asset.localhost/C:/x.png",
            "http://x.tauri.localhost/",
            "tauri://localhost/",
            "asset://localhost/x",
            "ipc://localhost/x",
        ] {
            let url = Url::parse(bad).unwrap();
            assert!(app_origin(&url), "{bad}");
            assert!(http_page(bad).is_err(), "{bad}");
            assert!(!page_navigation(&url), "{bad}");
            assert_eq!(classify(&url), Target::Refuse, "{bad}");
        }
        let blob = Url::parse("blob:http://tauri.localhost/0b6c").unwrap();
        assert!(!page_navigation(&blob));
        for ok in [
            "https://example.com/",
            "http://localhost:3000/",
            "http://app.localhost/",
            "http://tauri.example.com/",
            "about:blank",
            "blob:https://example.com/0b6c",
        ] {
            let url = Url::parse(ok).unwrap();
            assert!(!app_origin(&url), "{ok}");
            assert!(page_navigation(&url), "{ok}");
            assert_eq!(classify(&url), Target::Page, "{ok}");
        }
        assert!(http_page("file:///C:/x").is_err());
    }

    #[test]
    fn other_apps_links_ask_and_dangerous_ones_never_run() {
        for external in [
            "mailto:someone@example.com?subject=Hi",
            "tel:+4930123456",
            "zoommtg://zoom.us/join?confno=123",
            "vscode://file/c:/x.rs",
            "ms-windows-store://pdp/?productid=9NBLGGH4NNS1",
            "slack://open",
        ] {
            let url = Url::parse(external).unwrap();
            assert_eq!(classify(&url), Target::External, "{external}");
        }
        for refused in [
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "vbscript:msgbox",
            "ms-msdt:/id PCWDiagnostic",
            "search-ms:query=x&crumb=location:\\\\evil\\share",
            "ms-officecmd:{}",
            "ms-appinstaller:?source=https://evil/x.msix",
            "view-source:https://example.com",
            "parzi://anything",
            "chrome://settings",
        ] {
            let url = Url::parse(refused).unwrap();
            assert_eq!(classify(&url), Target::Refuse, "{refused}");
        }
        let long = Url::parse(&format!("mailto:{}@x.com", "a".repeat(3000))).unwrap();
        assert_eq!(classify(&long), Target::Refuse);
    }

    #[test]
    fn fragments_do_not_make_another_page() {
        assert!(same_page(
            "https://example.com/docs#intro",
            "https://example.com/docs"
        ));
        assert!(same_page("https://example.com", "https://example.com/#top"));
        assert!(!same_page(
            "https://example.com/docs?a=1",
            "https://example.com/docs?a=2"
        ));
        assert!(!same_page("https://example.com/a", "https://example.com/b"));
        assert!(same_page("not a url#x", "not a url"));
    }

    #[test]
    fn history_guess_follows_loads_and_steps() {
        let mut g = NavGuess::default();
        assert_eq!(g.loaded("https://a.com/"), (false, false));
        assert_eq!(
            g.loaded("https://a.com/"),
            (false, false),
            "same page again"
        );
        assert_eq!(g.loaded("https://a.com/1"), (true, false));
        assert_eq!(g.loaded("https://a.com/2"), (true, false));
        g.step("back");
        assert_eq!(g.loaded("https://a.com/1"), (true, true));
        g.step("back");
        assert_eq!(g.loaded("https://a.com/"), (false, true));
        g.step("forward");
        assert_eq!(g.loaded("https://a.com/1"), (true, true));
        g.step("reload");
        assert_eq!(g.loaded("https://a.com/1"), (true, true));
        assert_eq!(
            g.loaded("https://b.com/"),
            (true, false),
            "a new load drops forward"
        );
        g.step("back");
        g.step("back");
        assert_eq!(
            g.loaded("https://a.com/1"),
            (true, true),
            "one step per load"
        );
    }

    #[test]
    fn labels_are_safe_window_labels() {
        assert_eq!(page_label("tab-1").unwrap(), "page-tab-1");
        assert_eq!(page_label("a/b c").unwrap(), "page-a_b_c");
        assert!(page_label("").is_err());
    }
}
