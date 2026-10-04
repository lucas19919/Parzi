const MAX_LEN: usize = 8192;

pub fn check_open_url(url: &str) -> Result<(), String> {
    if url.is_empty() || url.len() > MAX_LEN {
        return Err("that link isn't allowed".into());
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("that link isn't allowed".into());
    }
    if !url.starts_with("https://") {
        return Err("that link isn't allowed".into());
    }
    if url.contains([
        '\\', '"', '<', '>', '`', '|', '^', '$', ';', '\'', '(', ')', '*',
    ]) {
        return Err("that link isn't allowed".into());
    }
    Ok(())
}
