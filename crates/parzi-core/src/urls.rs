const MAX_LEN: usize = 8192;

pub fn check_open_url(url: &str) -> Result<(), String> {
    if url.is_empty() || url.len() > MAX_LEN {
        return Err("that link isn't allowed".into());
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("that link isn't allowed".into());
    }
    // The toolbar offers http(s) pages, so both open. Anything else —
    // file:, javascript:, data: — stays blocked.
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("that link isn't allowed".into());
    }
    if url.contains([
        '\\', '"', '<', '>', '`', '|', '^', '$', ';', '\'', '(', ')', '*',
    ]) {
        return Err("that link isn't allowed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_pages_open_like_https_ones() {
        assert!(check_open_url("https://example.com/a?b=c").is_ok());
        assert!(check_open_url("http://localhost:3000/app").is_ok());
        assert!(check_open_url("http://192.168.1.20/").is_ok());
    }

    #[test]
    fn anything_but_http_stays_blocked() {
        for bad in [
            "",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "https://example.com/a b",
            "https://example.com/\u{7}bell",
            "https://example.com/\"q\"",
            "ftp://example.com/x",
        ] {
            assert!(check_open_url(bad).is_err(), "{bad}");
        }
    }
}
