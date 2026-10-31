//! Serves HTML artifacts from their own origin, with their own policy.
//!
//! An iframe's srcdoc inherits the app's Content-Security-Policy, which blocks
//! inline scripts, so an interactive artifact would load blank. A document from
//! this scheme keeps only the policy below: scripts run, nothing leaves the page.
//! The iframe's sandbox gives it an opaque origin, so it cannot reach the app.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use tauri::http::{header, Response, StatusCode};

pub const SCHEME: &str = "parzi-art";

const POLICY: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; media-src data: blob:; font-src data:";
const KEEP: usize = 64;
const MAX_BYTES: usize = 1024 * 1024;

static FRAMES: Mutex<VecDeque<(String, String)>> = Mutex::new(VecDeque::new());

fn keep(html: String) -> Result<String, String> {
    if html.len() > MAX_BYTES {
        return Err("this page is too large to show".into());
    }
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut frames = FRAMES.lock().unwrap_or_else(PoisonError::into_inner);
    frames.push_back((token.clone(), html));
    while frames.len() > KEEP {
        frames.pop_front();
    }
    Ok(token)
}

#[tauri::command]
pub fn artifact_frame(html: String) -> Result<String, String> {
    keep(html)
}

pub fn respond(path: &str) -> Response<Vec<u8>> {
    let token = path.trim_matches('/');
    let frames = FRAMES.lock().unwrap_or_else(PoisonError::into_inner);
    let found = frames
        .iter()
        .find(|(t, _)| t == token)
        .map(|(_, html)| html.clone());
    drop(frames);
    let built = match found {
        Some(html) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CONTENT_SECURITY_POLICY, POLICY)
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(html.into_bytes()),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new()),
    };
    built.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test: the pages are one shared list, so parallel tests would evict each other's.
    #[test]
    fn pages_come_back_under_their_own_policy_while_they_are_kept() {
        let token = keep("<p>hi</p>".into()).unwrap();
        let res = respond(&format!("/{token}"));
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.body(), b"<p>hi</p>");
        let policy = res.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap();
        assert!(policy.contains("script-src 'unsafe-inline'"));
        assert!(
            policy.starts_with("default-src 'none'"),
            "nothing leaves the page"
        );

        assert_eq!(respond("/nope").status(), StatusCode::NOT_FOUND);

        for _ in 0..KEEP {
            keep("new".into()).unwrap();
        }
        assert_eq!(
            respond(&format!("/{token}")).status(),
            StatusCode::NOT_FOUND,
            "only the newest pages are kept"
        );
        assert!(keep("x".repeat(MAX_BYTES + 1)).is_err());
    }
}
