//! Antigravity OAuth: constants, headers, token refresh, login flow.
//!
//! OAuth client parameters derived from MIT-licensed `opencode-antigravity-auth`
//! (attribution in THIRD_PARTY_NOTICES.md) — translated to Rust. The token
//! endpoint dance itself is standard Google OAuth2.

use parzi_core::error::{ParziError, Result};

/// Public OAuth client of the Antigravity integration (MIT source, attributed).
pub const CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
pub const CLIENT_SECRET: &str = "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";
pub const SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform \
    https://www.googleapis.com/auth/userinfo.email \
    https://www.googleapis.com/auth/userinfo.profile \
    https://www.googleapis.com/auth/cclog \
    https://www.googleapis.com/auth/experimentsandconfigs";
pub const REDIRECT_URI: &str = "http://localhost:51121/oauth-callback";
/// MUST track Google's supported Antigravity versions or requests get rejected.
pub const AG_VERSION: &str = "2.14.0";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

pub struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
}

/// Version-pinned headers. Google rejects stale/missing Antigravity versions.
pub fn headers() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    // Rotate among known-good fingerprints to avoid single-UA throttling.
    let agents = [
        format!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Antigravity/{AG_VERSION} Chrome/138.0.7204.235 Electron/37.3.1 Safari/537.36"
        ),
        format!("antigravity/{AG_VERSION} windows/amd64"),
        format!("antigravity/{AG_VERSION} darwin/arm64"),
    ];
    let pick = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as usize)
        .unwrap_or(0))
        % agents.len();
    h.insert("User-Agent", agents[pick].parse().unwrap());
    h.insert(
        "X-Goog-Api-Client",
        "google-cloud-sdk vscode_cloudshelleditor/0.1"
            .parse()
            .unwrap(),
    );
    h.insert(
        "Client-Metadata",
        r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#
            .parse()
            .unwrap(),
    );
    h
}

pub fn auth_url() -> String {
    format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={CLIENT_ID}&redirect_uri={REDIRECT_URI}&response_type=code&scope={}&access_type=offline&prompt=consent",
        urlencode(SCOPES)
    )
}

/// Open a URL in the default browser. On Windows this MUST NOT go through
/// `cmd /C start` unquoted: `&` (and `%xx`) in OAuth URLs are cmd
/// metacharacters, so the browser received only `?client_id=…` — Google's
/// `missing response_type` 400. `rundll32 url.dll,FileProtocolHandler` takes
/// the URL as a single argv and sidesteps cmd entirely.
pub fn open_browser(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .status();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).status();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).status();
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-.~/:".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

async fn token_request(form: &[(&str, &str)]) -> Result<Tokens> {
    let client = reqwest::Client::new();
    let v: serde_json::Value = client
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|e| ParziError::Provider("antigravity".into(), format!("oauth: {e}")))?
        .json()
        .await
        .map_err(|e| ParziError::Provider("antigravity".into(), format!("oauth parse: {e}")))?;
    if let Some(err) = v.get("error") {
        return Err(ParziError::Provider(
            "antigravity".into(),
            format!("oauth: {err}"),
        ));
    }
    Ok(Tokens {
        access: v
            .get("access_token")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .into(),
        refresh: v
            .get("refresh_token")
            .and_then(|t| t.as_str())
            .map(str::to_string),
    })
}

pub async fn exchange_code(code: &str) -> Result<Tokens> {
    token_request(&[
        ("code", code),
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("redirect_uri", REDIRECT_URI),
        ("grant_type", "authorization_code"),
    ])
    .await
}

pub async fn refresh_access(refresh_token: &str) -> Result<Tokens> {
    token_request(&[
        ("refresh_token", refresh_token),
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("grant_type", "refresh_token"),
    ])
    .await
}

/// Minimal localhost callback server. Reads one GET, extracts `?code=`,
/// answers with a human-readable page, returns the code.
pub async fn wait_for_code(timeout_secs: u64) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:51121")
        .await
        .map_err(|e| ParziError::Provider("antigravity".into(), format!("callback bind: {e}")))?;
    let accept = listener.accept();
    let (mut sock, _) = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), accept)
        .await
        .map_err(|_| ParziError::Provider("antigravity".into(), "sign-in expired (5 min)".into()))?
        .map_err(|e| ParziError::Provider("antigravity".into(), format!("callback: {e}")))?;
    let mut buf = vec![0u8; 8192];
    let n = sock
        .read(&mut buf)
        .await
        .map_err(|e| ParziError::Provider("antigravity".into(), format!("callback read: {e}")))?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let code = req
        .split_whitespace()
        .nth(1)
        .and_then(|path| path.split("code=").nth(1))
        .map(|s| s.split('&').next().unwrap_or("").to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ParziError::Provider("antigravity".into(), "no code in callback".into()))?;
    let page = "<html><body style='background:#0a0a0c;color:#ededf2;font-family:sans-serif;padding:40px'><h2>Signed in — back to Parzi.</h2></body></html>";
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        page.len(),
        page
    );
    let _ = sock.write_all(resp.as_bytes()).await;
    Ok(code)
}
