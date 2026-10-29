#![allow(unsafe_code, reason = "Win32 window and WebView2 calls")]

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2AcceleratorKeyPressedEventArgs, ICoreWebView2Controller,
    ICoreWebView2Deferral, ICoreWebView2DownloadOperation, ICoreWebView2Environment,
    ICoreWebView2PermissionRequestedEventArgs, ICoreWebView2PermissionRequestedEventArgs3,
    ICoreWebView2_15, ICoreWebView2_19, ICoreWebView2_2, ICoreWebView2_3, ICoreWebView2_4,
    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG, COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON,
    COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED, COREWEBVIEW2_DOWNLOAD_STATE,
    COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED, COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS,
    COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, COREWEBVIEW2_KEY_EVENT_KIND,
    COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL, COREWEBVIEW2_PERMISSION_KIND,
    COREWEBVIEW2_PERMISSION_KIND_CAMERA, COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ,
    COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE, COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION,
    COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
    COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES,
    COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS,
    COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS, COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS,
    COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
    COREWEBVIEW2_PERMISSION_STATE_DENY, COREWEBVIEW2_PHYSICAL_KEY_STATUS,
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_CSP_VIOLATION_REPORT,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST,
};
use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler,
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, BytesReceivedChangedEventHandler,
    CapturePreviewCompletedHandler, ContainsFullScreenElementChangedEventHandler,
    DOMContentLoadedEventHandler, DownloadStartingEventHandler, ExecuteScriptCompletedHandler,
    FaviconChangedEventHandler, GetFaviconCompletedHandler, HistoryChangedEventHandler,
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, PermissionRequestedEventHandler, ProcessFailedEventHandler,
    StateChangedEventHandler, TrySuspendCompletedHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler, WindowCloseRequestedEventHandler,
    ZoomFactorChangedEventHandler,
};
use windows::core::{w, Interface, BOOL, HSTRING, PWSTR};
use windows::Win32::Foundation::{HGLOBAL, HWND, RECT};
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{IStream, STREAM_SEEK_END, STREAM_SEEK_SET};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, ReleaseCapture, VIRTUAL_KEY, VK_B, VK_CONTROL, VK_F11, VK_H,
    VK_J, VK_K, VK_L, VK_LBUTTON, VK_MENU, VK_OEM_COMMA, VK_SHIFT, VK_T, VK_TAB, VK_W,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_BOTTOM, HWND_TOP, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW,
};

use crate::downloads::State as DownloadState;

static PAGE_ENV: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Asked {
    args: ICoreWebView2PermissionRequestedEventArgs,
    deferral: ICoreWebView2Deferral,
    tab: String,
}

thread_local! {
    static ASKED: RefCell<HashMap<String, Asked>> = RefCell::new(HashMap::new());
    static DOWNLOADS: RefCell<HashMap<String, ICoreWebView2DownloadOperation>> =
        RefCell::new(HashMap::new());
    static NEW_WINDOW: RefCell<Option<(String, bool)>> = const { RefCell::new(None) };
}

pub fn stash_page_environment(env: ICoreWebView2Environment) {
    let raw = Interface::into_raw(env) as isize;
    let prev = PAGE_ENV.swap(raw, std::sync::atomic::Ordering::SeqCst);
    if prev != 0 {
        unsafe {
            let old: ICoreWebView2Environment = Interface::from_raw(prev as *mut core::ffi::c_void);
            drop(old);
        }
    }
}

pub fn take_page_environment() -> Option<ICoreWebView2Environment> {
    let raw = PAGE_ENV.swap(0, std::sync::atomic::Ordering::SeqCst);
    if raw == 0 {
        None
    } else {
        Some(unsafe { Interface::from_raw(raw as *mut core::ffi::c_void) })
    }
}

pub fn fit_page_surface<R: tauri::Runtime>(
    wv: &tauri::Webview<R>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |platform| {
        let result = fit_controller(&platform.controller(), x, y, width, height);
        let _ = tx.send(result.map_err(|e| e.to_string()));
    })
    .map_err(|e| e.to_string())?;
    match rx.recv_timeout(std::time::Duration::from_secs(2)) {
        Ok(result) => result,
        Err(_) => Err("page surface timed out".into()),
    }
}

fn fit_controller(
    controller: &ICoreWebView2Controller,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> windows::core::Result<()> {
    let bounds = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    let mut host = HWND::default();
    let mut visible = BOOL::default();
    let mut current = RECT::default();
    unsafe {
        controller.ParentWindow(&raw mut host)?;
        SetWindowPos(
            host,
            Some(HWND_TOP),
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )?;
        controller.IsVisible(&raw mut visible)?;
        controller.Bounds(&raw mut current)?;
        if !visible.as_bool()
            || current.right - current.left < 8
            || current.bottom - current.top < 8
        {
            controller.SetIsVisible(false)?;
            controller.SetBounds(bounds)?;
            controller.SetIsVisible(true)?;
        } else if current != bounds {
            controller.SetBounds(bounds)?;
        }
        controller.NotifyParentWindowPositionChanged()
    }
}

pub fn hide_page_surface<R: tauri::Runtime>(wv: &tauri::Webview<R>) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |platform| {
        let result = hide_controller(&platform.controller());
        let _ = tx.send(result.map_err(|e| e.to_string()));
    })
    .map_err(|e| e.to_string())?;
    match rx.recv_timeout(std::time::Duration::from_secs(2)) {
        Ok(result) => result,
        Err(_) => Err("page surface hide timed out".into()),
    }
}

fn hide_controller(controller: &ICoreWebView2Controller) -> windows::core::Result<()> {
    let mut host = HWND::default();
    unsafe {
        let _ = controller.SetIsVisible(false);
        if controller.ParentWindow(&raw mut host).is_ok() {
            SetWindowPos(
                host,
                Some(HWND_BOTTOM),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_HIDEWINDOW,
            )?;
        }
    }
    Ok(())
}

pub fn steer_page<R: tauri::Runtime>(wv: &tauri::Webview<R>, action: String) -> Result<(), String> {
    wv.with_webview(move |platform| {
        let done = unsafe {
            platform
                .controller()
                .CoreWebView2()
                .and_then(|core| match action.as_str() {
                    "back" => core.GoBack(),
                    "forward" => core.GoForward(),
                    "reload" => core.Reload(),
                    _ => core.Stop(),
                })
        };
        if let Err(e) = done {
            tracing::debug!("page {action} failed: {e}");
        }
    })
    .map_err(|e| e.to_string())
}

pub fn page_source<R: tauri::Runtime>(wv: &tauri::Webview<R>) -> Result<String, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |platform| {
        let url = unsafe {
            platform.controller().CoreWebView2().and_then(|core| {
                let mut url = PWSTR::null();
                core.Source(&raw mut url).map(|()| take_pwstr(url))
            })
        };
        let _ = tx.send(url.map_err(|e| e.to_string()));
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|_| "the page did not answer".to_string())?
}

unsafe fn resume(core: &ICoreWebView2) {
    if let Ok(core) = core.cast::<ICoreWebView2_3>() {
        let _ = core.Resume();
    }
}

pub fn run_page_script<R: tauri::Runtime>(
    wv: &tauri::Webview<R>,
    script: String,
) -> Result<String, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |platform| {
        let done = tx.clone();
        let started = unsafe {
            platform.controller().CoreWebView2().and_then(|core| {
                resume(&core);
                core.ExecuteScript(
                    &HSTRING::from(script),
                    &ExecuteScriptCompletedHandler::create(Box::new(move |result, json| {
                        let _ = done.send(result.map(|()| json).map_err(|e| e.to_string()));
                        Ok(())
                    })),
                )
            })
        };
        if let Err(e) = started {
            let _ = tx.send(Err(e.to_string()));
        }
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| "the page did not answer".to_string())?
}

pub fn capture_page<R: tauri::Runtime>(wv: &tauri::Webview<R>) -> Result<Vec<u8>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |platform| {
        let done = tx.clone();
        let started = unsafe {
            CreateStreamOnHGlobal(HGLOBAL::default(), true).and_then(|stream| {
                let image = stream.clone();
                let core = platform.controller().CoreWebView2()?;
                resume(&core);
                core.CapturePreview(
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG,
                    &stream,
                    &CapturePreviewCompletedHandler::create(Box::new(move |result| {
                        let bytes = result.and_then(|()| stream_bytes(&image));
                        let _ = done.send(bytes.map_err(|e| e.to_string()));
                        Ok(())
                    })),
                )
            })
        };
        if let Err(e) = started {
            let _ = tx.send(Err(e.to_string()));
        }
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|_| "the page did not answer".to_string())?
}

unsafe fn stream_bytes(stream: &IStream) -> windows::core::Result<Vec<u8>> {
    let mut size = 0u64;
    stream.Seek(0, STREAM_SEEK_END, Some(&raw mut size))?;
    stream.Seek(0, STREAM_SEEK_SET, None)?;
    let len = u32::try_from(size).unwrap_or(u32::MAX);
    let mut buf = vec![0u8; len as usize];
    let mut read = 0u32;
    stream
        .Read(buf.as_mut_ptr().cast(), len, Some(&raw mut read))
        .ok()?;
    buf.truncate(read as usize);
    Ok(buf)
}

pub fn rest_page<R: tauri::Runtime>(wv: &tauri::Webview<R>, deep: bool) {
    let _ = wv.with_webview(move |platform| {
        unsafe {
            let Ok(core) = platform.controller().CoreWebView2() else {
                return;
            };
            if let Ok(core) = core.cast::<ICoreWebView2_19>() {
                let _ = core.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
            }
            if deep {
                if let Ok(core) = core.cast::<ICoreWebView2_3>() {
                    let _ = core
                        .TrySuspend(&TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(()))));
                }
            }
        }
    });
}

pub fn wake_page<R: tauri::Runtime>(wv: &tauri::Webview<R>) {
    let _ = wv.with_webview(move |platform| {
        unsafe {
            let Ok(core) = platform.controller().CoreWebView2() else {
                return;
            };
            if let Ok(core) = core.cast::<ICoreWebView2_19>() {
                let _ =
                    core.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL);
            }
            resume(&core);
        }
    });
}

pub fn set_devtools<R: tauri::Runtime>(wv: &tauri::Webview<R>, on: bool) {
    let _ = wv.with_webview(move |platform| {
        let done = unsafe {
            platform
                .controller()
                .CoreWebView2()
                .and_then(|core| core.Settings())
                .and_then(|settings| settings.SetAreDevToolsEnabled(on))
        };
        if let Err(e) = done {
            tracing::debug!("page devtools not set: {e}");
        }
    });
}

pub fn answer_permission(id: &str, allow: bool) {
    let Some(asked) = ASKED.with(|m| m.borrow_mut().remove(id)) else {
        return;
    };
    let state = if allow {
        COREWEBVIEW2_PERMISSION_STATE_ALLOW
    } else {
        COREWEBVIEW2_PERMISSION_STATE_DENY
    };
    unsafe {
        let _ = asked.args.SetState(state);
        let _ = asked.deferral.Complete();
    }
}

pub fn drop_permissions(tab: &str) {
    let ids: Vec<String> = ASKED.with(|m| {
        m.borrow()
            .iter()
            .filter(|(_, a)| a.tab == tab)
            .map(|(id, _)| id.clone())
            .collect()
    });
    for id in ids {
        answer_permission(&id, false);
    }
}

pub fn cancel_download(id: &str) {
    if let Some(op) = DOWNLOADS.with(|m| m.borrow().get(id).cloned()) {
        if let Err(e) = unsafe { op.Cancel() } {
            tracing::debug!("download not cancelled: {e}");
        }
    }
}

pub fn user_opened(url: &str) -> bool {
    NEW_WINDOW.with(|slot| match slot.borrow_mut().take() {
        Some((asked, user)) if crate::browser::same_page(&asked, url) => user,
        _ => true,
    })
}

pub enum PageSignal {
    Loading,
    Loaded {
        url: String,
        back: bool,
        forward: bool,
        ok: bool,
    },
    Fullscreen(bool),
    Zoom(f64),
    Key(&'static str),
    Blocked(u32),
    Background(String),
    Crashed(&'static str),
    Favicon(String),
    Permission {
        id: String,
        origin: String,
        kind: &'static str,
    },
    DownloadStarted {
        id: String,
        url: String,
        path: PathBuf,
        total: u64,
    },
    DownloadProgress {
        id: String,
        received: u64,
        total: u64,
    },
    DownloadFinished {
        id: String,
        state: DownloadState,
        path: String,
    },
    CloseRequested,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Page,
    Popup,
}

pub struct Hook {
    pub tab: String,
    pub seed: String,
    pub role: Role,
    pub downloads: Option<PathBuf>,
    pub devtools: bool,
}

const BLOCKED_EVERY: Duration = Duration::from_millis(400);
const KEY_CLAIM: Duration = Duration::from_millis(1500);
const KEY_SCRIPT: &str = r#"(() => {
  if (window.top !== window || window.__parziKeys) return;
  Object.defineProperty(window, "__parziKeys", { value: true });
  const names = { b: "ctrl+b", h: "ctrl+h", k: "ctrl+k" };
  const editable = (t) => t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName || ""));
  addEventListener("keydown", (e) => {
    const name = (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && names[String(e.key).toLowerCase()];
    if (!name || !e.isTrusted || editable(e.composedPath()[0])) return;
    setTimeout(() => {
      if (!e.defaultPrevented && window.chrome && chrome.webview) chrome.webview.postMessage({ parzi: "key", key: name });
    }, 0);
  });
})();"#;

pub fn watch_page<R: tauri::Runtime>(
    wv: &tauri::Webview<R>,
    hook: Hook,
    signal: impl Fn(PageSignal) + Send + 'static,
) -> Result<(), String> {
    wv.with_webview(move |platform| {
        if let Err(e) = hook_page(&platform.controller(), &hook, Rc::new(signal)) {
            tracing::debug!("page hooks failed: {e}");
        }
    })
    .map_err(|e| e.to_string())
}

pub fn watch_shell<R: tauri::Runtime>(
    wv: &tauri::Webview<R>,
    restart: impl Fn() + Send + 'static,
) -> Result<(), String> {
    wv.with_webview(move |platform| {
        let controller = platform.controller();
        let mut token = 0i64;
        let hooked = unsafe {
            controller.CoreWebView2().and_then(|core| {
                core.add_ProcessFailed(
                    &ProcessFailedEventHandler::create(Box::new(move |core, args| {
                        let Some(args) = args else { return Ok(()) };
                        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                        args.ProcessFailedKind(&raw mut kind)?;
                        match crash_kind(kind) {
                            Some("browser") => {
                                tracing::error!("the window's browser process exited; restarting");
                                restart();
                            }
                            Some(what) => {
                                tracing::error!("the window's page process {what}; reloading it");
                                if let Some(core) = core {
                                    let _ = core.Reload();
                                }
                            }
                            None => tracing::warn!(
                                "a helper process of the window failed (kind {})",
                                kind.0
                            ),
                        }
                        Ok(())
                    })),
                    &raw mut token,
                )
            })
        };
        if let Err(e) = hooked {
            tracing::warn!("window crash recovery not attached: {e}");
        }
    })
    .map_err(|e| e.to_string())
}

const ANY_NAV: u64 = u64::MAX;

struct Nav {
    id: Cell<Option<u64>>,
    top: RefCell<String>,
    filtering: Cell<bool>,
    blocked: Cell<u32>,
    exceptions: RefCell<Option<Rc<HashSet<String>>>>,
}

#[allow(
    clippy::too_many_lines,
    reason = "one place attaches every page handler"
)]
fn hook_page(
    controller: &ICoreWebView2Controller,
    hook: &Hook,
    signal: Rc<dyn Fn(PageSignal)>,
) -> windows::core::Result<()> {
    let nav = Rc::new(Nav {
        id: Cell::new(Some(ANY_NAV)),
        top: RefCell::new(hook.seed.clone()),
        filtering: Cell::new(crate::adblock::filtering(&hook.seed)),
        blocked: Cell::new(0),
        exceptions: RefCell::new(None),
    });
    let page = hook.role == Role::Page;
    let mut token = 0i64;
    unsafe {
        let core = controller.CoreWebView2()?;
        if let Ok(settings) = core.Settings() {
            let _ = settings.SetAreDevToolsEnabled(hook.devtools);
        }
        let (s, n) = (signal.clone(), nav.clone());
        core.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut cancel = BOOL::default();
                let mut id = 0u64;
                let mut uri = PWSTR::null();
                args.Cancel(&raw mut cancel)?;
                args.NavigationId(&raw mut id)?;
                args.Uri(&raw mut uri)?;
                let uri = take_pwstr(uri);
                if !cancel.as_bool() {
                    n.filtering.set(crate::adblock::filtering(&uri));
                    n.exceptions.borrow_mut().take();
                    *n.top.borrow_mut() = uri;
                    n.blocked.set(0);
                    n.id.set(Some(id));
                    if page {
                        s(PageSignal::Loading);
                    }
                }
                Ok(())
            })),
            &raw mut token,
        )?;
        let (s, n) = (signal.clone(), nav.clone());
        core.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |core, args| {
                let mut id = ANY_NAV;
                let mut ok = BOOL::from(true);
                if let Some(args) = &args {
                    args.NavigationId(&raw mut id)?;
                    let _ = args.IsSuccess(&raw mut ok);
                }
                if !matches!(n.id.get(), Some(cur) if cur == id || cur == ANY_NAV) {
                    return Ok(());
                }
                n.id.set(None);
                let Some(core) = core else { return Ok(()) };
                if let Some(exceptions) = n.exceptions.borrow_mut().take() {
                    hide_generic(&core, exceptions);
                }
                if !page {
                    return Ok(());
                }
                s(loaded(&core, ok.as_bool()));
                s(PageSignal::Blocked(n.blocked.get()));
                let _ = core.ExecuteScript(
                    &HSTRING::from(KEY_SCRIPT),
                    &ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(()))),
                );
                let paint = s.clone();
                let _ = core.ExecuteScript(
                    w!("(() => { const c = (e) => { const v = e && getComputedStyle(e).backgroundColor; return v && v !== 'transparent' && !v.endsWith(', 0)') ? v : ''; }; return c(document.documentElement) || c(document.body) || 'rgb(255, 255, 255)'; })()"),
                    &ExecuteScriptCompletedHandler::create(Box::new(move |_, json| {
                        if let Some(color) = page_color(&json) {
                            paint(PageSignal::Background(color));
                        }
                        Ok(())
                    })),
                );
                Ok(())
            })),
            &raw mut token,
        )?;
        let n = nav.clone();
        core.cast::<ICoreWebView2_2>()?.add_DOMContentLoaded(
            &DOMContentLoadedEventHandler::create(Box::new(move |core, _| {
                if let (true, Some(core)) = (n.filtering.get(), core) {
                    let top = n.top.borrow().clone();
                    if let Some(found) = crate::adblock::cosmetic(&top) {
                        inject_css(&core, &found.css);
                        if found.generic {
                            let exceptions = Rc::new(found.exceptions);
                            *n.exceptions.borrow_mut() = Some(exceptions.clone());
                            hide_generic(&core, exceptions);
                        }
                    }
                }
                Ok(())
            })),
            &raw mut token,
        )?;
        hook_blocker(&core, signal.clone(), nav.clone())?;
        hook_downloads(&core, signal.clone(), hook.downloads.clone())?;
        hook_permissions(&core, signal.clone(), hook.tab.clone())?;
        core.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut user = BOOL::default();
                let mut uri = PWSTR::null();
                args.IsUserInitiated(&raw mut user)?;
                args.Uri(&raw mut uri)?;
                let uri = take_pwstr(uri);
                NEW_WINDOW.with(|slot| *slot.borrow_mut() = Some((uri, user.as_bool())));
                Ok(())
            })),
            &raw mut token,
        )?;
        if !page {
            let s = signal.clone();
            core.add_WindowCloseRequested(
                &WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
                    s(PageSignal::CloseRequested);
                    Ok(())
                })),
                &raw mut token,
            )?;
            return Ok(());
        }
        let s = signal.clone();
        core.add_ProcessFailed(
            &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                args.ProcessFailedKind(&raw mut kind)?;
                if let Some(what) = crash_kind(kind) {
                    s(PageSignal::Crashed(what));
                }
                Ok(())
            })),
            &raw mut token,
        )?;
        {
            let s = signal.clone();
            controller.add_ZoomFactorChanged(
                &ZoomFactorChangedEventHandler::create(Box::new(move |controller, _| {
                    if let Some(c) = controller {
                        let mut factor = 1.0;
                        c.ZoomFactor(&raw mut factor)?;
                        s(PageSignal::Zoom(factor));
                    }
                    Ok(())
                })),
                &raw mut token,
            )?;
        }
        if let Ok(core15) = core.cast::<ICoreWebView2_15>() {
            let s = signal.clone();
            core15.add_FaviconChanged(
                &FaviconChangedEventHandler::create(Box::new(move |core, _| {
                    if let Some(core) = core.and_then(|c| c.cast::<ICoreWebView2_15>().ok()) {
                        favicon(&core, s.clone());
                    }
                    Ok(())
                })),
                &raw mut token,
            )?;
        }
        let (s, n) = (signal.clone(), nav.clone());
        core.add_HistoryChanged(
            &HistoryChangedEventHandler::create(Box::new(move |core, _| {
                if let (None, Some(core)) = (n.id.get(), core) {
                    s(loaded(&core, true));
                }
                Ok(())
            })),
            &raw mut token,
        )?;
        let s = signal.clone();
        core.add_ContainsFullScreenElementChanged(
            &ContainsFullScreenElementChangedEventHandler::create(Box::new(move |core, _| {
                let mut full = BOOL::default();
                if let Some(core) = core {
                    core.ContainsFullScreenElement(&raw mut full)?;
                }
                s(PageSignal::Fullscreen(full.as_bool()));
                Ok(())
            })),
            &raw mut token,
        )?;
        core.AddScriptToExecuteOnDocumentCreated(
            &HSTRING::from(KEY_SCRIPT),
            &AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(|_, _| Ok(()))),
        )?;
        let claimed: Rc<Cell<Option<(&'static str, Instant)>>> = Rc::new(Cell::new(None));
        let (s, waiting) = (signal.clone(), claimed.clone());
        core.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut raw = PWSTR::null();
                if args.WebMessageAsJson(&raw mut raw).is_err() {
                    return Ok(());
                }
                let Some(name) = key_message(&take_pwstr(raw)) else {
                    return Ok(());
                };
                if let Some((key, at)) = waiting.get() {
                    if key == name && at.elapsed() < KEY_CLAIM {
                        waiting.set(None);
                        s(PageSignal::Key(key));
                    }
                }
                Ok(())
            })),
            &raw mut token,
        )?;
        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                if let Some((key, first)) = browser_key(&args)? {
                    if page_first(key) {
                        if first {
                            claimed.set(Some((key, Instant::now())));
                        }
                    } else {
                        args.SetHandled(true)?;
                        if first {
                            signal(PageSignal::Key(key));
                        }
                    }
                }
                Ok(())
            })),
            &raw mut token,
        )?;
    }
    Ok(())
}

fn crash_kind(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> Option<&'static str> {
    match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => Some("browser"),
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => Some("exited"),
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => Some("unresponsive"),
        _ => None,
    }
}

unsafe fn favicon(core: &ICoreWebView2_15, signal: Rc<dyn Fn(PageSignal)>) {
    let mut uri = PWSTR::null();
    let uri = if core.FaviconUri(&raw mut uri).is_ok() {
        take_pwstr(uri)
    } else {
        String::new()
    };
    if uri.starts_with("https://") && uri.len() <= 1024 {
        signal(PageSignal::Favicon(uri));
        return;
    }
    let _ = core.GetFavicon(
        COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG,
        &GetFaviconCompletedHandler::create(Box::new(move |result, stream| {
            let Some(stream) = stream.filter(|_| result.is_ok()) else {
                return Ok(());
            };
            let bytes = stream_bytes(&stream)?;
            if !bytes.is_empty() && bytes.len() <= 16 * 1024 {
                use base64::Engine as _;
                let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                signal(PageSignal::Favicon(format!("data:image/png;base64,{data}")));
            }
            Ok(())
        })),
    );
}

unsafe fn inject_css(core: &ICoreWebView2, css: &str) {
    if css.is_empty() {
        return;
    }
    let Ok(css) = serde_json::to_string(css) else {
        return;
    };
    let script = format!(
        "(() => {{ const css = {css}; try {{ const sheet = new CSSStyleSheet(); sheet.replaceSync(css); \
         document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet]; }} catch {{ \
         const s = document.createElement('style'); s.textContent = css; (document.head || document.documentElement).appendChild(s); }} }})()"
    );
    let _ = core.ExecuteScript(
        &HSTRING::from(script),
        &ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(()))),
    );
}

const CLASS_ID_SCRIPT: &str = "(() => { const c = new Set(), i = new Set(); \
  for (const e of document.querySelectorAll('[class],[id]')) { if (c.size + i.size > 4000) break; \
  if (e.id) i.add(e.id); for (const k of e.classList) c.add(k); } return [[...c], [...i]]; })()";

unsafe fn hide_generic(core: &ICoreWebView2, exceptions: Rc<HashSet<String>>) {
    let target = core.clone();
    let _ = core.ExecuteScript(
        &HSTRING::from(CLASS_ID_SCRIPT),
        &ExecuteScriptCompletedHandler::create(Box::new(move |result, json| {
            if result.is_err() {
                return Ok(());
            }
            let Ok((classes, ids)) = serde_json::from_str::<(Vec<String>, Vec<String>)>(&json)
            else {
                return Ok(());
            };
            inject_css(
                &target,
                &crate::adblock::class_id_css(&classes, &ids, &exceptions),
            );
            Ok(())
        })),
    );
}

fn hook_blocker(
    core: &ICoreWebView2,
    signal: Rc<dyn Fn(PageSignal)>,
    nav: Rc<Nav>,
) -> windows::core::Result<()> {
    let mut token = 0i64;
    unsafe {
        let env = core.cast::<ICoreWebView2_2>()?.Environment()?;
        for filter in [w!("http://*"), w!("https://*"), w!("ws://*"), w!("wss://*")] {
            core.AddWebResourceRequestedFilter(filter, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
        }
        let last = Rc::new(Cell::new(Instant::now()));
        core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                if !nav.filtering.get() || !crate::adblock::enabled() {
                    return Ok(());
                }
                let Some(args) = args else { return Ok(()) };
                let mut uri = PWSTR::null();
                let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                args.Request()?.Uri(&raw mut uri)?;
                args.ResourceContext(&raw mut context)?;
                let uri = take_pwstr(uri);
                let block = {
                    let source = nav.top.borrow();
                    resource_kind(context, &uri, &source)
                        .is_some_and(|kind| crate::adblock::block_request(&uri, &source, kind))
                };
                if !block {
                    return Ok(());
                }
                let response =
                    env.CreateWebResourceResponse(None::<&IStream>, 403, w!("Blocked"), w!(""))?;
                args.SetResponse(&response)?;
                nav.blocked.set(nav.blocked.get().saturating_add(1));
                if last.get().elapsed() >= BLOCKED_EVERY {
                    last.set(Instant::now());
                    signal(PageSignal::Blocked(nav.blocked.get()));
                }
                Ok(())
            })),
            &raw mut token,
        )?;
    }
    Ok(())
}

fn hook_downloads(
    core: &ICoreWebView2,
    signal: Rc<dyn Fn(PageSignal)>,
    dir: Option<PathBuf>,
) -> windows::core::Result<()> {
    let mut token = 0i64;
    unsafe {
        core.cast::<ICoreWebView2_4>()?.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let op = args.DownloadOperation()?;
                let mut suggested = PWSTR::null();
                args.ResultFilePath(&raw mut suggested)?;
                let suggested = PathBuf::from(take_pwstr(suggested));
                let name = suggested
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let folder = dir
                    .clone()
                    .or_else(|| suggested.parent().map(Path::to_path_buf));
                let Some(folder) = folder else { return Ok(()) };
                let path = crate::downloads::target(&folder, &name);
                args.SetResultFilePath(&HSTRING::from(path.as_os_str()))?;
                args.SetHandled(true)?;
                let mut uri = PWSTR::null();
                op.Uri(&raw mut uri)?;
                let mut total = 0i64;
                let _ = op.TotalBytesToReceive(&raw mut total);
                let id = crate::downloads::new_id();
                signal(PageSignal::DownloadStarted {
                    id: id.clone(),
                    url: take_pwstr(uri),
                    path,
                    total: u64::try_from(total).unwrap_or(0),
                });
                let (s, at) = (signal.clone(), id.clone());
                op.add_BytesReceivedChanged(
                    &BytesReceivedChangedEventHandler::create(Box::new(move |op, _| {
                        let Some(op) = op else { return Ok(()) };
                        let (mut got, mut total) = (0i64, 0i64);
                        op.BytesReceived(&raw mut got)?;
                        let _ = op.TotalBytesToReceive(&raw mut total);
                        s(PageSignal::DownloadProgress {
                            id: at.clone(),
                            received: u64::try_from(got).unwrap_or(0),
                            total: u64::try_from(total).unwrap_or(0),
                        });
                        Ok(())
                    })),
                    &raw mut token,
                )?;
                let (s, at) = (signal.clone(), id.clone());
                op.add_StateChanged(
                    &StateChangedEventHandler::create(Box::new(move |op, _| {
                        let Some(op) = op else { return Ok(()) };
                        let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
                        op.State(&raw mut state)?;
                        if state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS {
                            return Ok(());
                        }
                        let mut why = COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON::default();
                        let _ = op.InterruptReason(&raw mut why);
                        let mut path = PWSTR::null();
                        let _ = op.ResultFilePath(&raw mut path);
                        let state = if state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
                            DownloadState::Done
                        } else if why == COREWEBVIEW2_DOWNLOAD_INTERRUPT_REASON_USER_CANCELED {
                            DownloadState::Canceled
                        } else {
                            DownloadState::Failed
                        };
                        DOWNLOADS.with(|m| m.borrow_mut().remove(&at));
                        s(PageSignal::DownloadFinished {
                            id: at.clone(),
                            state,
                            path: take_pwstr(path),
                        });
                        Ok(())
                    })),
                    &raw mut token,
                )?;
                DOWNLOADS.with(|m| m.borrow_mut().insert(id, op));
                Ok(())
            })),
            &raw mut token,
        )?;
    }
    Ok(())
}

fn permission_name(kind: COREWEBVIEW2_PERMISSION_KIND) -> Option<&'static str> {
    Some(match kind {
        COREWEBVIEW2_PERMISSION_KIND_CAMERA => "camera",
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => "microphone",
        COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => "geolocation",
        COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => "notifications",
        COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS => "sensors",
        COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ => "clipboard",
        COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS => "downloads",
        COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE => "files",
        COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS => "fonts",
        COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES => "midi",
        COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT => "windows",
        _ => return None,
    })
}

fn hook_permissions(
    core: &ICoreWebView2,
    signal: Rc<dyn Fn(PageSignal)>,
    tab: String,
) -> windows::core::Result<()> {
    let mut token = 0i64;
    unsafe {
        core.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&raw mut kind)?;
                let Some(name) = permission_name(kind) else {
                    return Ok(());
                };
                if let Ok(args3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
                    let _ = args3.SetSavesInProfile(false);
                }
                let mut uri = PWSTR::null();
                args.Uri(&raw mut uri)?;
                let Some(origin) = crate::siteprefs::site_origin(&take_pwstr(uri)) else {
                    return args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY);
                };
                match crate::siteprefs::decision(&origin, name) {
                    Some(true) => args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW),
                    Some(false) => args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY),
                    None => {
                        let deferral = args.GetDeferral()?;
                        let id = format!("p{}", NEXT.fetch_add(1, Ordering::Relaxed));
                        ASKED.with(|m| {
                            m.borrow_mut().insert(
                                id.clone(),
                                Asked {
                                    args: args.clone(),
                                    deferral,
                                    tab: tab.clone(),
                                },
                            )
                        });
                        signal(PageSignal::Permission {
                            id,
                            origin,
                            kind: name,
                        });
                        Ok(())
                    }
                }
            })),
            &raw mut token,
        )?;
    }
    Ok(())
}

fn resource_kind(
    context: COREWEBVIEW2_WEB_RESOURCE_CONTEXT,
    uri: &str,
    top: &str,
) -> Option<&'static str> {
    Some(match context {
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT if crate::browser::same_page(uri, top) => {
            return None
        }
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => "sub_frame",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => "stylesheet",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => "image",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => "media",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => "font",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => "script",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST
        | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => "xmlhttprequest",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => "ping",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => "websocket",
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_CSP_VIOLATION_REPORT => "csp_report",
        _ => "other",
    })
}

fn page_color(json: &str) -> Option<String> {
    let color: String = serde_json::from_str(json).ok()?;
    let plain = color.len() <= 40
        && color.starts_with("rgb")
        && color
            .chars()
            .all(|c| c.is_ascii_digit() || "rgba(), .".contains(c));
    plain.then_some(color)
}

fn loaded(core: &ICoreWebView2, ok: bool) -> PageSignal {
    let mut url = PWSTR::null();
    let mut back = BOOL::default();
    let mut forward = BOOL::default();
    unsafe {
        let _ = core.Source(&raw mut url);
        let _ = core.CanGoBack(&raw mut back);
        let _ = core.CanGoForward(&raw mut forward);
    }
    PageSignal::Loaded {
        url: take_pwstr(url),
        back: back.as_bool(),
        forward: forward.as_bool(),
        ok,
    }
}

fn key_message(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("parzi")?.as_str()? != "key" {
        return None;
    }
    v.get("key")?.as_str().map(str::to_string)
}

fn page_first(name: &str) -> bool {
    matches!(name, "ctrl+b" | "ctrl+h" | "ctrl+k")
}

fn key_name(ctrl: bool, shift: bool, key: VIRTUAL_KEY) -> Option<&'static str> {
    Some(match (ctrl, shift, key) {
        (false, false, VK_F11) => "f11",
        (true, false, VK_T) => "ctrl+t",
        (true, false, VK_W) => "ctrl+w",
        (true, false, VK_L) => "ctrl+l",
        (true, false, VK_K) => "ctrl+k",
        (true, false, VK_B) => "ctrl+b",
        (true, false, VK_H) => "ctrl+h",
        (true, false, VK_J) => "ctrl+j",
        (true, true, VK_T) => "ctrl+shift+t",
        (true, false, VK_OEM_COMMA) => "ctrl+comma",
        (true, false, VK_TAB) => "ctrl+tab",
        (true, true, VK_TAB) => "ctrl+shift+tab",
        _ => return None,
    })
}

fn browser_key(
    args: &ICoreWebView2AcceleratorKeyPressedEventArgs,
) -> windows::core::Result<Option<(&'static str, bool)>> {
    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
    let mut key = 0u32;
    let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
    let (ctrl, shift, alt) = unsafe {
        args.KeyEventKind(&raw mut kind)?;
        args.VirtualKey(&raw mut key)?;
        args.PhysicalKeyStatus(&raw mut status)?;
        (
            GetKeyState(i32::from(VK_CONTROL.0)) < 0,
            GetKeyState(i32::from(VK_SHIFT.0)) < 0,
            GetKeyState(i32::from(VK_MENU.0)) < 0,
        )
    };
    if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN || alt {
        return Ok(None);
    }
    let vk = VIRTUAL_KEY(u16::try_from(key).unwrap_or_default());
    Ok(key_name(ctrl, shift, vk).map(|name| (name, !status.WasKeyDown.as_bool())))
}

pub fn alert(title: &str, text: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND,
    };
    let (title, text) = (HSTRING::from(title), HSTRING::from(text));
    unsafe {
        MessageBoxW(
            None,
            &text,
            &title,
            MB_OK | MB_ICONWARNING | MB_SETFOREGROUND,
        );
    }
}

pub fn drag_allowed(button_down: bool) -> bool {
    button_down
}

pub fn begin_window_drag(window: tauri::Window) {
    let drag = window.clone();
    let posted = window.run_on_main_thread(move || {
        let down = unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON.0)) } < 0;
        if !drag_allowed(down) {
            unsafe {
                let _ = ReleaseCapture();
            }
            return;
        }
        let _ = drag.start_dragging();
    });
    if let Err(e) = posted {
        tracing::debug!("window drag was not started: {e}");
    }
}

pub fn round_window_corners(window: &tauri::WebviewWindow) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DWM_WINDOW_CORNER_PREFERENCE,
    };
    let Ok(hwnd) = window.hwnd() else { return };
    let pref: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
    let size = u32::try_from(std::mem::size_of_val(&pref)).unwrap_or(4);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&pref).cast(),
            size,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_P, VK_R};

    #[test]
    fn a_late_window_drag_is_refused_once_the_button_is_up() {
        assert!(drag_allowed(true));
        assert!(!drag_allowed(false));
    }

    #[test]
    fn page_keys_go_to_the_page_first_and_print_stays_the_pages() {
        assert_eq!(key_name(true, false, VK_K), Some("ctrl+k"));
        assert_eq!(key_name(true, false, VK_J), Some("ctrl+j"));
        assert!(!page_first("ctrl+j"));
        assert!(page_first("ctrl+k") && page_first("ctrl+b") && page_first("ctrl+h"));
        for reserved in [
            "ctrl+t",
            "ctrl+w",
            "ctrl+l",
            "ctrl+tab",
            "ctrl+shift+t",
            "f11",
        ] {
            assert!(!page_first(reserved), "{reserved}");
        }
        assert_eq!(key_name(true, false, VK_P), None, "Ctrl+P prints");
        assert_eq!(key_name(true, false, VK_R), None);
        assert_eq!(key_name(false, false, VK_T), None);
        assert_eq!(key_name(true, true, VK_T), Some("ctrl+shift+t"));
        assert_eq!(
            key_message(r#"{"parzi":"key","key":"ctrl+k"}"#).as_deref(),
            Some("ctrl+k")
        );
        assert_eq!(key_message(r#"{"key":"ctrl+k"}"#), None);
        assert_eq!(key_message(r#""parzi:key:ctrl+k""#), None);
    }

    #[test]
    fn documents_with_a_fragment_are_the_page_itself() {
        let top = "https://example.com/docs";
        assert_eq!(
            resource_kind(
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
                "https://example.com/docs#intro",
                top
            ),
            None
        );
        assert_eq!(
            resource_kind(
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
                "https://ads.example.net/frame",
                top
            ),
            Some("sub_frame")
        );
        assert_eq!(
            resource_kind(COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT, top, top),
            Some("script")
        );
    }

    #[test]
    fn crashes_that_need_the_user() {
        assert_eq!(
            crash_kind(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED),
            Some("exited")
        );
        assert_eq!(
            crash_kind(
                webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED
            ),
            None
        );
    }

    #[test]
    fn every_prompted_permission_can_be_remembered() {
        for kind in [
            COREWEBVIEW2_PERMISSION_KIND_CAMERA,
            COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
            COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION,
            COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS,
            COREWEBVIEW2_PERMISSION_KIND_OTHER_SENSORS,
            COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ,
            COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS,
            COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE,
            COREWEBVIEW2_PERMISSION_KIND_LOCAL_FONTS,
            COREWEBVIEW2_PERMISSION_KIND_MIDI_SYSTEM_EXCLUSIVE_MESSAGES,
            COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT,
        ] {
            let name = permission_name(kind).unwrap();
            assert!(crate::siteprefs::KINDS.contains(&name), "{name}");
        }
    }
}
