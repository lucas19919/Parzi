//! Fetching a URL for image and file tools, with a byte cap, and naming what
//! was saved.

use std::time::Duration;

pub(super) fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub(super) const IMAGE_FETCH_CAP: u64 = 12 * 1024 * 1024 + 1;

pub(super) async fn fetch_bytes(
    url: &str,
    max_bytes: u64,
) -> std::result::Result<(Vec<u8>, String), String> {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || fetch_blocking(&url, max_bytes))
        .await
        .map_err(|e| e.to_string())?
}

fn fetch_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(90)))
            .build()
            .into()
    })
}

fn fetch_blocking(url: &str, max_bytes: u64) -> std::result::Result<(Vec<u8>, String), String> {
    let mut res = match fetch_agent().get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(code)) => return Err(format!("the service replied {code}")),
        Err(e) => return Err(e.to_string()),
    };
    let content_type = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = res
        .body_mut()
        .with_config()
        .limit(max_bytes)
        .read_to_vec()
        .map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => {
                format!(
                    "the download passed the {} MiB cap, dropped",
                    max_bytes >> 20
                )
            }
            e => e.to_string(),
        })?;
    Ok((bytes, content_type))
}

pub(super) fn file_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let tag = uuid::Uuid::new_v4().simple().to_string();
    format!("{secs}-{}", &tag[..8])
}

pub(super) fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Some("jpg")
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}
