use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::McpServerCfg;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Found {
    pub name: String,
    pub source: String,
    pub server: McpServerCfg,
}

#[must_use]
pub fn connector_name(raw: &str) -> String {
    let n: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(32)
        .collect();
    if n.is_empty() {
        "connector".into()
    } else {
        n
    }
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| a.as_str().map(str::to_string))
        .collect()
}

fn string_map(v: Option<&Value>) -> HashMap<String, String> {
    v.and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect()
}

fn one_from_json(v: &Value) -> Option<McpServerCfg> {
    let o = v.as_object()?;
    let url = ["url", "serverUrl", "httpUrl"]
        .iter()
        .find_map(|k| o.get(*k).and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string();
    let command = o
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if url.is_empty() && command.is_empty() {
        return None;
    }
    let mut srv = McpServerCfg {
        command,
        args: strings(o.get("args")),
        env: string_map(o.get("env")),
        url,
        headers: string_map(o.get("headers")),
        enabled: o.get("disabled").and_then(Value::as_bool) != Some(true),
        timeout_ms: 30_000,
        ..Default::default()
    };
    unwrap_remote(&mut srv);
    normalize_authorization(&mut srv.headers);
    Some(srv)
}

fn normalize_authorization(headers: &mut HashMap<String, String>) {
    let Some(key) = headers
        .keys()
        .find(|k| k.eq_ignore_ascii_case("authorization"))
        .cloned()
    else {
        return;
    };
    let Some(value) = headers.remove(&key) else {
        return;
    };
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    headers.insert("Authorization".into(), authorization_value(value));
}

fn authorization_value(value: &str) -> String {
    if let Some(rest) = strip_prefix_ignore(value, "bearer ") {
        return format!("Bearer {}", rest.trim());
    }
    if let Some((scheme, rest)) = value.split_once(' ') {
        if !rest.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic()) {
            return format!("{scheme} {}", rest.trim());
        }
    }
    format!("Bearer {value}")
}

fn strip_prefix_ignore<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then_some(&value[prefix.len()..])
}

#[must_use]
pub fn from_json(v: &Value) -> Vec<(String, McpServerCfg)> {
    if let Some(one) = one_from_json(v) {
        return vec![("connector".into(), one)];
    }
    let map = v
        .get("mcpServers")
        .or_else(|| v.get("servers"))
        .or_else(|| v.get("mcp").and_then(|m| m.get("servers")))
        .unwrap_or(v);
    let mut out: Vec<(String, McpServerCfg)> = map
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, s)| Some((connector_name(k), one_from_json(s)?)))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[must_use]
pub fn from_toml(t: &toml::Value) -> Vec<(String, McpServerCfg)> {
    let Some(table) = t.get("mcp_servers").and_then(toml::Value::as_table) else {
        return vec![];
    };
    let mut out = vec![];
    for (name, s) in table {
        let Ok(json) = serde_json::to_value(s) else {
            continue;
        };
        let mut json = json;
        if let Some(o) = json.as_object_mut() {
            if let Some(h) = o.remove("http_headers") {
                o.entry("headers").or_insert(h);
            }
        }
        if let Some(srv) = one_from_json(&json) {
            out.push((connector_name(name), srv));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[must_use]
pub fn from_pasted(text: &str) -> Vec<(String, McpServerCfg)> {
    let t = text.trim();
    if t.starts_with('{') {
        return serde_json::from_str::<Value>(t)
            .map(|v| from_json(&v))
            .unwrap_or_default();
    }
    if let Some(one) = pasted_remote(t) {
        return vec![one];
    }
    vec![]
}

fn pasted_remote(text: &str) -> Option<(String, McpServerCfg)> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let url = lines.next()?;
    if !(url.starts_with("http://") || url.starts_with("https://"))
        || url.split_whitespace().nth(1).is_some()
    {
        return None;
    }
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = header_line(line) {
            headers.insert(k, v);
        } else if !headers
            .keys()
            .any(|k| k.eq_ignore_ascii_case("authorization"))
        {
            headers.insert("Authorization".into(), line.to_string());
        }
    }
    normalize_authorization(&mut headers);
    let name = url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split(['/', ':']).next())
        .map(|host| {
            let host = host
                .trim_start_matches("www.")
                .trim_start_matches("mcp.")
                .trim_start_matches("api.");
            host.split('.').next().unwrap_or(host).to_string()
        })
        .unwrap_or_default();
    Some((
        connector_name(&name),
        McpServerCfg {
            url: url.to_string(),
            headers,
            enabled: true,
            timeout_ms: 30_000,
            ..Default::default()
        },
    ))
}

fn header_line(line: &str) -> Option<(String, String)> {
    let (k, v) = line.split_once(':')?;
    let k = k.trim();
    let v = v.trim();
    if k.is_empty()
        || v.is_empty()
        || !k.chars().next()?.is_ascii_alphabetic()
        || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return None;
    }
    Some((k.to_string(), v.to_string()))
}

pub fn unwrap_remote(srv: &mut McpServerCfg) {
    let at = srv.args.iter().position(|a| a.starts_with("mcp-remote"));
    let (Some(at), "npx" | "npx.cmd") = (at, srv.command.as_str()) else {
        return;
    };
    let Some(url) = srv.args.get(at + 1).cloned() else {
        return;
    };
    let mut headers = HashMap::new();
    let mut i = at + 2;
    while i < srv.args.len() {
        if srv.args[i] == "--header" {
            if let Some((k, v)) = srv.args.get(i + 1).and_then(|h| h.split_once(':')) {
                let v = v.trim();
                let v = match v.strip_prefix("${").and_then(|r| r.strip_suffix('}')) {
                    Some(var) => srv.env.get(var).cloned().unwrap_or_default(),
                    None => v.to_string(),
                };
                headers.insert(k.trim().to_string(), v);
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    srv.url = url;
    srv.headers = headers;
    srv.command.clear();
    srv.args.clear();
    srv.env.clear();
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn read_toml(path: &Path) -> Option<toml::Value> {
    toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[must_use]
pub fn found_on_this_computer() -> Vec<Found> {
    let home = dirs::home_dir().unwrap_or_default();
    let roaming = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let mut out: Vec<Found> = vec![];
    let mut add = |source: &str, list: Vec<(String, McpServerCfg)>| {
        for (name, server) in list {
            if !out.iter().any(|f| f.name == name) {
                out.push(Found {
                    name,
                    source: source.into(),
                    server,
                });
            }
        }
    };
    if let Some(claude) = read_json(&home.join(".claude.json")) {
        add("Claude Code", from_json(&claude));
        for p in claude
            .get("projects")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(_, p)| p)
        {
            if p.get("mcpServers").is_some() {
                add("Claude Code", from_json(p));
            }
        }
    }
    for (source, path) in [
        (
            "Claude Desktop",
            roaming.join("Claude").join("claude_desktop_config.json"),
        ),
        ("Cursor", home.join(".cursor").join("mcp.json")),
        (
            "VS Code",
            roaming.join("Code").join("User").join("mcp.json"),
        ),
    ] {
        if let Some(v) = read_json(&path) {
            add(source, from_json(&v));
        }
    }
    for (source, path) in [
        ("Codex", home.join(".codex").join("config.toml")),
        ("Grok", home.join(".grok").join("config.toml")),
    ] {
        if let Some(t) = read_toml(&path) {
            add(source, from_toml(&t));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_usual_json_reads_as_connectors() {
        let pasted = json!({"mcpServers": {
            "Memory AI": {"type": "stdio", "command": "npx", "args": ["-y", "tsx", "index.ts"], "env": {"TOKEN": "t"}},
            "linear": {"type": "http", "url": "https://mcp.linear.app/mcp"},
            "notion": {"url": "https://mcp.notion.com/mcp", "headers": {"Authorization": "Bearer x"}},
            "broken": {"type": "stdio"},
        }});
        let got = from_json(&pasted);
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["linear", "memory-ai", "notion"]);
        let memory = &got[1].1;
        assert_eq!(memory.command, "npx");
        assert_eq!(memory.args, ["-y", "tsx", "index.ts"]);
        assert_eq!(memory.env.get("TOKEN").map(String::as_str), Some("t"));
        assert!(memory.enabled);
        assert_eq!(
            got[2].1.headers.get("Authorization").map(String::as_str),
            Some("Bearer x")
        );
        assert_eq!(from_json(&json!({"url": "https://x.dev/mcp"})).len(), 1);
        assert_eq!(from_json(&json!({"servers": {"gh": {"type": "http", "url": "https://api.githubcopilot.com/mcp/"}}}))[0].0, "gh");
    }

    #[test]
    fn codex_and_grok_toml_read_too() {
        let t: toml::Value = toml::from_str(
            r#"
            [mcp_servers.conductor]
            command = 'D:\Repos\conductor.exe'
            args = ["mcp"]
            enabled = true
            [mcp_servers.docs]
            url = "https://docs.example.com/mcp"
            http_headers = { "X-Key" = "k" }
            "#,
        )
        .unwrap();
        let got = from_toml(&t);
        assert_eq!(got[0].0, "conductor");
        assert_eq!(got[0].1.args, ["mcp"]);
        assert_eq!(got[1].1.url, "https://docs.example.com/mcp");
        assert_eq!(got[1].1.headers.get("X-Key").map(String::as_str), Some("k"));
    }

    #[test]
    fn a_pasted_address_or_block_is_understood() {
        let got = from_pasted("https://mcp.notion.com/mcp");
        assert_eq!(got[0].0, "notion");
        assert_eq!(got[0].1.url, "https://mcp.notion.com/mcp");
        assert_eq!(
            from_pasted(r#"{"mcpServers": {"a": {"command": "x"}}}"#)[0].0,
            "a"
        );
        assert!(from_pasted("not a server").is_empty());
        let authed = from_pasted("https://os.example.com/mcp\nAuthorization: Bearer secret\n");
        assert_eq!(authed[0].0, "os");
        assert_eq!(authed[0].1.url, "https://os.example.com/mcp");
        assert_eq!(
            authed[0].1.headers.get("Authorization").map(String::as_str),
            Some("Bearer secret")
        );
        assert_eq!(
            from_pasted("https://os.example.com/mcp\nsecret")[0]
                .1
                .headers
                .get("Authorization")
                .map(String::as_str),
            Some("Bearer secret")
        );
    }

    #[test]
    fn a_raw_token_is_sent_as_bearer() {
        let raw = from_json(&json!({
            "url": "https://os.example.com/mcp",
            "headers": {"Authorization": "secret"}
        }));
        assert_eq!(
            raw[0].1.headers.get("Authorization").map(String::as_str),
            Some("Bearer secret")
        );
        let basic = from_json(&json!({
            "url": "https://os.example.com/mcp",
            "headers": {"authorization": "Basic abc"}
        }));
        assert_eq!(
            basic[0].1.headers.get("Authorization").map(String::as_str),
            Some("Basic abc")
        );
    }

    #[test]
    fn mcp_remote_wrapping_comes_off() {
        let mut srv = McpServerCfg {
            command: "npx".into(),
            args: [
                "-y",
                "mcp-remote@0.14.3",
                "https://api.githubcopilot.com/mcp/",
                "--header",
                "Authorization:${AUTH_HEADER}",
            ]
            .map(String::from)
            .to_vec(),
            env: HashMap::from([("AUTH_HEADER".to_string(), "Bearer ghp_x".to_string())]),
            enabled: true,
            ..Default::default()
        };
        unwrap_remote(&mut srv);
        assert_eq!(srv.url, "https://api.githubcopilot.com/mcp/");
        assert_eq!(
            srv.headers.get("Authorization").map(String::as_str),
            Some("Bearer ghp_x")
        );
        assert!(srv.command.is_empty() && srv.args.is_empty() && srv.env.is_empty());
        let mut local = McpServerCfg {
            command: "node".into(),
            args: vec!["s.js".into()],
            ..Default::default()
        };
        let before = local.clone();
        unwrap_remote(&mut local);
        assert_eq!(local, before);
    }

    #[test]
    fn names_are_safe_keys() {
        assert_eq!(connector_name("Memory AI"), "memory-ai");
        assert_eq!(connector_name("  "), "connector");
    }
}
