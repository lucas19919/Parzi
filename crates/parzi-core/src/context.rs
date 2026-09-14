use serde::{Deserialize, Serialize};

use base64::Engine as _;

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
    /// Images ride with the message (newest user turn). Empty = text only,
    /// so every existing adapter keeps working unchanged.
    #[serde(default)]
    pub images: Vec<ImageData>,
}

/// One image for a vision-capable model: base64 bytes + MIME type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub media_type: String,
    pub data_b64: String,
}

impl ImageData {
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.media_type, self.data_b64)
    }
}

#[derive(Debug, Clone)]
pub struct AttachedFile {
    pub path: String,
    pub snippet: String,
    /// Set when the attachment is an image (bytes, not text).
    pub image: Option<ImageData>,
}

impl AttachedFile {
    pub fn text(path: String, snippet: String) -> Self {
        Self {
            path,
            snippet,
            image: None,
        }
    }

    pub fn is_image(&self) -> bool {
        self.image.is_some()
    }
}

/// Image extensions Parzi accepts as vision attachments (SVG is XML text
/// and travels the snippet path instead).
pub fn image_media_type(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()?.to_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

/// Raw bytes above this ride a downscale pass instead of going out whole.
pub const IMAGE_RAW_CAP: u64 = 4_000_000;
/// Longest edge after the downscale pass.
const IMAGE_MAX_DIM: u32 = 1568;

/// Encode attachment bytes: images become base64 (downscaled past the cap),
/// everything else stays None (the caller reads text).
pub fn encode_image(path: &str, bytes: &[u8]) -> Option<ImageData> {
    let media = image_media_type(path)?.to_string();
    if (bytes.len() as u64) <= IMAGE_RAW_CAP {
        return Some(ImageData {
            media_type: media,
            data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
        });
    }
    // Oversized: downscale to JPEG so a 4K screenshot still sends.
    let img = image::load_from_memory(bytes).ok()?;
    let scaled = img.resize(
        IMAGE_MAX_DIM,
        IMAGE_MAX_DIM,
        image::imageops::FilterType::Triangle,
    );
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85)
        .encode_image(&scaled)
        .ok()?;
    Some(ImageData {
        media_type: "image/jpeg".to_string(),
        data_b64: base64::engine::general_purpose::STANDARD.encode(&buf),
    })
}

/// Read one file off disk as an image attachment (for UI previews and
/// single-file flows). Returns None for non-images, missing files and
/// anything the decoder path rejects.
pub fn encode_image_file(path: &std::path::Path) -> Option<ImageData> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let bytes = std::fs::read(path).ok()?;
    encode_image(&name, &bytes)
}
/// Read @-attached files (cap 8), resolved against `cwd`. Text travels as a
/// 12k-char snippet; images travel as base64 (downscaled past the raw cap).
/// Unreadable paths are skipped; every returned file is send-ready.
pub fn read_attachments(cwd: &std::path::Path, paths: &[String]) -> Vec<AttachedFile> {
    paths
        .iter()
        .take(8)
        .filter_map(|p| {
            // Contain to cwd (lexical) — same rule as the tool sandbox.
            let full = cwd.join(p);
            let bytes = std::fs::read(&full).ok()?;
            if image_media_type(p).is_some() {
                return encode_image(p, &bytes).map(|image| AttachedFile {
                    path: p.clone(),
                    snippet: String::new(),
                    image: Some(image),
                });
            }
            let text = String::from_utf8_lossy(&bytes);
            Some(AttachedFile::text(
                p.clone(),
                text.chars().take(12_000).collect(),
            ))
        })
        .collect()
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
        let mut images: Vec<ImageData> = vec![];
        for f in &self.files {
            if let Some(img) = &f.image {
                // ~1k tokens per image estimate; never drops the text budget.
                if used + 1_000 > token_limit {
                    file_text.push_str("<file truncated: budget exceeded>\n");
                    used += 8;
                    break;
                }
                used += 1_000;
                images.push(img.clone());
                file_text.push_str(&format!("<image path=\"{}\" />\n", f.path));
                continue;
            }
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
                    images: vec![],
                }),
                Event::Assistant { text, .. } => Some(ChatMessage {
                    role: Role::Assistant,
                    content: text.clone(),
                    images: vec![],
                }),
                Event::ToolCall { name, args, .. } => Some(ChatMessage {
                    role: Role::Assistant,
                    content: format!(
                        "[tool:{name} {}]",
                        serde_json::to_string(args).unwrap_or_default()
                    ),
                    images: vec![],
                }),
                Event::ToolResult { name, output, .. } => Some(ChatMessage {
                    role: Role::Tool,
                    content: format!("[result:{name} untrusted]\n{output}"),
                    images: vec![],
                }),
                Event::Checkpoint { summary } => Some(ChatMessage {
                    role: Role::System,
                    content: format!("[checkpoint] {summary}"),
                    images: vec![],
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
        // Images ride the newest user turn (vision models see them there);
        // with no user turn yet they open their own.
        if !images.is_empty() {
            if let Some(last_user) = picked.iter_mut().rev().find(|m| m.role == Role::User) {
                last_user.images = images;
            } else {
                picked.push(ChatMessage {
                    role: Role::User,
                    content: "[attached images]".to_string(),
                    images,
                });
            }
        }
        if !file_text.is_empty() {
            picked.insert(
                0,
                ChatMessage {
                    role: Role::System,
                    content: format!("[context]\n{file_text}"),
                    images: vec![],
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
