use std::sync::OnceLock;
use std::time::Duration;

use tauri::Url;

const SUGGEST: &str = "https://duckduckgo.com/ac/";
const MAX_QUERY: usize = 200;
const MAX_PHRASES: usize = 8;

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_millis(1500)))
            .build()
            .into()
    })
}

#[tauri::command]
pub async fn search_suggest(query: String) -> Result<Vec<String>, String> {
    let q = query.trim().to_string();
    if q.is_empty() || q.chars().count() > MAX_QUERY {
        return Ok(Vec::new());
    }
    tauri::async_runtime::spawn_blocking(move || fetch(&q))
        .await
        .map_err(|e| e.to_string())?
}

fn fetch(query: &str) -> Result<Vec<String>, String> {
    let mut url = Url::parse(SUGGEST).map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("type", "list");
    let body = agent()
        .get(url.as_str())
        .header("User-Agent", "Parzi")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    Ok(phrases(&body))
}

fn phrases(body: &str) -> Vec<String> {
    let Ok(serde_json::Value::Array(parts)) = serde_json::from_str::<serde_json::Value>(body)
    else {
        return Vec::new();
    };
    parts
        .get(1)
        .and_then(serde_json::Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .take(MAX_PHRASES)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_open_search_list_shape() {
        assert_eq!(
            phrases(r#"["hello",["hello fresh","hello kitty"," "]]"#),
            ["hello fresh", "hello kitty"]
        );
        assert!(phrases("not json").is_empty());
        assert!(phrases(r#"[{"phrase":"x"}]"#).is_empty());
    }
}
