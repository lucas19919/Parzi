//! Zen wire runners: one SSE parser per endpoint kind (chat / messages /
//! responses). Split from opencode.rs for the 400-line file bar; the
//! adapter (auth, catalog, dispatch) stays there.

use futures::StreamExt;
use parzi_core::context::Role;
use parzi_core::error::{ParziError, Result};

use super::opencode::Opencode;
use crate::types::{ChatReq, StreamEvent, desanitize_tool, sanitize_tool};

/// POST helper: short error bodies stay routable (429/overload hop, 4xx halt).
async fn post(
    client: reqwest::Client,
    url: String,
    body: serde_json::Value,
) -> Result<reqwest::Response> {
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| ParziError::Provider("opencode".into(), format!("request: {e}")))?;
    if !resp.status().is_success() {
        let code = resp.status();
        let retry = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(|r| format!(" (retry after {r})"))
            .unwrap_or_default();
        let text = resp.text().await.unwrap_or_default();
        let short: String = text.chars().take(300).collect();
        return Err(ParziError::Provider(
            "opencode".into(),
            format!("http {code}{retry}: {short}"),
        ));
    }
    Ok(resp)
}

fn sse_data(line: &str) -> Option<String> {
    let data = line.strip_prefix("data:")?.trim();
    if data == "[DONE]" {
        return None;
    }
    Some(data.to_string())
}

impl Opencode {
    pub(crate) async fn run_chat(
        &self,
        model: String,
        base: String,
        session: String,
        req: ChatReq,
        tx: crate::types::EventTx,
    ) -> Result<()> {
        let client = self.client(&session, false)?;
        let url = format!("{}/chat/completions", base.trim_end_matches('/'));
        let mut body = serde_json::json!({
            "model": model,
            "messages": self.openai_messages(&req),
            "max_tokens": req.max_tokens,
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        let tools = self.openai_tools(&req);
        if !tools.is_empty() {
            body["tools"] = serde_json::Value::Array(tools);
        }
        let resp = post(client, url, body).await?;
        // Accumulate tool-call argument fragments keyed by index.
        let mut calls: std::collections::HashMap<usize, (String, String, String)> =
            std::collections::HashMap::new();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| ParziError::Provider("opencode".into(), format!("stream: {e}")))?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim().to_string();
                buf.drain(..=pos);
                let Some(data) = sse_data(&line) else { continue };
                let v: serde_json::Value = match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                // NOTE: Zen co-locates `usage` and `delta` in one chunk —
                // both must be read; never `continue` after usage.
                if let Some(u) = v.get("usage") {
                    let pin = u.get("prompt_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                    let pout = u.get("completion_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                    let _ = tx.send(Ok(StreamEvent::Usage { tokens_in: pin, tokens_out: pout }));
                }
                let delta = match v
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|c| c.first())
                    .and_then(|c| c.get("delta"))
                {
                    Some(d) => d,
                    None => continue,
                };
                if let Some(t) = delta.get("content").and_then(|c| c.as_str()) {
                    if !t.is_empty() {
                        let _ = tx.send(Ok(StreamEvent::Text(t.to_string())));
                    }
                }
                // Interleaved thinking (verified live: content null + reasoning).
                if let Some(t) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                    if !t.is_empty() {
                        let _ = tx.send(Ok(StreamEvent::Reasoning(t.to_string())));
                    }
                }
                if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in tcs {
                        let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                        let e = calls
                            .entry(idx)
                            .or_insert_with(|| (format!("call_{idx}"), String::new(), String::new()));
                        if let Some(call_id) = tc.get("id").and_then(|i| i.as_str()) {
                            e.0 = call_id.to_string();
                        }
                        if let Some(f) = tc.get("function") {
                            if let Some(n) = f.get("name").and_then(|n| n.as_str()) {
                                e.1 = n.to_string();
                            }
                            if let Some(a) = f.get("arguments").and_then(|a| a.as_str()) {
                                e.2.push_str(a);
                            }
                        }
                    }
                }
            }
        }
        let mut ordered: Vec<_> = calls.into_iter().collect();
        ordered.sort_by_key(|(i, _)| *i);
        for (_, (call_id, name, args_str)) in ordered {
            if name.is_empty() {
                continue;
            }
            let args = serde_json::from_str(&args_str).unwrap_or(serde_json::Value::Null);
            let name = desanitize_tool(&req.tools, &name);
            let _ = tx.send(Ok(StreamEvent::ToolCall { id: call_id, name, args }));
        }
        Ok(())
    }

    pub(crate) async fn run_messages(
        &self,
        model: String,
        base: String,
        session: String,
        req: ChatReq,
        tx: crate::types::EventTx,
    ) -> Result<()> {
        let client = self.client(&session, true)?;
        let url = format!("{}/messages", base.trim_end_matches('/'));
        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::Assistant => "assistant",
                    _ => "user",
                };
                serde_json::json!({"role": role, "content": crate::images::anthropic_content(&m.content, &m.images)})
            })
            .collect();
        let tools: Vec<serde_json::Value> = req
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": sanitize_tool(&t.name),
                    "description": t.description,
                    "input_schema": t.schema,
                })
            })
            .collect();
        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": self.output_cap(&model, req.max_tokens),
            "messages": messages,
            "tools": tools,
            "stream": true,
        });
        if !req.system.trim().is_empty() {
            body["system"] = serde_json::json!(req.system);
        }
        let resp = post(client, url, body).await?;
        let mut tool_json = String::new();
        let mut tool_name = String::new();
        let mut tool_id = String::new();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| ParziError::Provider("opencode".into(), format!("stream: {e}")))?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim().to_string();
                buf.drain(..=pos);
                let Some(data) = sse_data(&line) else { continue };
                let v: serde_json::Value = match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                    "content_block_delta" => {
                        let d = v.get("delta").cloned().unwrap_or_default();
                        match d.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                            "text_delta" => {
                                if let Some(t) = d.get("text").and_then(|t| t.as_str()) {
                                    let _ = tx.send(Ok(StreamEvent::Text(t.to_string())));
                                }
                            }
                            "thinking_delta" | "signature_delta" => {
                                if let Some(t) = d.get("thinking").and_then(|t| t.as_str()) {
                                    if !t.is_empty() {
                                        let _ = tx.send(Ok(StreamEvent::Reasoning(t.to_string())));
                                    }
                                }
                            }
                            "input_json_delta" => {
                                if let Some(p) = d.get("partial_json").and_then(|p| p.as_str()) {
                                    tool_json.push_str(p);
                                }
                            }
                            _ => {}
                        }
                    }
                    "content_block_start" => {
                        if let Some(b) = v.get("content_block") {
                            if b.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                                tool_id = b
                                    .get("id")
                                    .and_then(|i| i.as_str())
                                    .unwrap_or("call_0")
                                    .to_string();
                                tool_name = b
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                tool_json.clear();
                            }
                        }
                    }
                    "message_start" => {
                        if let Some(u) = v
                            .get("message")
                            .and_then(|m| m.get("usage"))
                            .and_then(|u| u.as_object())
                        {
                            let inp = u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                            let _ =
                                tx.send(Ok(StreamEvent::Usage { tokens_in: inp, tokens_out: 0 }));
                        }
                    }
                    "message_delta" => {
                        if let Some(u) = v.get("usage") {
                            let out =
                                u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                            let _ =
                                tx.send(Ok(StreamEvent::Usage { tokens_in: 0, tokens_out: out }));
                        }
                    }
                    _ => {}
                }
            }
        }
        if !tool_name.is_empty() {
            let args = serde_json::from_str(&tool_json).unwrap_or(serde_json::Value::Null);
            let name = desanitize_tool(&req.tools, &tool_name);
            let _ = tx.send(Ok(StreamEvent::ToolCall { id: tool_id, name, args }));
        }
        Ok(())
    }

    pub(crate) async fn run_responses(
        &self,
        model: String,
        base: String,
        session: String,
        req: ChatReq,
        tx: crate::types::EventTx,
    ) -> Result<()> {
        let client = self.client(&session, false)?;
        let url = format!("{}/responses", base.trim_end_matches('/'));
        let input: Vec<serde_json::Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::Assistant => "assistant",
                    _ => "user",
                };
                serde_json::json!({"role": role, "content": crate::images::responses_content(&m.content, &m.images)})
            })
            .collect();
        let tools: Vec<serde_json::Value> = req
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function", "name": sanitize_tool(&t.name),
                    "description": t.description, "parameters": t.schema,
                })
            })
            .collect();
        let mut body = serde_json::json!({
            "model": model, "input": input, "tools": tools,
            "stream": true,
            "max_output_tokens": self.output_cap(&model, req.max_tokens),
        });
        if !req.system.trim().is_empty() {
            body["instructions"] = serde_json::json!(req.system);
        }
        let resp = post(client, url, body).await?;
        let mut arg_buf = String::new();
        let mut fn_name = String::new();
        let mut call_id = String::new();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| ParziError::Provider("opencode".into(), format!("stream: {e}")))?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim().to_string();
                buf.drain(..=pos);
                let Some(data) = sse_data(&line) else { continue };
                let v: serde_json::Value = match serde_json::from_str(&data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                    "response.output_text.delta" => {
                        if let Some(t) = v.get("delta").and_then(|d| d.as_str()) {
                            let _ = tx.send(Ok(StreamEvent::Text(t.to_string())));
                        }
                    }
                    "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                        if let Some(t) = v.get("delta").and_then(|d| d.as_str()) {
                            let _ = tx.send(Ok(StreamEvent::Reasoning(t.to_string())));
                        }
                    }
                    "response.function_call_arguments.delta" => {
                        if let Some(d) = v.get("delta").and_then(|d| d.as_str()) {
                            arg_buf.push_str(d);
                        }
                    }
                    "response.output_item.done" => {
                        if let Some(item) = v.get("item") {
                            if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                                fn_name =
                                    item.get("name").and_then(|n| n.as_str()).unwrap_or("").into();
                                call_id = item
                                    .get("call_id")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("call_0")
                                    .into();
                                arg_buf = item
                                    .get("arguments")
                                    .and_then(|a| a.as_str())
                                    .unwrap_or("")
                                    .into();
                            }
                        }
                    }
                    "response.completed" => {
                        if let Some(u) = v.get("response").and_then(|r| r.get("usage")) {
                            let pin =
                                u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                            let pout =
                                u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
                            let _ =
                                tx.send(Ok(StreamEvent::Usage { tokens_in: pin, tokens_out: pout }));
                        }
                    }
                    _ => {}
                }
            }
        }
        if !fn_name.is_empty() {
            let args = serde_json::from_str(&arg_buf).unwrap_or(serde_json::Value::Null);
            let name = desanitize_tool(&req.tools, &fn_name);
            let _ = tx.send(Ok(StreamEvent::ToolCall { id: call_id, name, args }));
        }
        Ok(())
    }
}
