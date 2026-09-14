use serde::{Deserialize, Serialize};

use crate::store::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct AttachedFile {
    pub path: String,
    pub snippet: String,
}

/// Provider-agnostic context assembly. One algorithm for every adapter.
pub struct ContextBuilder {
    pub system_parts: Vec<String>,
    pub history: Vec<Event>,
    pub files: Vec<AttachedFile>,
}

#[derive(Debug, Clone)]
pub struct AssembledContext {
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub estimated_tokens: u64,
}

impl ContextBuilder {
    pub fn estimate(text: &str) -> u64 {
        (text.len() as u64 / 4).max(1)
    }

    /// Fill newest-first under `token_limit`. Pinned system parts never drop.
    pub fn assemble(&self, token_limit: u64) -> AssembledContext {
        let system = self.system_parts.join("\n\n---\n\n");
        let mut used = Self::estimate(&system);
        let mut file_text = String::new();
        for f in &self.files {
            let chunk = format!("<file path=\"{}\">\n{}</file>\n", f.path, f.snippet);
            if used + Self::estimate(&chunk) > token_limit {
                file_text.push_str("<file truncated: budget exceeded>\n");
                used += 8;
                break;
            }
            used += Self::estimate(&chunk);
            file_text.push_str(&chunk);
        }

        // Walk history newest-first, then reverse to restore order.
        let mut picked: Vec<ChatMessage> = vec![];
        for e in self.history.iter().rev() {
            let msg = match e {
                Event::User { text } => Some(ChatMessage {
                    role: Role::User,
                    content: text.clone(),
                }),
                Event::Assistant { text, .. } => Some(ChatMessage {
                    role: Role::Assistant,
                    content: text.clone(),
                }),
                Event::ToolCall { name, args, .. } => Some(ChatMessage {
                    role: Role::Assistant,
                    content: format!(
                        "[tool:{name} {}]",
                        serde_json::to_string(args).unwrap_or_default()
                    ),
                }),
                Event::ToolResult { name, output, .. } => Some(ChatMessage {
                    role: Role::Tool,
                    content: format!("[result:{name} untrusted]\n{output}"),
                }),
                Event::Checkpoint { summary } => Some(ChatMessage {
                    role: Role::System,
                    content: format!("[checkpoint] {summary}"),
                }),
                Event::System { .. }
                | Event::Widget { .. }
                | Event::Artifact { .. }
                | Event::Reasoning { .. }
                | Event::RouteTransition { .. } => None,
            };
            if let Some(m) = msg {
                let cost = Self::estimate(&m.content);
                if used + cost > token_limit {
                    break;
                }
                used += cost;
                picked.push(m);
            }
        }
        picked.reverse();
        if !file_text.is_empty() {
            picked.insert(
                0,
                ChatMessage {
                    role: Role::System,
                    content: format!("[context]\n{file_text}"),
                },
            );
        }
        AssembledContext {
            system,
            messages: picked,
            estimated_tokens: used,
        }
    }

    /// Digest-based compaction: keep the newest `keep_last` events raw,
    /// fold older ones into one Checkpoint. LLM summarization plugs in later
    /// behind this same signature.
    pub fn compact(events: &[Event], keep_last: usize) -> Vec<Event> {
        if events.len() <= keep_last {
            return events.to_vec();
        }
        let cut = events.len() - keep_last;
        let mut digest = String::from("Earlier conversation digest:\n");
        for e in &events[..cut] {
            let line = match e {
                Event::User { text } => format!("- user: {}\n", head(text)),
                Event::Assistant { text, .. } => format!("- assistant: {}\n", head(text)),
                Event::ToolCall { name, .. } => format!("- tool: {name}\n"),
                Event::ToolResult { name, ok, .. } => format!("- result: {name} ok={ok}\n"),
                Event::Artifact {
                    id, title, version, ..
                } => format!("- artifact: {title} ({id} v{version})\n"),
                Event::Checkpoint { summary } => format!("- checkpoint: {summary}\n"),
                Event::System { .. }
                | Event::Widget { .. }
                | Event::Reasoning { .. }
                | Event::RouteTransition { .. } => continue,
            };
            digest.push_str(&line);
        }
        let mut out = vec![Event::Checkpoint { summary: digest }];
        out.extend(events[cut..].iter().cloned());
        out
    }
}

fn head(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(160).collect()
}
