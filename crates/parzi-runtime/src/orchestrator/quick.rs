//! One quick turn on a small model, with no tools: naming a session from its
//! first message, and tidying a message the person sent for display (math as
//! LaTeX, code as code). Both run beside the real work and never hold it up;
//! when no agent offers a small model, nothing happens.

use std::sync::Arc;
use std::time::Duration;

use parzi_core::store::Event;
use parzi_core::text::{fit_name, NAME_WORDS};
use parzi_providers::{
    PermissionDecision, PermissionGate, PermissionRequest, ProviderEvent, ProviderStatus, State,
    TurnSpec,
};
use tokio_util::sync::CancellationToken;

use super::Orchestrator;
use crate::handler::RunEvent;

/// Words in a model's id or name that mark it as one of an agent's small,
/// fast models.
const SMALL: [&str; 7] = ["haiku", "mini", "flash", "lite", "nano", "small", "fast"];
const LIMIT: Duration = Duration::from_secs(60);

struct NoTools;

#[async_trait::async_trait]
impl PermissionGate for NoTools {
    async fn decide(&self, _request: PermissionRequest) -> PermissionDecision {
        PermissionDecision::Deny("this quick turn uses no tools; answer in text".into())
    }
}

/// The small model to use: the preferred agent's first, then any ready
/// agent's, as (agent, model id).
pub(crate) fn small_model(all: &[ProviderStatus], prefer: &str) -> Option<(String, String)> {
    let small = |s: &ProviderStatus| {
        s.models
            .iter()
            .find(|m| {
                let both = format!("{} {}", m.id, m.name).to_ascii_lowercase();
                SMALL.iter().any(|w| both.contains(w))
            })
            .map(|m| (s.provider.clone(), m.id.clone()))
    };
    let ready = all.iter().filter(|s| s.state == State::Ready);
    ready
        .clone()
        .filter(|s| s.provider == prefer)
        .find_map(small)
        .or_else(|| ready.filter(|s| s.provider != prefer).find_map(small))
}

/// The small models to try, in order: the one chosen in Settings, its
/// fallback (each only when its agent is ready), then the automatic pick.
pub(crate) fn candidates(
    chosen: &parzi_core::config::QuickModels,
    all: &[ProviderStatus],
    prefer: &str,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for spec in [&chosen.model, &chosen.fallback] {
        let Some((agent, model)) = spec.trim().split_once('/') else {
            continue;
        };
        let ready = all
            .iter()
            .any(|s| s.provider == agent && s.state == State::Ready);
        let pair = (agent.to_string(), model.to_string());
        if ready && !model.is_empty() && !out.contains(&pair) {
            out.push(pair);
        }
    }
    if let Some(auto) = small_model(all, prefer).filter(|p| !out.contains(p)) {
        out.push(auto);
    }
    out
}

/// Does this message hold math or code worth setting properly?
pub(crate) fn worth_tidying(text: &str) -> bool {
    let t = text.trim();
    if t.len() < 12 || t.len() > 4_000 || t.contains("$$") || t.contains("\\(") {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    let greek = [
        "alpha", "beta", "gamma", "delta", "epsilon", "theta", "lambda", "sigma", "omega", "rho",
        "mu ", "nu ", "phi", "psi", "tau",
    ];
    let mathy = [
        "^", "_", "/dt", "/dx", "d/d", "sqrt", "integral", "∫", "∑", "∂", "≈", "≤", "≥",
    ];
    let has_eq = t.contains('=') && t.chars().any(|c| c.is_ascii_alphabetic());
    let words = greek.iter().any(|g| lower.contains(g));
    let marks = mathy.iter().any(|m| t.contains(m));
    has_eq && (words || marks) || (words && marks)
}

const NAMER: &str = "You name chat sessions. Reply with only a name for the conversation that starts with the message below: 2 to 5 words, sentence case, no quotes, no full stop, at most 60 characters. Name what it is about, not that it is a question.";

const TIDIER: &str = "You set a person's chat message properly for display. Keep their words, meaning, order and tone exactly; fix nothing about the content. Only change notation: write mathematics as LaTeX, small expressions inline inside $...$, and any equation with a fraction, integral or sum that stands on its own as a display formula, $$...$$ on its own line, so it is large enough to read. Use \\frac, never \\tfrac or a slash for a fraction. Put names of code, files and commands in backticks, and lists as Markdown lists where they already were lists. Reply with only the rewritten message. If there is nothing to set, reply with the message unchanged.";

impl Orchestrator {
    /// One tool-less turn on a small model; its reply, or None when no small
    /// model answers. Tries the model chosen in Settings, then its fallback,
    /// then the session's agent's own small model (or any ready agent's).
    pub async fn quick(&self, prefer: &str, instructions: &str, prompt: &str) -> Option<String> {
        let cfg = self.config();
        let all = self.provider_statuses();
        for (agent, model) in candidates(&cfg.quick, &all, prefer) {
            if !cfg.provider(&agent).enabled {
                continue;
            }
            if let Some(said) = self.quick_on(&agent, &model, instructions, prompt).await {
                return Some(said);
            }
        }
        None
    }

    async fn quick_on(
        &self,
        agent: &str,
        model: &str,
        instructions: &str,
        prompt: &str,
    ) -> Option<String> {
        let cfg = self.config();
        let provider = (self.source)(agent, &cfg)?;
        let model = model.to_string();
        let dir = std::env::temp_dir().join("parzi-quick");
        std::fs::create_dir_all(&dir).ok()?;
        let spec = TurnSpec {
            cwd: dir,
            model: Some(model),
            effort: None,
            instructions: Some(instructions.to_string()),
            resume: None,
            prompt: prompt.to_string(),
            images: vec![],
            tools: None,
            compact: false,
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let turn = provider.run_turn(spec, Arc::new(NoTools), tx, cancel.clone());
        let collect = async {
            let mut said = String::new();
            let mut last = String::new();
            while let Some(e) = rx.recv().await {
                match e {
                    ProviderEvent::TextDelta(t) => said.push_str(&t),
                    ProviderEvent::Message(m) => last = m,
                    _ => {}
                }
            }
            if last.trim().is_empty() {
                said
            } else {
                last
            }
        };
        let done = tokio::time::timeout(LIMIT, async { tokio::join!(turn, collect) }).await;
        cancel.cancel();
        match done {
            Ok((Ok(_), text)) => Some(text.trim().to_string()).filter(|t| !t.is_empty()),
            Ok((Err(e), _)) => {
                tracing::info!(agent = %agent, "quick turn failed: {e}");
                None
            }
            Err(_) => {
                tracing::info!(agent = %agent, "quick turn took too long");
                None
            }
        }
    }

    /// Names a new session with a small model, unless it was renamed first.
    pub fn name_soon(self: &Arc<Self>, id: &str, prompt: &str) {
        let (me, id, prompt) = (self.clone(), id.to_string(), prompt.to_string());
        tokio::spawn(async move {
            let Ok(meta) = me.store.get(&id) else { return };
            let before = meta.title.clone();
            let agent = meta.model.split('/').next().unwrap_or_default().to_string();
            let asked: String = prompt.chars().take(2_000).collect();
            let Some(said) = me.quick(&agent, NAMER, &asked).await else {
                return;
            };
            let name = fit_name(said.trim().trim_matches(['"', '\'', '.']));
            if name.is_empty() || name.split(' ').count() > NAME_WORDS {
                return;
            }
            // The person or the agent may have named it meanwhile.
            if me.store.get(&id).map(|m| m.title).ok() != Some(before) {
                return;
            }
            if me.store.set_title(&id, &name).is_ok() {
                let _ = me.bus.send((id, RunEvent::Refreshed));
            }
        });
    }

    /// Tidies a message the person sent, for display only: the agent keeps
    /// what they typed. Stored as a `parzi-tidy` widget naming which of the
    /// session's messages it is.
    pub fn tidy_soon(self: &Arc<Self>, id: &str, text: &str) {
        if !worth_tidying(text) {
            return;
        }
        let (me, id, text) = (self.clone(), id.to_string(), text.to_string());
        tokio::spawn(async move {
            let Ok(meta) = me.store.get(&id) else { return };
            let agent = meta.model.split('/').next().unwrap_or_default().to_string();
            let Some(tidy) = me.quick(&agent, TIDIER, &text).await else {
                return;
            };
            if tidy.trim() == text.trim() || tidy.len() > text.len() * 3 + 200 {
                return;
            }
            let events = me.store.events(&id).unwrap_or_default();
            let Some(n) = events
                .iter()
                .filter(|e| matches!(e, Event::User { .. }))
                .position(|e| matches!(e, Event::User { text: t } if t.trim_start().starts_with(text.trim())))
            else {
                return;
            };
            let payload = serde_json::json!({ "n": n, "text": tidy });
            let event = Event::Widget {
                fence: "parzi-tidy".into(),
                payload,
            };
            if me.store.append(&id, &event).is_ok() {
                let _ = me.bus.send((id, RunEvent::Refreshed));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parzi_providers::ModelInfo;

    fn status(provider: &str, state: State, models: &[&str]) -> ProviderStatus {
        ProviderStatus {
            provider: provider.into(),
            state,
            version: None,
            account: None,
            hint: String::new(),
            usage: vec![],
            models: models
                .iter()
                .map(|m| ModelInfo {
                    id: (*m).into(),
                    name: (*m).into(),
                    is_default: false,
                    efforts: vec![],
                    images: None,
                })
                .collect(),
            gated: false,
            checked_at: 0,
        }
    }

    #[test]
    fn the_sessions_own_agent_lends_its_small_model_first() {
        let all = [
            status(
                "claude",
                State::Ready,
                &["claude-opus-5-5", "claude-haiku-5-5"],
            ),
            status("codex", State::Ready, &["gpt-5", "gpt-5-mini"]),
        ];
        assert_eq!(
            small_model(&all, "codex"),
            Some(("codex".into(), "gpt-5-mini".into()))
        );
        assert_eq!(
            small_model(&all, "claude"),
            Some(("claude".into(), "claude-haiku-5-5".into()))
        );
    }

    #[test]
    fn without_a_small_model_another_ready_agent_lends_one() {
        let all = [
            status("grok", State::Ready, &["grok-4.7"]),
            status("codex", State::SignedOut, &["gpt-5-mini"]),
            status("claude", State::Ready, &["claude-haiku-5-5"]),
        ];
        assert_eq!(
            small_model(&all, "grok"),
            Some(("claude".into(), "claude-haiku-5-5".into()))
        );
        assert_eq!(
            small_model(&all[..2], "grok"),
            None,
            "a signed-out agent lends nothing"
        );
    }

    #[test]
    fn the_chosen_model_then_its_fallback_then_the_automatic_pick() {
        let all = [
            status(
                "claude",
                State::Ready,
                &["claude-opus-5-5", "claude-haiku-5-5"],
            ),
            status("codex", State::SignedOut, &["gpt-5-mini"]),
            status("grok", State::Ready, &["grok-4.7-fast"]),
        ];
        let chosen = parzi_core::config::QuickModels {
            model: "grok/grok-4.7-fast".into(),
            fallback: "codex/gpt-5-mini".into(),
        };
        assert_eq!(
            candidates(&chosen, &all, "claude"),
            [
                ("grok".to_string(), "grok-4.7-fast".to_string()),
                ("claude".to_string(), "claude-haiku-5-5".to_string()),
            ],
            "a signed-out fallback is skipped; the automatic pick comes last"
        );
        let none = parzi_core::config::QuickModels::default();
        assert_eq!(
            candidates(&none, &all, "claude"),
            [("claude".to_string(), "claude-haiku-5-5".to_string())]
        );
    }

    #[test]
    fn math_is_worth_tidying_and_chat_is_not() {
        assert!(worth_tidying(
            "Derive pDe/Dt, e = u + 1/2v^2, then integrate sigma ij dA"
        ));
        assert!(worth_tidying("why is sqrt(x^2) = |x| and not x"));
        assert!(!worth_tidying("can you fix the login page please"));
        assert!(!worth_tidying("ok"));
        assert!(!worth_tidying(
            "already set: $e = u + v^2/2$ and $$\\int f$$"
        ));
    }
}
