//! Image attachments: media-type routing, base64 encoding, downscale
//! past the raw cap, and context assembly onto the newest user turn.

use parzi_core::context::{
    encode_image, image_media_type, read_attachments, AttachedFile, ChatMessage, ContextBuilder,
    Role,
};
use parzi_core::store::Event;

fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> String {
    std::fs::write(dir.join(name), bytes).unwrap();
    name.to_string()
}

#[test]
fn media_types_route() {
    assert_eq!(image_media_type("a.png"), Some("image/png"));
    assert_eq!(image_media_type("A.JPG"), Some("image/jpeg"));
    assert_eq!(image_media_type("a.jpeg"), Some("image/jpeg"));
    assert_eq!(image_media_type("a.gif"), Some("image/gif"));
    assert_eq!(image_media_type("a.webp"), Some("image/webp"));
    assert_eq!(image_media_type("a.bmp"), Some("image/bmp"));
    assert_eq!(image_media_type("a.svg"), None); // XML text: snippet path
    assert_eq!(image_media_type("a.rs"), None);
    assert_eq!(image_media_type("noext"), None);
}

#[test]
fn small_image_encodes_raw() {
    let img = encode_image("shot.png", &[1, 2, 3, 4]).unwrap();
    assert_eq!(img.media_type, "image/png");
    assert!(img.data_url().starts_with("data:image/png;base64,"));
    assert_eq!(image_media_type("x.txt"), None);
}

#[test]
fn read_mixes_text_and_images() {
    let dir = tempfile::tempdir().unwrap();
    let text = write(dir.path(), "note.md", b"# hello");
    let png = write(dir.path(), "shot.png", &[7; 64]);
    let missing = "gone.png".to_string();
    let files = read_attachments(dir.path(), &[text, png, missing]);
    assert_eq!(files.len(), 2, "missing files stay skipped");
    assert!(files[0].image.is_none());
    assert!(files[0].snippet.contains("hello"));
    assert!(files[1].image.is_some());
    assert_eq!(files[1].image.as_ref().unwrap().media_type, "image/png");
}

#[test]
fn oversized_image_downscales_to_jpeg() {
    // Noisy 1600x1600 RGB: PNG lands well past the raw cap, stays decodable.
    use image::{ExtendedColorType, ImageEncoder};
    let mut state: u64 = 0x1234_5678_9abc_def1;
    let mut px = vec![0u8; 1600 * 1600 * 3];
    for b in px.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *b = (state >> 11) as u8;
    }
    let mut buf = Vec::new();
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(&px, 1600, 1600, ExtendedColorType::Rgb8)
        .unwrap();
    assert!(
        (buf.len() as u64) > parzi_core::context::IMAGE_RAW_CAP,
        "fixture must exceed the raw cap ({} bytes)",
        buf.len()
    );
    let img = encode_image("big.png", &buf).unwrap();
    assert_eq!(img.media_type, "image/jpeg");
    assert!(!img.data_b64.is_empty());
}

#[test]
fn images_ride_newest_user_turn() {
    let history = vec![
        Event::User {
            text: "first".into(),
        },
        Event::Assistant {
            text: "reply".into(),
            done: true,
        },
        Event::User {
            text: "look at this".into(),
        },
    ];
    let b = ContextBuilder {
        system_parts: vec![],
        history,
        files: vec![AttachedFile {
            path: "shot.png".into(),
            snippet: String::new(),
            image: encode_image("shot.png", &[1, 2, 3]),
        }],
    };
    let ctx = b.assemble(100_000);
    let users: Vec<&ChatMessage> = ctx
        .messages
        .iter()
        .filter(|m| m.role == Role::User)
        .collect();
    assert_eq!(users.len(), 2);
    assert!(users[0].images.is_empty(), "older turn stays text-only");
    assert_eq!(users[1].images.len(), 1, "newest user turn carries it");
    assert_eq!(users[1].images[0].media_type, "image/png");
    // The model still sees a filename marker in the text channel.
    assert!(ctx.messages.iter().any(|m| m.content.contains("<image")));
}

#[test]
fn chat_message_stays_backward_compatible() {
    // Old serialized transcripts have no `images` key.
    let m: ChatMessage = serde_json::from_str(r#"{"role":"user","content":"hi"}"#).unwrap();
    assert!(m.images.is_empty());
}
