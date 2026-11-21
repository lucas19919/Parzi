//! `parzi connector`: connector tools from a session's shell, so an agent can
//! script several calls (list, filter, act) in one command instead of one
//! model turn per call, and keep bulky results out of the conversation.
//!
//! Parzi gives every session's shell the address of that session's own tool
//! server (`PARZI_MCP_URL`). This client speaks MCP to it, so each call goes
//! through the session's approvals, hooks and per-tool choices like any
//! connector call.

use serde_json::{json, Value};

/// The variable a session's shell holds the address of its tool server in.
pub const URL_VAR: &str = "PARZI_MCP_URL";

pub const USAGE: &str = "\
parzi connector list                         connectors this session can use
parzi connector tools <connector>            their tools, as typed signatures
parzi connector call <connector> <tool> [json|-]
                                             run one; - reads the json from stdin

Run it from a Parzi session's shell (it reads PARZI_MCP_URL). Each call
asks for approval like any connector tool.";

/// What a command printed and whether it worked.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub text: String,
}

impl Outcome {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: text.into(),
        }
    }

    fn fail(text: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: text.into(),
        }
    }
}

/// Runs `parzi connector <args>`. `stdin` is read only for `call ... -`.
pub fn run(args: &[String], stdin: impl FnOnce() -> String) -> Outcome {
    let Ok(url) = std::env::var(URL_VAR) else {
        return Outcome::fail(format!(
            "{URL_VAR} is not set: run this from a Parzi session's shell (shell_exec).\n\n{USAGE}"
        ));
    };
    run_with(&url, args, stdin)
}

/// `run` against a given tool server address instead of `PARZI_MCP_URL`.
pub fn run_with(url: &str, args: &[String], stdin: impl FnOnce() -> String) -> Outcome {
    run_at(url, args, stdin, &|url, body| post(url, body))
}

/// Sends one JSON-RPC request; split out so tests can answer it.
type Send<'a> = &'a dyn Fn(&str, &Value) -> Result<Value, String>;

fn run_at(url: &str, args: &[String], stdin: impl FnOnce() -> String, send: Send) -> Outcome {
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    match arg(0) {
        "list" => match tool(url, send, "connector_tools", json!({})) {
            Ok(text) => Outcome::ok(list_text(&text)),
            Err(e) => Outcome::fail(e),
        },
        "tools" if !arg(1).is_empty() => {
            match tool(url, send, "connector_tools", json!({ "connector": arg(1) })) {
                Ok(text) => match serde_json::from_str::<Vec<Value>>(&text) {
                    Ok(entries) => {
                        Outcome::ok(entries.iter().map(signature).collect::<Vec<_>>().join("\n"))
                    }
                    Err(_) => Outcome::fail(text),
                },
                Err(e) => Outcome::fail(e),
            }
        }
        "call" if !arg(1).is_empty() && !arg(2).is_empty() => {
            let raw = match arg(3) {
                "" => "{}".to_string(),
                "-" => stdin(),
                given => given.to_string(),
            };
            let inputs: Value = match serde_json::from_str(raw.trim()) {
                Ok(v @ Value::Object(_)) => v,
                _ => return Outcome::fail(format!("the inputs must be a JSON object, got: {raw}")),
            };
            let name = match relayed_name(url, send, arg(1), arg(2)) {
                Ok(n) => n,
                Err(e) => return Outcome::fail(e),
            };
            call(url, send, &name, inputs)
        }
        _ => Outcome::fail(USAGE),
    }
}

fn post(url: &str, body: &Value) -> Result<Value, String> {
    let mut reply = ureq::post(url)
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|e| {
            format!("Parzi's tool server did not answer ({e}); is the session still running?")
        })?;
    let text = reply
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Parzi's tool server sent something unreadable: {e}"))?;
    serde_json::from_str(&text).map_err(|_| format!("Parzi's tool server sent: {text}"))
}

/// Calls one of Parzi's own tools and returns its text, or why it failed.
fn tool(url: &str, send: Send, name: &str, arguments: Value) -> Result<String, String> {
    let (ok, blocks) = tools_call(url, send, name, arguments)?;
    let text = text_of(&blocks);
    if ok {
        Ok(text)
    } else {
        Err(text)
    }
}

fn tools_call(
    url: &str,
    send: Send,
    name: &str,
    arguments: Value,
) -> Result<(bool, Vec<Value>), String> {
    let reply = send(
        url,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        }),
    )?;
    if let Some(err) = reply.get("error") {
        return Err(err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the call failed")
            .to_string());
    }
    let result = reply.get("result").cloned().unwrap_or(Value::Null);
    let blocks = result
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let failed = result.get("isError").and_then(Value::as_bool) == Some(true);
    Ok((!failed, blocks))
}

/// The name Parzi serves a connector's tool under, found from its own name.
fn relayed_name(url: &str, send: Send, connector: &str, tool_name: &str) -> Result<String, String> {
    let text = tool(
        url,
        send,
        "connector_tools",
        json!({ "connector": connector }),
    )?;
    let entries: Vec<Value> = serde_json::from_str(&text).map_err(|_| text.clone())?;
    let named = |e: &&Value, key: &str| e.get(key).and_then(Value::as_str) == Some(tool_name);
    entries
        .iter()
        .find(|e| named(e, "tool") || named(e, "name"))
        .and_then(|e| e.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .ok_or_else(|| {
            format!("{connector} has no tool `{tool_name}`; see parzi connector tools {connector}")
        })
}

fn call(url: &str, send: Send, name: &str, inputs: Value) -> Outcome {
    match tools_call(
        url,
        send,
        "connector_call",
        json!({"tool": name, "arguments": inputs}),
    ) {
        Ok((ok, blocks)) => Outcome {
            ok,
            text: blocks.iter().map(block_text).collect::<Vec<_>>().join("\n"),
        },
        Err(e) => Outcome::fail(e),
    }
}

fn text_of(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A result block as text: its words, or a picture saved to a file.
fn block_text(b: &Value) -> String {
    match b.get("type").and_then(Value::as_str) {
        Some("image") => {
            let data = b.get("data").and_then(Value::as_str).unwrap_or("");
            let ext = match b.get("mimeType").and_then(Value::as_str) {
                Some("image/jpeg") => "jpg",
                Some("image/webp") => "webp",
                Some("image/gif") => "gif",
                _ => "png",
            };
            let path = std::env::temp_dir().join(format!(
                "parzi-connector-{}.{ext}",
                uuid::Uuid::new_v4().simple()
            ));
            use base64::Engine as _;
            match base64::engine::general_purpose::STANDARD.decode(data) {
                Ok(bytes) if std::fs::write(&path, &bytes).is_ok() => {
                    format!("[picture saved to {}]", path.display())
                }
                _ => "[a picture that could not be saved]".into(),
            }
        }
        _ => b
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

fn list_text(json_text: &str) -> String {
    let Ok(list) = serde_json::from_str::<Vec<Value>>(json_text) else {
        return json_text.to_string();
    };
    if list.is_empty() {
        return "no connectors are on for this session".into();
    }
    list.iter()
        .map(|c| {
            format!(
                "{} ({} tools)",
                c.get("connector").and_then(Value::as_str).unwrap_or("?"),
                c.get("tools").and_then(Value::as_u64).unwrap_or(0)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A tool as one typed line: `name(args: { a: string; b?: number })  // what it does`.
#[must_use]
pub fn signature(entry: &Value) -> String {
    let name = entry
        .get("tool")
        .or_else(|| entry.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    let args = entry
        .get("inputSchema")
        .map_or_else(|| "{}".to_string(), |s| ts_type(s, 0));
    let about = entry
        .get("description")
        .and_then(Value::as_str)
        .map(first_sentence)
        .unwrap_or_default();
    if about.is_empty() {
        format!("{name}(args: {args})")
    } else {
        format!("{name}(args: {args})  // {about}")
    }
}

fn first_sentence(s: &str) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    let end = line
        .find(". ")
        .map_or(line.len(), |i| i + 1)
        .min(line.char_indices().nth(140).map_or(line.len(), |(i, _)| i));
    line[..end].trim().to_string()
}

/// A JSON schema as a TypeScript type, kept to one line.
fn ts_type(schema: &Value, depth: usize) -> String {
    if depth > 3 {
        return "any".into();
    }
    if let Some(choices) = schema.get("enum").and_then(Value::as_array) {
        let parts: Vec<String> = choices.iter().map(Value::to_string).collect();
        if !parts.is_empty() {
            return parts.join(" | ");
        }
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(options) = schema.get(key).and_then(Value::as_array) {
            let parts: Vec<String> = options.iter().map(|o| ts_type(o, depth + 1)).collect();
            if !parts.is_empty() {
                return parts.join(" | ");
            }
        }
    }
    let kind = match schema.get("type") {
        Some(Value::String(t)) => t.as_str(),
        Some(Value::Array(ts)) => {
            let parts: Vec<String> = ts
                .iter()
                .filter_map(Value::as_str)
                .map(|t| ts_type(&json!({"type": t}), depth + 1))
                .collect();
            return parts.join(" | ");
        }
        _ if schema.get("properties").is_some() => "object",
        _ => "",
    };
    match kind {
        "string" => "string".into(),
        "integer" | "number" => "number".into(),
        "boolean" => "boolean".into(),
        "null" => "null".into(),
        "array" => {
            let item = schema
                .get("items")
                .map_or_else(|| "any".to_string(), |i| ts_type(i, depth + 1));
            if item.contains(' ') {
                format!("({item})[]")
            } else {
                format!("{item}[]")
            }
        }
        "object" => {
            let Some(props) = schema.get("properties").and_then(Value::as_object) else {
                return "object".into();
            };
            if props.is_empty() {
                return "{}".into();
            }
            let required: Vec<&str> = schema
                .get("required")
                .and_then(Value::as_array)
                .map(|r| r.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let fields: Vec<String> = props
                .iter()
                .map(|(k, v)| {
                    let opt = if required.contains(&k.as_str()) {
                        ""
                    } else {
                        "?"
                    };
                    format!("{k}{opt}: {}", ts_type(v, depth + 1))
                })
                .collect();
            format!("{{ {} }}", fields.join("; "))
        }
        _ => "any".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_schema_reads_as_a_typescript_signature() {
        let entry = json!({
            "name": "github__list_pull_requests",
            "tool": "list_pull_requests",
            "description": "List pull requests in a repository. Supports filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "owner": {"type": "string"},
                    "state": {"type": "string", "enum": ["open", "closed"]},
                    "labels": {"type": "array", "items": {"type": "string"}},
                    "page": {"type": "integer"},
                },
                "required": ["owner"],
            },
        });
        assert_eq!(
            signature(&entry),
            "list_pull_requests(args: { labels?: string[]; owner: string; page?: number; state?: \"open\" | \"closed\" })  // List pull requests in a repository."
        );
        assert_eq!(
            signature(&json!({"name": "x", "inputSchema": {"type": "object", "properties": {}}})),
            "x(args: {})"
        );
    }

    /// A tool server that knows one connector with one tool.
    fn server(url: &str, body: &Value) -> Result<Value, String> {
        assert_eq!(url, "http://test/mcp/t");
        let name = body
            .pointer("/params/name")
            .and_then(Value::as_str)
            .unwrap_or("");
        let args = body
            .pointer("/params/arguments")
            .cloned()
            .unwrap_or(Value::Null);
        let text = match (name, args.get("connector").and_then(Value::as_str)) {
            ("connector_tools", None) => {
                json!([{"connector": "github", "tools": 1, "listed": false}]).to_string()
            }
            ("connector_tools", Some("github")) => json!([{
                "name": "github__get_me",
                "tool": "get_me",
                "description": "Who you are.",
                "inputSchema": {"type": "object", "properties": {}},
            }])
            .to_string(),
            ("connector_call", _) => {
                assert_eq!(args["tool"], "github__get_me");
                format!("me, with {}", args["arguments"])
            }
            _ => {
                return Ok(
                    json!({"result": {"content": [{"type": "text", "text": "no"}], "isError": true}}),
                )
            }
        };
        Ok(json!({"result": {"content": [{"type": "text", "text": text}]}}))
    }

    fn go(args: &[&str]) -> Outcome {
        let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
        run_at(
            "http://test/mcp/t",
            &args,
            || r#"{"from":"stdin"}"#.into(),
            &server,
        )
    }

    #[test]
    fn lists_shows_and_calls_by_the_connector_s_own_names() {
        assert_eq!(go(&["list"]), Outcome::ok("github (1 tools)"));
        assert_eq!(
            go(&["tools", "github"]),
            Outcome::ok("get_me(args: {})  // Who you are.")
        );
        assert_eq!(
            go(&["call", "github", "get_me"]),
            Outcome::ok("me, with {}")
        );
        assert_eq!(
            go(&["call", "github", "get_me", "-"]),
            Outcome::ok(r#"me, with {"from":"stdin"}"#)
        );
        let missing = go(&["call", "github", "nope"]);
        assert!(
            !missing.ok && missing.text.contains("no tool `nope`"),
            "{}",
            missing.text
        );
        assert!(
            !go(&["call", "github", "get_me", "[1]"]).ok,
            "inputs are an object"
        );
        assert!(!go(&["what"]).ok && go(&["what"]).text.contains("parzi connector list"));
    }
}
