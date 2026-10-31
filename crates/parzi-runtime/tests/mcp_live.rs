use std::collections::HashMap;

use parzi_core::config::McpServerCfg;
use parzi_runtime::mcp::McpManager;

fn hosted(url: &str, auth: &str) -> McpServerCfg {
    McpServerCfg {
        url: url.into(),
        auth: auth.into(),
        enabled: true,
        timeout_ms: 60_000,
        ..Default::default()
    }
}

#[tokio::test]
#[ignore = "network"]
async fn a_public_server_lists_and_runs_its_tools() {
    let m = McpManager::new(
        HashMap::from([(
            "deepwiki".to_string(),
            hosted("https://mcp.deepwiki.com/mcp", ""),
        )]),
        60,
    );
    let tools = m
        .exposed_tools("deepwiki")
        .await
        .expect("deepwiki connects");
    println!(
        "deepwiki: {:?}",
        tools.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
    assert!(!tools.is_empty());
    let (ok, out) = m
        .call_tool(
            "deepwiki",
            "read_wiki_structure",
            serde_json::json!({"repoName": "tauri-apps/tauri"}),
        )
        .await;
    println!(
        "call ok={ok}: {}",
        out.chars().take(300).collect::<String>()
    );
    assert!(ok, "{out}");
}

#[tokio::test]
#[ignore = "network, and a signed-in GitHub CLI"]
async fn github_connects_through_the_github_cli() {
    let m = McpManager::new(
        HashMap::from([(
            "github".to_string(),
            hosted("https://api.githubcopilot.com/mcp/", "gh"),
        )]),
        60,
    );
    let tools = m
        .exposed_tools("github")
        .await
        .expect("github connects with gh's login");
    println!(
        "github: {} tools, e.g. {:?}",
        tools.len(),
        tools.iter().take(8).map(|t| &t.name).collect::<Vec<_>>()
    );
    assert!(tools.len() > 5);
    let (ok, out) = m.call_tool("github", "get_me", serde_json::json!({})).await;
    println!(
        "get_me ok={ok}: {}",
        out.chars().take(200).collect::<String>()
    );
    assert!(ok, "{out}");
}

#[tokio::test]
#[ignore = "network"]
async fn a_server_that_signs_in_says_so_and_names_where() {
    let m = McpManager::new(
        HashMap::from([(
            "notion".to_string(),
            hosted("https://mcp.notion.com/mcp", ""),
        )]),
        60,
    );
    let err = m
        .exposed_tools("notion")
        .await
        .expect_err("notion needs a sign-in");
    println!("notion: {err}");
    assert!(
        err.to_string().contains(parzi_runtime::mcp_http::SIGN_IN),
        "{err}"
    );
    let challenge = parzi_runtime::mcp_http::last_challenge("notion");
    println!("challenge: {challenge:?}");
    let ends = tokio::task::spawn_blocking(move || {
        parzi_runtime::mcp_oauth::discover("https://mcp.notion.com/mcp", challenge.as_deref())
    })
    .await
    .unwrap()
    .expect("discovers its sign-in");
    println!("{ends:?}");
    assert!(ends.authorize.starts_with("https://"));
    assert!(ends.register.is_some(), "it lets apps register");

    let shown = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let seen = shown.clone();
    let attempt = parzi_runtime::mcp_oauth::sign_in(
        "notion-live-test",
        "https://mcp.notion.com/mcp",
        None,
        move |url| {
            *seen.lock().unwrap() = url.to_string();
            Ok(())
        },
    );
    let _ = tokio::time::timeout(std::time::Duration::from_secs(20), attempt).await;
    let url = shown.lock().unwrap().clone();
    println!(
        "sign-in page: {}",
        url.chars().take(220).collect::<String>()
    );
    for part in [
        "client_id=",
        "code_challenge=",
        "code_challenge_method=S256",
        "redirect_uri=http%3A%2F%2F127.0.0.1%3A",
        "resource=https%3A%2F%2Fmcp.notion.com%2Fmcp",
    ] {
        assert!(url.contains(part), "{part} in {url}");
    }
}

#[tokio::test]
#[ignore = "network"]
async fn a_custom_connector_gets_its_logo_from_its_server_or_site() {
    std::env::set_var(
        "PARZI_HOME",
        std::env::temp_dir().join(format!("parzi-live-icons-{}", std::process::id())),
    );
    let m = McpManager::new(
        HashMap::from([(
            "wiki".to_string(),
            hosted("https://mcp.deepwiki.com/mcp", ""),
        )]),
        60,
    );
    let d = m.details("wiki").await.expect("details");
    println!(
        "title {:?} website {:?} icons {:?} tools {}",
        d.title,
        d.website,
        d.icons,
        d.tools.len()
    );
    assert!(
        d.tools.iter().all(|t| !t.description.is_empty()),
        "tools carry their descriptions"
    );
    let icon = tokio::task::spawn_blocking(move || {
        parzi_runtime::connector_icon::resolve(
            "wiki",
            &d.icons,
            &d.website,
            "https://mcp.deepwiki.com/mcp",
        )
    })
    .await
    .unwrap();
    println!(
        "icon: {}",
        icon.as_deref()
            .map(|i| &i[..i.len().min(60)])
            .unwrap_or("none")
    );
    assert!(icon.is_some_and(|i| i.starts_with("data:image/")));
}
