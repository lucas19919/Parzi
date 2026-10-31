use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;

const MAX_ICON: u64 = 512 * 1024;

fn picked_dir() -> Option<PathBuf> {
    Some(parzi_core::paths::parzi_dir().ok()?.join("connector-icons"))
}

fn cache_path(name: &str) -> Option<PathBuf> {
    let safe = parzi_core::connectors::connector_name(name);
    Some(
        parzi_core::paths::cache_dir()
            .ok()?
            .join("connector-icons")
            .join(format!("{safe}.txt")),
    )
}

fn mime_for(path: &str, sniff: &[u8]) -> &'static str {
    let p = path.to_ascii_lowercase();
    if sniff.starts_with(b"\x89PNG") || p.ends_with(".png") {
        "image/png"
    } else if sniff.starts_with(b"<svg") || sniff.starts_with(b"<?xml") || p.ends_with(".svg") {
        "image/svg+xml"
    } else if sniff.starts_with(b"\xFF\xD8") || p.ends_with(".jpg") || p.ends_with(".jpeg") {
        "image/jpeg"
    } else if sniff.starts_with(b"RIFF") || p.ends_with(".webp") {
        "image/webp"
    } else if sniff.starts_with(b"GIF8") || p.ends_with(".gif") {
        "image/gif"
    } else {
        "image/x-icon"
    }
}

fn data_url(bytes: &[u8], hint: &str) -> String {
    format!(
        "data:{};base64,{}",
        mime_for(hint, bytes),
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

pub fn set_picked(name: &str, file: &std::path::Path) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > 2 * MAX_ICON {
        return Err("that picture is too big for a logo (1 MB at most)".into());
    }
    let dir = picked_dir().ok_or("no place to keep it")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe = parzi_core::connectors::connector_name(name);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.file_name()
                .to_string_lossy()
                .starts_with(&format!("{safe}."))
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    std::fs::write(dir.join(format!("{safe}.{ext}")), &bytes).map_err(|e| e.to_string())?;
    Ok(data_url(&bytes, &format!("x.{ext}")))
}

fn picked(name: &str) -> Option<String> {
    let safe = parzi_core::connectors::connector_name(name);
    let entry = std::fs::read_dir(picked_dir()?).ok()?.flatten().find(|e| {
        e.file_name()
            .to_string_lossy()
            .starts_with(&format!("{safe}."))
    })?;
    let bytes = std::fs::read(entry.path()).ok()?;
    Some(data_url(&bytes, &entry.file_name().to_string_lossy()))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .into()
}

fn fetch(url: &str) -> Option<(Vec<u8>, String)> {
    let mut res = agent().get(url).call().ok()?;
    if res.status().as_u16() != 200 {
        return None;
    }
    let ctype = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = res
        .body_mut()
        .with_config()
        .limit(MAX_ICON)
        .read_to_vec()
        .ok()?;
    (!bytes.is_empty()).then_some((bytes, ctype))
}

fn fetch_image(url: &str) -> Option<String> {
    if url.starts_with("data:image/") {
        return Some(url.to_string());
    }
    let (bytes, ctype) = fetch(url)?;
    if ctype.contains("text/html") || bytes.starts_with(b"<!") || bytes.starts_with(b"<html") {
        return None;
    }
    Some(data_url(&bytes, url))
}

fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    Some(format!("{scheme}://{host}"))
}

fn site_of(url: &str) -> Option<String> {
    let o = origin(url)?;
    let (scheme, host) = o.split_once("://")?;
    let parts: Vec<&str> = host.split('.').collect();
    let host = if parts.len() > 2 && matches!(parts[0], "mcp" | "api" | "server" | "mcp-server") {
        parts[1..].join(".")
    } else {
        host.to_string()
    };
    Some(format!("{scheme}://{host}"))
}

fn icon_link(html: &str, base: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut best: Option<(u8, String)> = None;
    let mut at = 0;
    while let Some(i) = lower[at..].find("<link") {
        let start = at + i;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        let tag = &html[start..end];
        let tl = &lower[start..end];
        at = end;
        let rank = if tl.contains("apple-touch-icon") {
            3
        } else if tl.contains("rel=\"icon\"")
            || tl.contains("rel='icon'")
            || tl.contains("rel=icon")
        {
            2
        } else if tl.contains("shortcut icon") {
            1
        } else {
            continue;
        };
        let Some(h) = tl.find("href=") else { continue };
        let rest = &tag[h + 5..];
        let href = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            _ => rest.split([' ', '>']).next().unwrap_or(""),
        }
        .trim();
        if href.is_empty() {
            continue;
        }
        let abs = if href.starts_with("http") || href.starts_with("data:") {
            href.to_string()
        } else if let Some(rest) = href.strip_prefix("//") {
            format!("https://{rest}")
        } else if href.starts_with('/') {
            format!("{}{href}", origin(base)?)
        } else {
            format!("{}/{href}", base.trim_end_matches('/'))
        };
        if best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, abs));
        }
    }
    best.map(|(_, h)| h)
}

fn from_site(site: &str) -> Option<String> {
    if let Some((page, _)) = fetch(site) {
        let html = String::from_utf8_lossy(&page);
        if let Some(icon) = icon_link(&html, site).and_then(|u| fetch_image(&u)) {
            return Some(icon);
        }
    }
    let o = origin(site)?;
    fetch_image(&format!("{o}/favicon.ico")).or_else(|| {
        let host = o.split("://").nth(1)?;
        fetch_image(&format!(
            "https://www.google.com/s2/favicons?domain={host}&sz=64"
        ))
    })
}

#[must_use]
pub fn resolve(name: &str, icons: &[String], website: &str, url: &str) -> Option<String> {
    if let Some(p) = picked(name) {
        return Some(p);
    }
    let cache = cache_path(name);
    if let Some(text) = cache.as_ref().and_then(|c| std::fs::read_to_string(c).ok()) {
        return Some(text).filter(|t| !t.is_empty());
    }
    let found = icons
        .iter()
        .find_map(|i| fetch_image(i))
        .or_else(|| (!website.is_empty()).then(|| from_site(website)).flatten())
        .or_else(|| site_of(url).and_then(|s| from_site(&s)));
    if let Some(c) = cache {
        if let Some(dir) = c.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(c, found.as_deref().unwrap_or(""));
    }
    found
}

pub fn forget(name: &str) {
    if let Some(c) = cache_path(name) {
        let _ = std::fs::remove_file(c);
    }
    let safe = parzi_core::connectors::connector_name(name);
    if let Some(dir) = picked_dir() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                if e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{safe}."))
                {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hosted_server_belongs_to_its_site() {
        assert_eq!(
            site_of("https://mcp.linear.app/mcp").as_deref(),
            Some("https://linear.app")
        );
        assert_eq!(
            site_of("https://api.githubcopilot.com/mcp/").as_deref(),
            Some("https://githubcopilot.com")
        );
        assert_eq!(
            site_of("https://mcp.deepwiki.com/mcp").as_deref(),
            Some("https://deepwiki.com")
        );
        assert_eq!(
            site_of("https://example.com/mcp").as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn the_pages_best_icon_link_is_found() {
        let html = r#"<head><link rel="stylesheet" href="/a.css"><link rel="icon" href="/favicon-32.png"><link rel="apple-touch-icon" href="https://cdn.x.com/touch.png"></head>"#;
        assert_eq!(
            icon_link(html, "https://x.com").as_deref(),
            Some("https://cdn.x.com/touch.png")
        );
        let rel = r#"<link rel='shortcut icon' href='img/fav.ico'>"#;
        assert_eq!(
            icon_link(rel, "https://y.org/app").as_deref(),
            Some("https://y.org/app/img/fav.ico")
        );
        assert!(icon_link("<link rel=stylesheet href=x.css>", "https://z.io").is_none());
    }

    #[test]
    fn pictures_say_what_they_are() {
        assert!(data_url(b"\x89PNG....", "x").starts_with("data:image/png;base64,"));
        assert!(data_url(b"<svg xmlns", "x").starts_with("data:image/svg+xml;base64,"));
    }
}
