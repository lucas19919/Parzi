use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager, Url, Webview};

const PAGE_PREFIX: &str = "page-";
const UNLOCK_WAIT: Duration = Duration::from_secs(180);
const BW_WAIT: Duration = Duration::from_secs(30);
const MAX_KEY: usize = 512;

static SESSION: Mutex<Option<String>> = Mutex::new(None);
static OFFERED: Mutex<Option<Offer>> = Mutex::new(None);

struct Offer {
    tab: String,
    origin: String,
    ids: Vec<String>,
}

#[derive(Serialize)]
pub struct VaultState {
    status: String,
    email: String,
}

#[derive(Serialize)]
pub struct Login {
    id: String,
    name: String,
    username: String,
}

#[derive(Serialize)]
pub struct Logins {
    status: String,
    host: String,
    items: Vec<Login>,
}

#[derive(Deserialize)]
struct BwStatus {
    #[serde(default)]
    status: String,
    #[serde(default, rename = "userEmail")]
    user_email: Option<String>,
}

#[derive(Deserialize)]
struct BwItem {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(rename = "type", default)]
    kind: u8,
    login: Option<BwLogin>,
}

#[derive(Deserialize)]
struct BwLogin {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
}

fn locked<T>(m: &'static Mutex<T>) -> MutexGuard<'static, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn from_shell(webview: &Webview) -> Result<(), String> {
    if webview.label() == "main" {
        Ok(())
    } else {
        Err("not allowed here".into())
    }
}

fn session() -> Option<String> {
    locked(&SESSION).clone()
}

fn find_bw() -> Option<PathBuf> {
    parzi_providers::process::resolve("bw").or_else(|| {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let roaming = std::env::var_os("APPDATA").map(PathBuf::from);
        [
            local.map(|d| {
                d.join("Microsoft")
                    .join("WinGet")
                    .join("Links")
                    .join("bw.exe")
            }),
            roaming.map(|d| d.join("npm").join("bw.cmd")),
        ]
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
    })
}

async fn bw(args: &[&str], key: Option<&str>) -> Result<String, String> {
    let program = find_bw().ok_or("missing")?;
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .env("BW_NOINTERACTION", "true")
        .env_remove("BW_SESSION")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(key) = key {
        cmd.env("BW_SESSION", key);
    }
    #[cfg(windows)]
    cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
    let out = tokio::time::timeout(BW_WAIT, cmd.output())
        .await
        .map_err(|_| "Bitwarden did not answer".to_string())?
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("Bitwarden failed")
            .trim()
            .to_string())
    }
}

async fn status(key: Option<&str>) -> VaultState {
    if find_bw().is_none() {
        return VaultState {
            status: "missing".into(),
            email: String::new(),
        };
    }
    match bw(&["status"], key)
        .await
        .ok()
        .and_then(|raw| serde_json::from_str::<BwStatus>(raw.trim()).ok())
    {
        Some(s) => VaultState {
            status: if s.status.is_empty() {
                "locked".into()
            } else {
                s.status
            },
            email: s.user_email.unwrap_or_default(),
        },
        None => VaultState {
            status: "locked".into(),
            email: String::new(),
        },
    }
}

async fn current_state() -> VaultState {
    let key = session();
    let state = status(key.as_deref()).await;
    if key.is_some() && state.status != "unlocked" {
        *locked(&SESSION) = None;
    }
    state
}

#[tauri::command]
pub async fn vault_state(webview: Webview) -> Result<VaultState, String> {
    from_shell(&webview)?;
    Ok(current_state().await)
}

#[tauri::command]
pub async fn vault_lock(webview: Webview) -> Result<VaultState, String> {
    from_shell(&webview)?;
    let key = locked(&SESSION).take();
    *locked(&OFFERED) = None;
    if key.is_some() {
        let _ = bw(&["lock"], key.as_deref()).await;
    }
    Ok(status(None).await)
}

#[tauri::command]
pub async fn vault_unlock(webview: Webview) -> Result<VaultState, String> {
    from_shell(&webview)?;
    let now = current_state().await;
    match now.status.as_str() {
        "missing" => return Err("missing".into()),
        "unlocked" => return Ok(now),
        _ => {}
    }
    let action = if now.status == "unauthenticated" {
        "login"
    } else {
        "unlock"
    };
    let key = ask_for_key(action).await?;
    let state = status(Some(&key)).await;
    if state.status != "unlocked" {
        return Err("Bitwarden did not unlock".into());
    }
    *locked(&SESSION) = Some(key.clone());
    tauri::async_runtime::spawn(async move {
        let _ = bw(&["sync"], Some(&key)).await;
    });
    Ok(state)
}

#[cfg(windows)]
async fn ask_for_key(action: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt as _;
    use tokio::io::AsyncReadExt as _;
    use tokio::net::windows::named_pipe::ServerOptions;

    let program = find_bw().ok_or("missing")?;
    let dir = program.parent().ok_or("missing")?.to_path_buf();
    let stem = program
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| {
            s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .ok_or("unexpected Bitwarden program name")?
        .to_string();
    let pipe = format!(r"\\.\pipe\parzi-bw-{}", uuid::Uuid::new_v4().simple());
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .max_instances(1)
        .access_outbound(false)
        .create(&pipe)
        .map_err(|e| format!("could not prepare the unlock: {e}"))?;
    std::process::Command::new("cmd.exe")
        .args(["/c", "start", "Unlock Bitwarden", "cmd.exe", "/c"])
        .args([
            stem.as_str(),
            action,
            "--raw",
            "^>",
            pipe.as_str(),
            "^|^|",
            "pause",
        ])
        .current_dir(&dir)
        .creation_flags(parzi_providers::process::CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("could not open a terminal: {e}"))?;
    tokio::time::timeout(UNLOCK_WAIT, server.connect())
        .await
        .map_err(|_| "Bitwarden was not unlocked in time".to_string())?
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    tokio::time::timeout(
        BW_WAIT,
        (&mut server).take(MAX_KEY as u64 + 1).read_to_end(&mut raw),
    )
    .await
    .map_err(|_| "Bitwarden did not finish".to_string())?
    .map_err(|e| e.to_string())?;
    let key = String::from_utf8(raw).map_err(|_| "Bitwarden did not unlock".to_string())?;
    let key = key.trim();
    let plain = !key.is_empty()
        && key.len() <= MAX_KEY
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c));
    if plain {
        Ok(key.to_string())
    } else {
        Err("Bitwarden did not unlock".into())
    }
}

#[cfg(not(windows))]
async fn ask_for_key(action: &str) -> Result<String, String> {
    Err(format!(
        "Run `bw {action}` in a terminal; unlocking from Parzi needs Windows for now"
    ))
}

fn page(app: &AppHandle, tab: &str) -> Result<Webview, String> {
    let label = format!(
        "{PAGE_PREFIX}{}",
        tab.chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            })
            .collect::<String>()
    );
    app.get_webview(&label).ok_or_else(|| "no such page".into())
}

fn origin_of(url: &str) -> Option<(String, String)> {
    let url = Url::parse(url).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let local = matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]");
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return None;
    }
    Some((url.origin().ascii_serialization(), host))
}

async fn page_origin(app: &AppHandle, tab: &str) -> Result<(String, String), String> {
    let wv = page(app, tab)?;
    let source = tauri::async_runtime::spawn_blocking(move || page_source(&wv))
        .await
        .map_err(|e| e.to_string())??;
    origin_of(&source).ok_or_else(|| "Passwords are only filled on https pages".into())
}

#[cfg(windows)]
fn page_source(wv: &Webview) -> Result<String, String> {
    crate::dwm::page_source(wv)
}

#[cfg(not(windows))]
fn page_source(wv: &Webview) -> Result<String, String> {
    wv.url().map(|u| u.to_string()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn vault_logins(webview: Webview, app: AppHandle, tab: String) -> Result<Logins, String> {
    from_shell(&webview)?;
    let (origin, host) = page_origin(&app, &tab).await?;
    let state = current_state().await;
    let Some(key) = session().filter(|_| state.status == "unlocked") else {
        return Ok(Logins {
            status: state.status,
            host,
            items: Vec::new(),
        });
    };
    let raw = bw(&["list", "items", "--url", &origin], Some(&key)).await?;
    let items: Vec<Login> = serde_json::from_str::<Vec<BwItem>>(raw.trim())
        .map_err(|_| "Bitwarden sent something unexpected".to_string())?
        .into_iter()
        .filter(|i| i.kind == 1 && i.login.as_ref().is_some_and(|l| l.password.is_some()))
        .map(|i| Login {
            username: i.login.and_then(|l| l.username).unwrap_or_default(),
            id: i.id,
            name: i.name,
        })
        .collect();
    *locked(&OFFERED) = Some(Offer {
        tab,
        origin,
        ids: items.iter().map(|i| i.id.clone()).collect(),
    });
    Ok(Logins {
        status: state.status,
        host,
        items,
    })
}

#[tauri::command]
pub async fn vault_fill(
    webview: Webview,
    app: AppHandle,
    tab: String,
    id: String,
) -> Result<String, String> {
    from_shell(&webview)?;
    let (origin, _) = page_origin(&app, &tab).await?;
    let offered = locked(&OFFERED)
        .as_ref()
        .is_some_and(|o| o.tab == tab && o.origin == origin && o.ids.contains(&id));
    if !offered {
        return Err("That login is not for this page".into());
    }
    let key = session().ok_or("Bitwarden is locked")?;
    let raw = bw(&["get", "item", &id], Some(&key)).await?;
    let item: BwItem = serde_json::from_str(raw.trim())
        .map_err(|_| "Bitwarden sent something unexpected".to_string())?;
    drop(raw);
    let login = item.login.ok_or("That item has no login")?;
    let script = fill_script(
        &origin,
        &login.username.unwrap_or_default(),
        &login.password.unwrap_or_default(),
    );
    let wv = page(&app, &tab)?;
    let result = tauri::async_runtime::spawn_blocking(move || run_script(&wv, script))
        .await
        .map_err(|e| e.to_string())??;
    Ok(serde_json::from_str::<String>(&result).unwrap_or_default())
}

#[cfg(windows)]
fn run_script(wv: &Webview, script: String) -> Result<String, String> {
    crate::dwm::run_page_script(wv, script)
}

#[cfg(not(windows))]
fn run_script(wv: &Webview, script: String) -> Result<String, String> {
    wv.eval(&script)
        .map(|()| "\"filled\"".to_string())
        .map_err(|e| e.to_string())
}

fn fill_script(origin: &str, username: &str, password: &str) -> String {
    let data = json!({ "origin": origin, "username": username, "password": password });
    format!(
        "(() => {{ const d = {data}; \
         if (location.origin !== d.origin) return 'moved'; \
         const shown = (e) => e && !e.disabled && !e.readOnly && e.getClientRects().length > 0 && getComputedStyle(e).visibility !== 'hidden'; \
         const put = (e, v) => {{ if (!e || !v) return; e.focus(); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(e, v); \
         e.dispatchEvent(new Event('input', {{ bubbles: true }})); e.dispatchEvent(new Event('change', {{ bubbles: true }})); }}; \
         const pass = [...document.querySelectorAll('input[type=password]')].find(shown); \
         const scope = (pass && pass.form) || document; \
         const names = [...scope.querySelectorAll('input:not([type]),input[type=text],input[type=email],input[type=tel]')].filter(shown); \
         const before = pass ? names.filter((e) => e.compareDocumentPosition(pass) & Node.DOCUMENT_POSITION_FOLLOWING) : names; \
         const guess = names.find((e) => e.type === 'email' || /user|mail|login|ident|account/i.test(e.name + ' ' + e.id + ' ' + e.autocomplete)); \
         const user = pass ? before[before.length - 1] || guess : guess; \
         put(user, d.username); put(pass, d.password); \
         return pass ? 'filled' : user ? 'username' : 'none'; }})()"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_and_local_pages_get_an_origin() {
        assert_eq!(
            origin_of("https://GitHub.com/login?x=1"),
            Some(("https://github.com".into(), "github.com".into()))
        );
        assert_eq!(origin_of("http://example.com/"), None);
        assert_eq!(
            origin_of("http://localhost:3000/a").map(|o| o.0),
            Some("http://localhost:3000".into())
        );
        assert_eq!(origin_of("file:///c:/x.html"), None);
    }

    #[test]
    fn fill_script_carries_values_as_json() {
        let s = fill_script("https://a.com", "me\"x", "p'w</script>");
        assert!(s.contains(r#""username":"me\"x""#));
        assert!(s.contains("p'w</script>"));
        assert!(s.contains("location.origin !== d.origin"));
    }
}
