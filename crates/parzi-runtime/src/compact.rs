//! Compaction: the model summarizes a thread, the summary is appended as a
//! `Checkpoint`, and every later request is read from that checkpoint on
//! (`context::since_checkpoint`). The transcript on disk stays whole.

use parzi_core::context::{since_checkpoint, ChatMessage, ContextBuilder, Role, COMPACT_PROMPT};
use parzi_core::error::{ParziError, Result};
use parzi_core::store::Event;
use parzi_providers::{ChatReq, Provider, StreamEvent};

/// Share of the usable window at which a run compacts before its next step.
pub const AUTO_COMPACT_PERCENT: u64 = 85;

/// Tokens the summary itself may take.
const SUMMARY_TOKENS: u32 = 16_384;

/// The window a request may fill: the model's context minus room for the
/// reply (never more than a quarter of the window).
pub fn usable_window(context_limit: u64, max_tokens: u32) -> u64 {
    context_limit.saturating_sub(u64::from(max_tokens).min(context_limit / 4))
}

/// Whether a thread this full should compact before its next request.
pub fn should_compact(context_tokens: u64, context_limit: u64, max_tokens: u32) -> bool {
    let usable = usable_window(context_limit, max_tokens);
    usable > 0 && context_tokens.saturating_mul(100) >= usable.saturating_mul(AUTO_COMPACT_PERCENT)
}

/// Ask `provider` for a summary of everything since the last checkpoint.
/// `focus` is the person's steer ("keep the API design"), may be empty.
pub async fn summarize(
    provider: &dyn Provider,
    model: &str,
    events: &[Event],
    focus: &str,
    context_limit: u64,
    session: &str,
) -> Result<String> {
    // Leave room for the prompt and the summary; the estimate is rough.
    let budget = usable_window(context_limit, SUMMARY_TOKENS) * 8 / 10;
    let mut ctx = ContextBuilder {
        system_parts: vec![],
        history: since_checkpoint(events).to_vec(),
        files: vec![],
    }
    .assemble(budget);
    let mut ask = COMPACT_PROMPT.to_string();
    if !focus.trim().is_empty() {
        ask.push_str(&format!("\n\nThe person asks you to focus on: {}", focus.trim()));
    }
    ctx.messages.push(ChatMessage {
        role: Role::User,
        content: ask,
        images: vec![],
    });
    let req = ChatReq {
        model: model.to_string(),
        system: "You compact a working session between a person and a coding agent into a \
                 summary the agent continues from."
            .into(),
        messages: ctx.messages,
        tools: vec![],
        max_tokens: SUMMARY_TOKENS,
        effort: "low".into(),
        session: session.to_string(),
    };
    let mut rx = provider.chat_stream(req).await?;
    let mut text = String::new();
    while let Some(ev) = rx.recv().await {
        match ev {
            Ok(StreamEvent::Text(t)) => text.push_str(&t),
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }
    let text = text.trim();
    if text.is_empty() {
        return Err(ParziError::Provider(
            provider.id().into(),
            "the model returned an empty summary".into(),
        ));
    }
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compacts_near_the_top_of_the_usable_window() {
        // 200k window, 16k reply room: usable 184k, threshold ~156k.
        assert!(!should_compact(150_000, 200_000, 16_384));
        assert!(should_compact(160_000, 200_000, 16_384));
        // Reply room never eats more than a quarter of a small window.
        assert_eq!(usable_window(100_000, 262_144), 75_000);
        assert!(!should_compact(0, 0, 16_384));
    }
}
