#![allow(unsafe_code, reason = "Win32 window and WebView2 calls")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2AcceleratorKeyPressedEventArgs, ICoreWebView2Controller,
    ICoreWebView2Environment, ICoreWebView2_2, COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG,
    COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
    COREWEBVIEW2_PHYSICAL_KEY_STATUS, COREWEBVIEW2_WEB_RESOURCE_CONTEXT,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_CSP_VIOLATION_REPORT,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST,
};
use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler, CapturePreviewCompletedHandler,
    ContainsFullScreenElementChangedEventHandler, ExecuteScriptCompletedHandler,
    HistoryChangedEventHandler, NavigationCompletedEventHandler, NavigationStartingEventHandler,
    WebResourceRequestedEventHandler,
};
use windows::core::{w, Interface, BOOL, HSTRING, PWSTR};
use windows::Win32::Foundation::{HGLOBAL, HWND, RECT};
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{IStream, STREAM_SEEK_END, STREAM_SEEK_SET};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_B, VK_CONTROL, VK_F11, VK_H, VK_K, VK_L, VK_MENU, VK_OEM_COMMA,
    VK_P, VK_SHIFT, VK_T, VK_TAB, VK_W,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW,
};

static PAGE_ENV: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

pub fn stash_page_environment(env: ICoreWebView2Environment) {
    let raw = Interface::into_raw(env) as isize;
    let prev = PAGE_ENV.swap(raw, std::sync::atomic::Ordering::SeqCst);
    if prev != 0 {
        // SAFETY: `prev` came from `into_raw` and was swapped out, so this is
        // the only owner of that reference.
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
        // SAFETY: `raw` came from `into_raw` and was swapped out, so ownership
        // moves here exactly once.
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
    // SAFETY: this runs on the UI thread that owns the controller, `host` is
    // the controller's own container window, and every out-param outlives
    // the call that writes it.
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

pub fn steer_page<R: tauri::Runtime>(wv: &tauri::Webview<R>, action: String) -> Result<(), String> {
    wv.with_webview(move |platform| {
        // SAFETY: with_webview runs this on the UI thread that owns the controller.
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
        // SAFETY: with_webview runs this on the UI thread that owns the
        // controller; take_pwstr frees the string Source allocates.
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

pub fn run_page_script<R: tauri::Runtime>(
    wv: &tauri::Webview<R>,
    script: String,
) -> Result<String, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |platform| {
        let done = tx.clone();
        // SAFETY: with_webview runs this on the UI thread that owns the
        // controller, and WebView2 calls the completion handler on that same
        // thread; the script string outlives the ExecuteScript call.
        let started = unsafe {
            platform.controller().CoreWebView2().and_then(|core| {
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
        // SAFETY: with_webview runs this on the UI thread that owns the
        // controller, and WebView2 calls the completion handler on that same
        // thread after it has written the whole image into the stream.
        let started = unsafe {
            CreateStreamOnHGlobal(HGLOBAL::default(), true).and_then(|stream| {
                let image = stream.clone();
                platform.controller().CoreWebView2()?.CapturePreview(
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

pub enum PageSignal {
    Loading,
    Loaded {
        url: String,
        back: bool,
        forward: bool,
    },
    Fullscreen(bool),
    Key(&'static str),
    Blocked(u32),
    Background(String),
}

const BLOCKED_EVERY: Duration = Duration::from_millis(400);

pub fn watch_page<R: tauri::Runtime>(
    wv: &tauri::Webview<R>,
    signal: impl Fn(PageSignal) + Send + 'static,
) -> Result<(), String> {
    wv.with_webview(move |platform| {
        if let Err(e) = hook_page(&platform.controller(), Rc::new(signal)) {
            tracing::debug!("page hooks failed: {e}");
        }
    })
    .map_err(|e| e.to_string())
}

const ANY_NAV: u64 = u64::MAX;

fn hook_page(
    controller: &ICoreWebView2Controller,
    signal: Rc<dyn Fn(PageSignal)>,
) -> windows::core::Result<()> {
    let nav = Rc::new(Cell::new(Some(ANY_NAV)));
    let top = Rc::new(RefCell::new(String::new()));
    let blocked = Rc::new(Cell::new(0u32));
    let mut token = 0i64;
    // SAFETY: this runs on the UI thread that owns the controller; WebView2
    // invokes every handler on that same thread, and each out-param outlives
    // the call that writes it.
    unsafe {
        let core = controller.CoreWebView2()?;
        let (s, n, t, b) = (signal.clone(), nav.clone(), top.clone(), blocked.clone());
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
                    *t.borrow_mut() = uri;
                    b.set(0);
                    n.set(Some(id));
                    s(PageSignal::Loading);
                }
                Ok(())
            })),
            &raw mut token,
        )?;
        let (s, n, b) = (signal.clone(), nav.clone(), blocked.clone());
        core.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |core, args| {
                let mut id = ANY_NAV;
                if let Some(args) = args {
                    args.NavigationId(&raw mut id)?;
                }
                if matches!(n.get(), Some(cur) if cur == id || cur == ANY_NAV) {
                    n.set(None);
                    if let Some(core) = core {
                        s(loaded(&core));
                        s(PageSignal::Blocked(b.get()));
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
                    }
                }
                Ok(())
            })),
            &raw mut token,
        )?;
        hook_blocker(&core, signal.clone(), top, blocked)?;
        let (s, n) = (signal.clone(), nav);
        core.add_HistoryChanged(
            &HistoryChangedEventHandler::create(Box::new(move |core, _| {
                if let (None, Some(core)) = (n.get(), core) {
                    s(loaded(&core));
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
        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                if let Some((key, first)) = browser_key(&args)? {
                    args.SetHandled(true)?;
                    if first {
                        signal(PageSignal::Key(key));
                    }
                }
                Ok(())
            })),
            &raw mut token,
        )?;
    }
    Ok(())
}

fn hook_blocker(
    core: &ICoreWebView2,
    signal: Rc<dyn Fn(PageSignal)>,
    top: Rc<RefCell<String>>,
    blocked: Rc<Cell<u32>>,
) -> windows::core::Result<()> {
    let mut token = 0i64;
    // SAFETY: this runs on the UI thread that owns the webview; WebView2
    // invokes the request handler on that same thread, and each out-param
    // outlives the call that writes it.
    unsafe {
        let env = core.cast::<ICoreWebView2_2>()?.Environment()?;
        core.AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
        let last = Rc::new(Cell::new(Instant::now()));
        core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut uri = PWSTR::null();
                let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                args.Request()?.Uri(&raw mut uri)?;
                args.ResourceContext(&raw mut context)?;
                let uri = take_pwstr(uri);
                let block = {
                    let source = top.borrow();
                    resource_kind(context, &uri, &source)
                        .is_some_and(|kind| crate::adblock::should_block(&uri, &source, kind))
                };
                if !block {
                    return Ok(());
                }
                let response =
                    env.CreateWebResourceResponse(None::<&IStream>, 403, w!("Blocked"), w!(""))?;
                args.SetResponse(&response)?;
                blocked.set(blocked.get().saturating_add(1));
                if last.get().elapsed() >= BLOCKED_EVERY {
                    last.set(Instant::now());
                    signal(PageSignal::Blocked(blocked.get()));
                }
                Ok(())
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
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT if uri == top => return None,
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

fn loaded(core: &ICoreWebView2) -> PageSignal {
    let mut url = PWSTR::null();
    let mut back = BOOL::default();
    let mut forward = BOOL::default();
    // SAFETY: called from a WebView2 handler on the UI thread; the out-params
    // outlive the calls and `take_pwstr` frees the string Source allocates.
    unsafe {
        let _ = core.Source(&raw mut url);
        let _ = core.CanGoBack(&raw mut back);
        let _ = core.CanGoForward(&raw mut forward);
    }
    PageSignal::Loaded {
        url: take_pwstr(url),
        back: back.as_bool(),
        forward: forward.as_bool(),
    }
}

fn browser_key(
    args: &ICoreWebView2AcceleratorKeyPressedEventArgs,
) -> windows::core::Result<Option<(&'static str, bool)>> {
    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
    let mut key = 0u32;
    let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
    // SAFETY: called from the accelerator handler on the UI thread, whose
    // input queue the page shares, so GetKeyState sees this key event; the
    // out-params outlive the calls.
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
    let name = match (
        ctrl,
        shift,
        VIRTUAL_KEY(u16::try_from(key).unwrap_or_default()),
    ) {
        (false, false, VK_F11) => "f11",
        (true, false, VK_T) => "ctrl+t",
        (true, false, VK_W) => "ctrl+w",
        (true, false, VK_L) => "ctrl+l",
        (true, false, VK_P) => "ctrl+p",
        (true, false, VK_K) => "ctrl+k",
        (true, false, VK_B) => "ctrl+b",
        (true, false, VK_H) => "ctrl+h",
        (true, true, VK_T) => "ctrl+shift+t",
        (true, false, VK_OEM_COMMA) => "ctrl+comma",
        (true, false, VK_TAB) => "ctrl+tab",
        (true, true, VK_TAB) => "ctrl+shift+tab",
        _ => return Ok(None),
    };
    Ok(Some((name, !status.WasKeyDown.as_bool())))
}

pub fn round_window_corners(window: &tauri::WebviewWindow) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DWM_WINDOW_CORNER_PREFERENCE,
    };
    let Ok(hwnd) = window.hwnd() else { return };
    let pref: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
    let size = u32::try_from(std::mem::size_of_val(&pref)).unwrap_or(4);
    // SAFETY: `hwnd` is the live window handle Tauri just handed us and
    // `pref` outlives the call, which copies `size` bytes out of it.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&pref).cast(),
            size,
        );
    }
}
