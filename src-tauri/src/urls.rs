//! Hosts and origins of page URLs, parsed one way everywhere: by the URL
//! parser, so `user:pw@host:port` or a `#fragment` never pass for a host.

use tauri::Url;

/// The host a person recognizes: lowercase, without `www.`; empty when the
/// URL has none.
pub fn host(raw: &str) -> String {
    Url::parse(raw)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .map(|h| match h.strip_prefix("www.") {
            Some(rest) => rest.to_string(),
            None => h,
        })
        .unwrap_or_default()
}

/// The page's origin as the page itself reports it (`location.origin`):
/// `https://host[:port]`, or `null` for `about:`, `data:` and the like.
pub fn origin(raw: &str) -> String {
    Url::parse(raw).map_or_else(|_| "null".into(), |u| u.origin().ascii_serialization())
}

/// `https://host[:port]` for a web page; None for anything that is not a
/// web origin (a decision for `data:` or `file:` would never match again).
pub fn site_origin(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_never_show_credentials_or_paths() {
        assert_eq!(
            host("https://user:pw@Bank.example:8443/a?b#c"),
            "bank.example"
        );
        assert_eq!(host("https://WWW.Example.com/a"), "example.com");
        assert_eq!(host("not a url"), "");
        assert_eq!(host("about:blank"), "");
    }

    #[test]
    fn origins_match_what_pages_report() {
        assert_eq!(
            origin("https://Mail.Example.com:443/x#y"),
            "https://mail.example.com"
        );
        assert_eq!(origin("http://localhost:3000/a"), "http://localhost:3000");
        assert_eq!(origin("about:blank"), "null");
        assert_eq!(origin("garbage"), "null");
        assert_eq!(
            site_origin("https://Meet.Google.com/abc?x=1#y").as_deref(),
            Some("https://meet.google.com")
        );
        assert_eq!(site_origin("data:text/html,hi"), None);
        assert_eq!(site_origin("file:///C:/x.html"), None);
    }
}
