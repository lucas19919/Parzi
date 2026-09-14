//! Native image blocks per wire format. One helper each so the five
//! adapters can't disagree on shape. Text-only when no images are attached.

use parzi_core::context::ImageData;

/// OpenAI chat-completions shape (xai, opencode Go/Zen-free): a string when
/// imageless, else `[{type: text}, {type: image_url, image_url: {url}}]`.
pub fn openai_content(text: &str, images: &[ImageData]) -> serde_json::Value {
    if images.is_empty() {
        return serde_json::Value::String(text.to_string());
    }
    let mut parts = vec![serde_json::json!({"type": "text", "text": text})];
    for img in images {
        parts.push(serde_json::json!({
            "type": "image_url",
            "image_url": {"url": img.data_url()},
        }));
    }
    serde_json::Value::Array(parts)
}

/// Anthropic Messages shape (claude, opencode `messages` kind): a string
/// when imageless, else `[{type: text}, {type: image, source: {base64}}]`.
pub fn anthropic_content(text: &str, images: &[ImageData]) -> serde_json::Value {
    if images.is_empty() {
        return serde_json::Value::String(text.to_string());
    }
    let mut parts = vec![serde_json::json!({"type": "text", "text": text})];
    for img in images {
        parts.push(serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": img.media_type,
                "data": img.data_b64,
            },
        }));
    }
    serde_json::Value::Array(parts)
}

/// Gemini shape (antigravity): `[{text}]` plus `[{inlineData}]` per image.
pub fn gemini_parts(text: &str, images: &[ImageData]) -> serde_json::Value {
    let mut parts = vec![serde_json::json!({"text": text})];
    for img in images {
        parts.push(serde_json::json!({
            "inlineData": {"mimeType": img.media_type, "data": img.data_b64},
        }));
    }
    serde_json::Value::Array(parts)
}

/// Responses-API shape (codex, opencode `responses` kind): a string when
/// imageless, else `[{type: input_text}, {type: input_image}]`.
pub fn responses_content(text: &str, images: &[ImageData]) -> serde_json::Value {
    if images.is_empty() {
        return serde_json::Value::String(text.to_string());
    }
    let mut parts = vec![serde_json::json!({"type": "input_text", "text": text})];
    for img in images {
        parts.push(serde_json::json!({
            "type": "input_image",
            "image_url": img.data_url(),
        }));
    }
    serde_json::Value::Array(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img() -> ImageData {
        ImageData {
            media_type: "image/png".into(),
            data_b64: "QUJD".into(),
        }
    }

    #[test]
    fn imageless_stays_string() {
        assert_eq!(
            openai_content("hi", &[]),
            serde_json::Value::String("hi".into())
        );
        assert_eq!(
            anthropic_content("hi", &[]),
            serde_json::Value::String("hi".into())
        );
        assert_eq!(
            responses_content("hi", &[]),
            serde_json::Value::String("hi".into())
        );
    }

    #[test]
    fn openai_shape() {
        let v = openai_content("see", &[img()]);
        assert_eq!(v[0]["type"], "text");
        assert_eq!(v[1]["type"], "image_url");
        assert!(v[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }

    #[test]
    fn anthropic_shape() {
        let v = anthropic_content("see", &[img()]);
        assert_eq!(v[1]["type"], "image");
        assert_eq!(v[1]["source"]["type"], "base64");
        assert_eq!(v[1]["source"]["media_type"], "image/png");
        assert_eq!(v[1]["source"]["data"], "QUJD");
    }

    #[test]
    fn gemini_shape() {
        let v = gemini_parts("see", &[img()]);
        assert_eq!(v[0]["text"], "see");
        assert_eq!(v[1]["inlineData"]["mimeType"], "image/png");
        assert_eq!(v[1]["inlineData"]["data"], "QUJD");
    }

    #[test]
    fn responses_shape() {
        let v = responses_content("see", &[img()]);
        assert_eq!(v[0]["type"], "input_text");
        assert_eq!(v[1]["type"], "input_image");
        assert!(v[1]["image_url"].as_str().unwrap().starts_with("data:"));
    }
}
