//! Connector tools offered through Parzi's own MCP server: the names an agent
//! calls them by, schemas every vendor accepts, and results passed back.

use std::collections::HashSet;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::mcp::McpTool;

/// Claude puts `mcp__parzi__` (12 characters) before a tool's name and vendors
/// stop at 64, so a relayed name keeps to 52.
pub(crate) const NAME_MAX: usize = 52;
const DESCRIPTION_MAX: usize = 2048;
const SCHEMA_MAX: usize = 16 * 1024;
pub(crate) const TEXT_MAX: usize = 100 * 1024;
pub(crate) const MEDIA_MAX: usize = 4 * 1024 * 1024;

/// One connector tool as this session's agent sees it.
#[derive(Debug, Clone)]
pub(crate) struct Relayed {
    pub name: String,
    pub server: String,
    pub tool: String,
    pub entry: Value,
}

#[derive(Debug, Default)]
pub(crate) struct Relay {
    tools: Vec<Relayed>,
}

impl Relay {
    pub fn build(mut tools: Vec<McpTool>) -> Self {
        tools.sort_by(|a, b| (&a.server, &a.name).cmp(&(&b.server, &b.name)));
        tools.dedup_by(|a, b| a.server == b.server && a.name == b.name);
        let pairs: Vec<(&str, &str)> = tools
            .iter()
            .map(|t| (t.server.as_str(), t.name.as_str()))
            .collect();
        let names = agent_names(&pairs);
        let tools = tools
            .iter()
            .zip(names)
            .map(|(t, name)| Relayed {
                entry: entry(&name, t),
                name,
                server: t.server.clone(),
                tool: t.name.clone(),
            })
            .collect();
        Self { tools }
    }

    pub fn find(&self, name: &str) -> Option<&Relayed> {
        self.tools.iter().find(|t| t.name == name)
    }

    pub fn entries(&self) -> impl Iterator<Item = &Value> {
        self.tools.iter().map(|t| &t.entry)
    }
}

fn entry(name: &str, t: &McpTool) -> Value {
    let mut e = json!({
        "name": name,
        "description": clip_chars(&t.description, DESCRIPTION_MAX),
        "inputSchema": clean_schema(&t.input_schema),
    });
    if !t.title.is_empty() {
        e["title"] = json!(t.title);
    }
    if t.annotations.is_object() {
        e["annotations"] = t.annotations.clone();
    }
    e
}

fn clip_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s.to_string(),
    }
}

fn clean_part(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn hash8(server: &str, tool: &str) -> String {
    let d = Sha256::digest(format!("{server}\0{tool}").as_bytes());
    d[..4].iter().map(|b| format!("{b:02x}")).collect()
}

/// `<server>__<tool>`, only `[A-Za-z0-9_-]`, at most [`NAME_MAX`]. A name too
/// long or already taken ends in a hash of the real pair instead. Parzi's own
/// names never hold `__`, so a relayed name never shadows one of them.
pub(crate) fn agent_names(pairs: &[(&str, &str)]) -> Vec<String> {
    let mut taken: HashSet<String> = HashSet::new();
    pairs
        .iter()
        .map(|(server, tool)| {
            let base = format!("{}__{}", clean_part(server), clean_part(tool));
            let mut name = base.clone();
            if name.len() > NAME_MAX || taken.contains(&name) {
                let tag = hash8(server, tool);
                let keep = NAME_MAX - tag.len() - 1;
                name = format!("{}_{tag}", &base[..keep.min(base.len())]);
                let mut n = 1;
                while taken.contains(&name) {
                    n += 1;
                    let tail = format!("_{tag}{n}");
                    name = format!("{}{tail}", &base[..(NAME_MAX - tail.len()).min(base.len())]);
                }
            }
            taken.insert(name.clone());
            name
        })
        .collect()
}

/// A schema every vendor takes: an object at the top with its properties,
/// no `$schema`, and small enough to send with every turn.
pub(crate) fn clean_schema(raw: &Value) -> Value {
    let mut s: Map<String, Value> = raw.as_object().cloned().unwrap_or_default();
    s.remove("$schema");
    s.remove("$id");
    let mut props: Map<String, Value> = match s.remove("properties") {
        Some(Value::Object(p)) => p,
        _ => Map::new(),
    };
    let mut required: Vec<Value> = match s.remove("required") {
        Some(Value::Array(r)) => r,
        _ => vec![],
    };
    // Vendors refuse a choice at the top; its branches' fields join the rest.
    for k in ["allOf", "anyOf", "oneOf"] {
        let Some(Value::Array(branches)) = s.remove(k) else {
            continue;
        };
        for b in &branches {
            if let Some(p) = b.get("properties").and_then(Value::as_object) {
                for (name, schema) in p {
                    props.entry(name.clone()).or_insert_with(|| schema.clone());
                }
            }
            if k == "allOf" {
                if let Some(r) = b.get("required").and_then(Value::as_array) {
                    required.extend(r.iter().cloned());
                }
            }
        }
    }
    for k in ["not", "enum", "const", "if", "then", "else"] {
        s.remove(k);
    }
    let mut seen = HashSet::new();
    required.retain(|r| {
        r.as_str()
            .is_some_and(|r| props.contains_key(r) && seen.insert(r.to_string()))
    });
    s.insert("type".into(), json!("object"));
    s.insert("properties".into(), Value::Object(props));
    if !required.is_empty() {
        s.insert("required".into(), Value::Array(required));
    }
    let mut v = Value::Object(s);
    scrub(&mut v, &["$schema"], true);
    if v.to_string().len() > SCHEMA_MAX {
        scrub(
            &mut v,
            &["description", "examples", "example", "title"],
            true,
        );
    }
    if v.to_string().len() > SCHEMA_MAX {
        v = shallow(&v);
    }
    v
}

/// Takes `keys` out of every schema below the top, walking only where
/// schemas sit, so a property that happens to be called `title` stays.
fn scrub(schema: &mut Value, keys: &[&str], top: bool) {
    let Value::Object(o) = schema else {
        return;
    };
    if !top {
        for k in keys {
            o.remove(*k);
        }
    }
    for map in ["properties", "patternProperties", "$defs", "definitions"] {
        if let Some(Value::Object(m)) = o.get_mut(map) {
            for child in m.values_mut() {
                scrub(child, keys, false);
            }
        }
    }
    for list in ["anyOf", "oneOf", "allOf", "prefixItems"] {
        if let Some(Value::Array(a)) = o.get_mut(list) {
            for child in a {
                scrub(child, keys, false);
            }
        }
    }
    for one in ["items", "additionalProperties", "not", "contains"] {
        match o.get_mut(one) {
            Some(Value::Array(a)) => a.iter_mut().for_each(|c| scrub(c, keys, false)),
            Some(child) => scrub(child, keys, false),
            None => {}
        }
    }
}

fn shallow(v: &Value) -> Value {
    let props: Map<String, Value> = v
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(k, p)| {
            let t = p
                .get("type")
                .filter(|t| t.is_string())
                .map_or_else(|| json!({}), |t| json!({"type": t}));
            (k.clone(), t)
        })
        .collect();
    let mut out = json!({"type": "object", "properties": props});
    if let Some(r) = v.get("required") {
        out["required"] = r.clone();
    }
    out
}

fn text(t: impl Into<String>) -> Value {
    json!({"type": "text", "text": t.into()})
}

fn kb(n: usize) -> usize {
    n.div_ceil(1024)
}

/// A connector's `tools/call` result as content for the agent: text passes
/// through, pictures stay pictures, `isError` is kept, and the whole is capped.
pub(crate) fn relay_result(result: &Value) -> (bool, Vec<Value>) {
    let failed = result.get("isError").and_then(Value::as_bool) == Some(true);
    let mut out = Blocks::default();
    for c in result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let s = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("");
        match s("type") {
            "text" => out.text(s("text")),
            "image" => out.image(s("data"), s("mimeType")),
            "audio" => out.note(&format!(
                "[{} audio of {} KB left out: agents here take text and pictures]",
                s("mimeType"),
                kb(s("data").len())
            )),
            "resource" => {
                let r = c.get("resource").unwrap_or(&Value::Null);
                let rs = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("");
                if let Some(t) = r.get("text").and_then(Value::as_str) {
                    out.text(&format!("{}\n{t}", rs("uri")));
                } else if rs("mimeType").starts_with("image/") {
                    out.image(rs("blob"), rs("mimeType"));
                } else {
                    out.note(&format!(
                        "[{} ({}, {} KB) left out: not text]",
                        rs("uri"),
                        rs("mimeType"),
                        kb(rs("blob").len())
                    ));
                }
            }
            "resource_link" => {
                let mut line = format!("{} ({})", s("name"), s("uri"));
                if !s("description").is_empty() {
                    line.push_str(": ");
                    line.push_str(s("description"));
                }
                out.text(&line);
            }
            _ => out.text(&c.to_string()),
        }
    }
    if out.blocks.is_empty() {
        match result.get("structuredContent") {
            Some(sc) if !sc.is_null() => out.text(&sc.to_string()),
            _ => out.text("(the tool returned nothing)"),
        }
    }
    (!failed, out.blocks)
}

#[derive(Default)]
struct Blocks {
    blocks: Vec<Value>,
    text_used: usize,
    media_used: usize,
    cut: bool,
}

impl Blocks {
    fn text(&mut self, t: &str) {
        if self.cut {
            return;
        }
        let room = TEXT_MAX.saturating_sub(self.text_used);
        if t.len() <= room {
            self.text_used += t.len();
            self.blocks.push(text(t));
            return;
        }
        let mut at = room;
        while !t.is_char_boundary(at) {
            at -= 1;
        }
        self.text_used = TEXT_MAX;
        self.cut = true;
        self.blocks
            .push(text(format!("{}\n[cut at {} KB]", &t[..at], kb(TEXT_MAX))));
    }

    fn note(&mut self, t: &str) {
        if !self.cut {
            self.blocks.push(text(t));
        }
    }

    fn image(&mut self, data: &str, mime: &str) {
        if data.is_empty() {
            return;
        }
        if self.media_used + data.len() > MEDIA_MAX {
            self.note(&format!(
                "[a {} KB picture left out: cut at {} KB]",
                kb(data.len()),
                kb(MEDIA_MAX)
            ));
            return;
        }
        self.media_used += data.len();
        let mime = if mime.is_empty() { "image/png" } else { mime };
        self.blocks
            .push(json!({"type": "image", "data": data, "mimeType": mime}));
    }
}

/// The text of relayed content, for hooks and the run's record.
pub(crate) fn text_of(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b.get("type").and_then(Value::as_str) {
            Some("text") => b.get("text").and_then(Value::as_str).map(str::to_string),
            Some("image") => Some("[picture]".into()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_name(n: &str) -> bool {
        n.len() <= NAME_MAX
            && n.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }

    #[test]
    fn names_are_callable_short_and_distinct() {
        let long = "a_tool_whose_name_goes_on_and_on_past_any_vendor_limit";
        let pairs = [
            ("github", "get_me"),
            ("diagonaldiagrams", "render"),
            ("my server", "files.read"),
            ("my_server", "files_read"),
            ("diagonaldiagrams", long),
            ("diagonaldiagrams", &format!("{long}_2")),
        ];
        let names = agent_names(&pairs);
        assert_eq!(names[0], "github__get_me");
        assert_eq!(names[1], "diagonaldiagrams__render");
        assert_eq!(names[2], "my_server__files_read");
        assert_ne!(names[3], names[2], "a clash gets its own name");
        let unique: HashSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
        for n in &names {
            assert!(ok_name(n), "{n}");
            assert!(n.contains("__"), "{n} keeps clear of Parzi's names");
        }
        assert_eq!(names[4].len(), NAME_MAX);
        assert_eq!(agent_names(&pairs), names, "the same every time");
        for own in crate::tools::all_defs() {
            assert!(
                !crate::tools::to_mcp(&own.name).contains("__"),
                "{}",
                own.name
            );
        }
    }

    #[test]
    fn schemas_come_out_as_plain_objects() {
        let s = clean_schema(&json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {"a": {"type": "string", "$schema": "x"}, "title": {"type": "string"}},
            "required": ["a", "gone"],
        }));
        assert_eq!(
            s,
            json!({"type": "object", "properties": {"a": {"type": "string"}, "title": {"type": "string"}}, "required": ["a"]})
        );
        assert_eq!(
            clean_schema(&Value::Null),
            json!({"type": "object", "properties": {}})
        );
        let either = clean_schema(&json!({"anyOf": [
            {"properties": {"x": {"type": "number"}}, "required": ["x"]},
            {"properties": {"y": {"type": "number"}}},
        ]}));
        assert_eq!(either["type"], "object");
        assert!(either["properties"]["x"].is_object() && either["properties"]["y"].is_object());
        assert!(either.get("anyOf").is_none() && either.get("required").is_none());
        let wordy: Map<String, Value> = (0..400)
            .map(|i| {
                (
                    format!("field{i}"),
                    json!({"type": "string", "description": "x".repeat(100)}),
                )
            })
            .collect();
        let mut wordy = wordy;
        wordy.insert(
            "title".into(),
            json!({"type": "string", "description": "kept"}),
        );
        let big = clean_schema(&json!({"type": "object", "properties": wordy}));
        assert!(
            big.to_string().len() <= SCHEMA_MAX,
            "{}",
            big.to_string().len()
        );
        assert_eq!(big["properties"]["field7"]["type"], "string");
        assert_eq!(big["properties"]["title"], json!({"type": "string"}));
    }

    #[test]
    fn results_keep_text_pictures_and_failure() {
        let (ok, blocks) = relay_result(&json!({"content": [
            {"type": "text", "text": "hello"},
            {"type": "image", "data": "AAAA", "mimeType": "image/png"},
        ]}));
        assert!(ok);
        assert_eq!(blocks[0], json!({"type": "text", "text": "hello"}));
        assert_eq!(
            blocks[1],
            json!({"type": "image", "data": "AAAA", "mimeType": "image/png"})
        );
        let (ok, blocks) =
            relay_result(&json!({"isError": true, "content": [{"type": "text", "text": "no"}]}));
        assert!(!ok);
        assert_eq!(text_of(&blocks), "no");
        let (_, blocks) = relay_result(&json!({"structuredContent": {"a": 1}}));
        assert_eq!(text_of(&blocks), r#"{"a":1}"#);
        let (_, blocks) = relay_result(&json!({"content": [
            {"type": "text", "text": "é".repeat(TEXT_MAX)},
            {"type": "text", "text": "after the cut"},
        ]}));
        let all = text_of(&blocks);
        assert!(all.ends_with(&format!("[cut at {} KB]", TEXT_MAX / 1024)));
        assert!(all.len() < TEXT_MAX + 64);
        assert!(!all.contains("after the cut"));
        let huge = "A".repeat(MEDIA_MAX + 4);
        let (_, blocks) = relay_result(&json!({"content": [
            {"type": "image", "data": huge, "mimeType": "image/png"},
        ]}));
        assert_eq!(blocks[0]["type"], "text");
        assert!(text_of(&blocks).contains("picture left out"));
    }
}
