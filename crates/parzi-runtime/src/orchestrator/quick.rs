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

/// The first small model offered by a ready agent, as (agent, model id).
fn first_small(s: &ProviderStatus) -> Option<(String, String)> {
    s.models
        .iter()
        .find(|m| {
            let both = format!("{} {}", m.id, m.name).to_ascii_lowercase();
            SMALL.iter().any(|w| both.contains(w))
        })
        .map(|m| (s.provider.clone(), m.id.clone()))
}

/// Ready agents' small models ordered by preference: fast-starting agents
/// first (session's agent before others), then slow-starting agents.
pub(crate) fn small_models(
    all: &[ProviderStatus],
    prefer: &str,
    is_slow: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    let ready: Vec<&ProviderStatus> = all.iter().filter(|s| s.state == State::Ready).collect();
    let mut out = Vec::new();
    for slow_tier in [false, true] {
        for prefer_tier in [true, false] {
            for s in &ready {
                if is_slow(&s.provider) != slow_tier || (s.provider == prefer) != prefer_tier {
                    continue;
                }
                if let Some(pair) = first_small(s) {
                    if !out.contains(&pair) {
                        out.push(pair);
                    }
                }
            }
        }
    }
    out
}

/// The small model to use: the preferred agent's first, then any ready
/// agent's, as (agent, model id).
#[cfg(test)]
pub(crate) fn small_model(all: &[ProviderStatus], prefer: &str) -> Option<(String, String)> {
    small_models(all, prefer, |_| false).into_iter().next()
}

/// The small models to try, in order: the one chosen in Settings, its
/// fallback (each only when its agent is ready), then the automatic picks
/// (fast-starting agents before slow-starting ones).
pub(crate) fn candidates_with(
    chosen: &parzi_core::config::QuickModels,
    all: &[ProviderStatus],
    prefer: &str,
    is_slow: impl Fn(&str) -> bool,
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
    for auto in small_models(all, prefer, is_slow) {
        if !out.contains(&auto) {
            out.push(auto);
        }
    }
    out
}

#[cfg(test)]
pub(crate) fn candidates(
    chosen: &parzi_core::config::QuickModels,
    all: &[ProviderStatus],
    prefer: &str,
) -> Vec<(String, String)> {
    candidates_with(chosen, all, prefer, |_| false)
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

/// Non-math prose words that should never be dropped when tidying notation.
const PROSE_KEEP: &[&str] = &[
    "if",
    "when",
    "while",
    "unless",
    "until",
    "because",
    "since",
    "although",
    "though",
    "whereas",
    "suppose",
    "assume",
    "assuming",
    "given",
    "consider",
    "prove",
    "derive",
    "show",
    "explain",
    "check",
    "verify",
    "find",
    "solve",
    "compute",
    "calculate",
    "evaluate",
    "simplify",
    "test",
    "testing",
    "why",
    "how",
    "what",
    "which",
    "whether",
    "can",
    "could",
    "would",
    "should",
    "does",
    "not",
    "never",
    "always",
    "please",
    "therefore",
    "hence",
    "thus",
    "however",
    "but",
    "then",
];

fn ascii_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

pub(crate) fn tidy_ok(orig: &str, tidy: &str) -> bool {
    let orig = orig.trim();
    let tidy = tidy.trim();
    if tidy.is_empty() || tidy == orig || tidy.len() > orig.len() * 3 + 200 {
        return false;
    }
    let kept = ascii_words(tidy);
    for w in ascii_words(orig) {
        if PROSE_KEEP.contains(&w.as_str()) && !kept.iter().any(|k| k == &w) {
            return false;
        }
    }
    true
}

const NAMER: &str = "You name chat sessions. Reply with only a name for the conversation that starts with the message below: 2 to 5 words, sentence case, no quotes, no full stop, at most 60 characters. Name what it is about, not that it is a question.";

const TIDIER: &str = "You format a person's chat message for display without changing any prose. Keep every non-math word, prefix, connecting word (such as 'if', 'the', 'where', 'given', 'when'), punctuation and sentence fragment verbatim, in the exact same order, even if the message is an incomplete sentence or cut off. Fix nothing about the content or grammar. Only convert mathematical notation to LaTeX: keep math inside a sentence inline in $...$ (including integrals, sums and fractions, which already render full size), and use $$...$$ on its own line only when an equation is already on its own line. Use \\frac, never \\tfrac or a slash for a fraction. Put names of code, files and commands in backticks, and keep Markdown lists only where they already were lists. Reply with only the rewritten message, or the message unchanged if nothing needs formatting.";

impl Orchestrator {
    /// One tool-less turn on a small model; its reply, or None when no small
    /// model answers. Tries the model chosen in Settings, then its fallback,
    /// then the session's agent's own small model (or any ready fast-starting
    /// agent's).
    pub async fn quick(&self, prefer: &str, instructions: &str, prompt: &str) -> Option<String> {
        let cfg = self.config();
        let all = self.provider_statuses();
        let is_slow = |agent: &str| {
            !cfg.provider(agent).enabled
                || (self.source)(agent, &cfg).is_some_and(|p| p.slow_start())
        };
        for (agent, model) in candidates_with(&cfg.quick, &all, prefer, is_slow) {
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

    async fn run_name(&self, id: &str, prompt: &str) {
        let Ok(meta) = self.store.get(id) else { return };
        let before = meta.title.clone();
        let agent = meta.model.split('/').next().unwrap_or_default().to_string();
        let asked: String = prompt.chars().take(2_000).collect();
        let Some(said) = self.quick(&agent, NAMER, &asked).await else {
            return;
        };
        let name = fit_name(said.trim().trim_matches(['"', '\'', '.']));
        if name.is_empty() || name.split(' ').count() > NAME_WORDS {
            return;
        }
        // The person or the agent may have named it meanwhile.
        if self.store.get(id).map(|m| m.title).ok() != Some(before) {
            return;
        }
        if self.store.set_title(id, &name).is_ok() {
            let _ = self.bus.send((id.to_string(), RunEvent::Refreshed));
        }
    }

    async fn run_tidy(&self, id: &str, text: &str) {
        if !worth_tidying(text) {
            return;
        }
        let Ok(meta) = self.store.get(id) else { return };
        let agent = meta.model.split('/').next().unwrap_or_default().to_string();
        let Some(tidy) = self.quick(&agent, TIDIER, text).await else {
            return;
        };
        if !tidy_ok(text, &tidy) {
            return;
        }
        let events = self.store.events(id).unwrap_or_default();
        let Some(n) = events
            .iter()
            .filter(|e| matches!(e, Event::User { .. }))
            .position(
                |e| matches!(e, Event::User { text: t } if t.trim_start().starts_with(text.trim())),
            )
        else {
            return;
        };
        let payload = serde_json::json!({ "n": n, "text": tidy });
        let event = Event::Widget {
            fence: "parzi-tidy".into(),
            payload,
        };
        if self.store.append(id, &event).is_ok() {
            let _ = self.bus.send((id.to_string(), RunEvent::Refreshed));
        }
    }

    /// Names a new session (when `name` is true) and then tidies the message
    /// for display, sequentially in one background task so naming finishes
    /// before formatting runs.
    pub fn prepare_soon(self: &Arc<Self>, id: &str, prompt: &str, name: bool) {
        let do_tidy = worth_tidying(prompt);
        if !name && !do_tidy {
            return;
        }
        let (me, id, prompt) = (self.clone(), id.to_string(), prompt.to_string());
        tokio::spawn(async move {
            if name {
                me.run_name(&id, &prompt).await;
            }
            if do_tidy {
                me.run_tidy(&id, &prompt).await;
            }
        });
    }

    /// Names a new session with a small model, unless it was renamed first.
    pub fn name_soon(self: &Arc<Self>, id: &str, prompt: &str) {
        let (me, id, prompt) = (self.clone(), id.to_string(), prompt.to_string());
        tokio::spawn(async move {
            me.run_name(&id, &prompt).await;
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
            me.run_tidy(&id, &text).await;
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
            [
                ("claude".to_string(), "claude-haiku-5-5".to_string()),
                ("grok".to_string(), "grok-4.7-fast".to_string()),
            ]
        );
    }

    #[test]
    fn fast_starting_agents_come_before_slow_starting_ones() {
        let all = [
            status("antigravity", State::Ready, &["gemini-flash"]),
            status("opencode", State::Ready, &["opencode-mini"]),
        ];
        assert_eq!(
            small_models(&all, "antigravity", |a| a == "antigravity"),
            [
                ("opencode".to_string(), "opencode-mini".to_string()),
                ("antigravity".to_string(), "gemini-flash".to_string()),
            ]
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

    #[test]
    fn tidier_rejects_dropped_prose_words() {
        let orig = "testing... if the surface area integral of njsigmaij dA + volume integral density fv dV = integral density De/Dt dV";
        let dropped =
            "testing... $$\\int n_j \\sigma_{ij}\\,dA + \\int \\rho f_v\\,dV = \\int \\rho \\frac{De}{Dt}\\,dV$$";
        let kept =
            "testing... if the surface area integral $\\int n_j \\sigma_{ij}\\,dA + \\int \\rho f_v\\,dV = \\int \\rho \\frac{De}{Dt}\\,dV$";
        assert!(!tidy_ok(orig, dropped));
        assert!(tidy_ok(orig, kept));
    }
}
