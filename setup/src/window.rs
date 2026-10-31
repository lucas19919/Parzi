use std::borrow::Cow;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::{IconExtWindows, WindowBuilderExtWindows};
use tao::window::{Icon, Window, WindowBuilder};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;
use wry::http::{header::CONTENT_TYPE, Response};
use wry::{WebContext, WebView, WebViewBuilder};

const INSTALLER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/installer.exe"));
const PAGE: &str = include_str!("page.html");
const INTER_400: &[u8] = include_bytes!("../fonts/inter-latin-400-normal.woff2");
const INTER_600: &[u8] = include_bytes!("../fonts/inter-latin-600-normal.woff2");

// What the installer calls itself. Only a test build sets these, to stay clear of a real install.
const PRODUCT: &str = match option_env!("PARZI_SETUP_PRODUCT") {
    Some(p) => p,
    None => "Parzi",
};
const BINARY: &str = match option_env!("PARZI_SETUP_BINARY") {
    Some(b) => b,
    None => "parzi-app",
};

/// So a fast install still reads as something that happened, not a flash.
const AT_LEAST: Duration = Duration::from_millis(1200);
const OPENING_FOR: Duration = Duration::from_millis(600);

#[derive(Debug)]
enum Msg {
    Ready,
    Install,
    Installed(Result<(), String>),
    Drag,
    Quit,
}

fn installed_exe() -> PathBuf {
    let key = format!("Software\\{PRODUCT}\\{PRODUCT}");
    let dir = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(key)
        .and_then(|k| k.get_value::<String, _>(""))
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(PRODUCT)
        });
    dir.join(format!("{BINARY}.exe"))
}

/// A running program's file can't be opened for writing. That is exactly what
/// the installer has to do, so it answers the only question that matters here.
fn in_use(exe: &Path) -> bool {
    const SHARING_VIOLATION: i32 = 32;
    matches!(
        OpenOptions::new().write(true).open(exe),
        Err(e) if e.raw_os_error() == Some(SHARING_VIOLATION)
    )
}

fn unpack(dir: &Path) -> Result<PathBuf, String> {
    if INSTALLER.is_empty() {
        return Err("This build of setup has no installer inside it.".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not unpack: {e}"))?;
    let exe = dir.join("parzi-install.exe");
    std::fs::write(&exe, INSTALLER).map_err(|e| format!("Could not unpack: {e}"))?;
    Ok(exe)
}

/// `/S` runs silently and `/R` opens the app when it's done.
fn install(dir: &Path) -> Result<(), String> {
    let exe = unpack(dir)?;
    // Antivirus can hold a freshly written file for a moment.
    let mut tries = 0;
    let status = loop {
        match Command::new(&exe).args(["/S", "/R"]).status() {
            Ok(status) => break status,
            Err(_) if tries < 10 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(e) => return Err(format!("The installer would not start: {e}")),
        }
    };
    match status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(format!("The installer stopped with code {code}.")),
        None => Err("The installer stopped unexpectedly.".into()),
    }
}

fn start_install(proxy: EventLoopProxy<Msg>, dir: PathBuf) {
    std::thread::spawn(move || {
        let started = Instant::now();
        let done = install(&dir);
        if let Some(left) = AT_LEAST.checked_sub(started.elapsed()) {
            std::thread::sleep(left);
        }
        let _ = proxy.send_event(Msg::Installed(done));
    });
}

fn clean_up(dir: &Path) {
    for _ in 0..10 {
        if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn centre(window: &Window) {
    let Some(monitor) = window.current_monitor() else {
        return;
    };
    let (screen, at, size) = (monitor.size(), monitor.position(), window.outer_size());
    window.set_outer_position(PhysicalPosition::new(
        at.x + (screen.width as i32 - size.width as i32) / 2,
        at.y + (screen.height as i32 - size.height as i32) / 2,
    ));
}

fn page(view: &WebView, state: &str, detail: &str) {
    let detail = detail.replace('\\', "\\\\").replace('\'', "\\'");
    let _ = view.evaluate_script(&format!("show('{state}', '{detail}')"));
}

fn asset(path: &str) -> (&'static str, Cow<'static, [u8]>) {
    match path {
        "/inter-400.woff2" => ("font/woff2", Cow::Borrowed(INTER_400)),
        "/inter-600.woff2" => ("font/woff2", Cow::Borrowed(INTER_600)),
        _ => ("text/html", Cow::Borrowed(PAGE.as_bytes())),
    }
}

pub fn run() {
    let dir = std::env::temp_dir().join(format!("parzi-setup-{}", std::process::id()));
    let event_loop = EventLoopBuilder::<Msg>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Hidden until the page has drawn, so it never shows a white or half-built window.
    let window = WindowBuilder::new()
        .with_title("Parzi Setup")
        .with_inner_size(LogicalSize::new(420.0, 232.0))
        .with_resizable(false)
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_visible(false)
        .with_window_icon(Icon::from_resource(1, None).ok())
        .build(&event_loop);
    let mut context = WebContext::new(Some(dir.join("webview")));
    let view = window.as_ref().ok().and_then(|window| {
        let from_page = proxy.clone();
        WebViewBuilder::new_with_web_context(&mut context)
            .with_background_color((0x16, 0x16, 0x16, 0xff))
            .with_custom_protocol("setup".into(), |_, request| {
                let (kind, body) = asset(request.uri().path());
                Response::builder()
                    .header(CONTENT_TYPE, kind)
                    .body(body)
                    .expect("a fixed response")
            })
            .with_url("setup://localhost/")
            .with_ipc_handler(move |request| {
                let msg = match request.body().as_str() {
                    "ready" => Msg::Ready,
                    "install" => Msg::Install,
                    "drag" => Msg::Drag,
                    "close" => Msg::Quit,
                    _ => return,
                };
                let _ = from_page.send_event(msg);
            })
            .build(window)
            .ok()
    });

    let (Ok(window), Some(view)) = (window, view) else {
        // No WebView2 yet: the installer brings it. The app opens when it is done.
        let code = i32::from(install(&dir).is_err());
        clean_up(&dir);
        std::process::exit(code);
    };
    centre(&window);

    let mut view = Some(view);
    let mut installing = false;
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Msg::Ready) => {
                window.set_visible(true);
                window.set_focus();
                if in_use(&installed_exe()) {
                    if let Some(view) = &view {
                        page(view, "open", "");
                    }
                } else {
                    let _ = proxy.send_event(Msg::Install);
                }
            }
            Event::UserEvent(Msg::Install) if !installing => {
                installing = true;
                if let Some(view) = &view {
                    page(view, "installing", "");
                }
                start_install(proxy.clone(), dir.clone());
            }
            Event::UserEvent(Msg::Installed(Ok(()))) => {
                if let Some(view) = &view {
                    page(view, "opening", "");
                }
                let last = proxy.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(OPENING_FOR);
                    let _ = last.send_event(Msg::Quit);
                });
            }
            Event::UserEvent(Msg::Installed(Err(why))) => {
                installing = false;
                if let Some(view) = &view {
                    page(view, "failed", &why);
                }
            }
            Event::UserEvent(Msg::Drag) => {
                let _ = window.drag_window();
            }
            Event::UserEvent(Msg::Quit) => {
                window.set_visible(false);
                drop(view.take());
                clean_up(&dir);
                *flow = ControlFlow::Exit;
            }
            // Stopping halfway would leave a half-written install, and the
            // silent installer keeps going regardless of this window.
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } if !installing => {
                let _ = proxy.send_event(Msg::Quit);
            }
            _ => {}
        }
    });
}
