#![cfg(target_os = "windows")]
#![allow(unsafe_code, reason = "Win32 window and WebView2 calls")]

use std::cell::Cell;
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2AcceleratorKeyPressedEventArgs, ICoreWebView2Controller,
    ICoreWebView2Environment, COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
    COREWEBVIEW2_PHYSICAL_KEY_STATUS,
};
use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler, ContainsFullScreenElementChangedEventHandler,
    HistoryChangedEventHandler, NavigationCompletedEventHandler, NavigationStartingEventHandler,
};
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_B, VK_CONTROL, VK_F11, VK_K, VK_L, VK_MENU, VK_OEM_COMMA, VK_P,
    VK_SHIFT, VK_T, VK_TAB, VK_W,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW,
};

static PAGE_ENV: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

pub fn stash_page_environment(env: ICoreWebView2Environment) {
    use windows::core::Interface;
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
    use windows::core::Interface;
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

pub enum PageSignal {
    Loading,
    Loaded {
        url: String,
        back: bool,
        forward: bool,
    },
    Fullscreen(bool),
    Key(&'static str),
}

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
    let mut token = 0i64;
    // SAFETY: this runs on the UI thread that owns the controller; WebView2
    // invokes every handler on that same thread, and each out-param outlives
    // the call that writes it.
    unsafe {
        let core = controller.CoreWebView2()?;
        let (s, n) = (signal.clone(), nav.clone());
        core.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut cancel = BOOL::default();
                let mut id = 0u64;
                args.Cancel(&raw mut cancel)?;
                args.NavigationId(&raw mut id)?;
                if !cancel.as_bool() {
                    n.set(Some(id));
                    s(PageSignal::Loading);
                }
                Ok(())
            })),
            &raw mut token,
        )?;
        let (s, n) = (signal.clone(), nav.clone());
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
                    }
                }
                Ok(())
            })),
            &raw mut token,
        )?;
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
