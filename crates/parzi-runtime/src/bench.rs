//! The agents a session can hand work to and the real models they offer.
//! `models.list` shows this and `session.spawn` checks against it, so a
//! subsession only starts on something that can run.

use parzi_core::config::ParziConfig;
use parzi_providers::{canonical_id, display_name, split_spec, ModelInfo, ProviderStatus, State};

const SEE_LIST: &str = "models.list shows what can run";

struct Seat {
    id: &'static str,
    models: Vec<ModelInfo>,
    /// Why it can't run. A failed or pending status check is not a reason:
    /// the agent may well work, so only a definite no closes a seat.
    closed: Option<&'static str>,
}

impl Seat {
    fn find(&self, wanted: &str) -> Option<&str> {
        self.models
            .iter()
            .find(|m| m.id.eq_ignore_ascii_case(wanted))
            .map(|m| m.id.as_str())
    }

    fn default_model(&self, cfg: &ParziConfig) -> Option<&str> {
        self.find(cfg.provider(self.id).default_model.trim())
            .or_else(|| {
                self.models
                    .iter()
                    .find(|m| m.is_default)
                    .map(|m| m.id.as_str())
            })
    }
}

fn seats(cfg: &ParziConfig, statuses: &[ProviderStatus]) -> Vec<Seat> {
    let mut ids: Vec<&'static str> = vec![];
    let named = cfg.routing.order.iter().map(String::as_str);
    for id in named.chain(parzi_providers::PROVIDERS.iter().copied()) {
        if let Some(id) = canonical_id(id).filter(|id| !ids.contains(id)) {
            ids.push(id);
        }
    }
    ids.into_iter()
        .map(|id| {
            let status = statuses.iter().find(|s| s.provider == id);
            let closed = match status.map(|s| s.state) {
                _ if !cfg.provider(id).enabled => Some("switched off"),
                Some(State::Disabled) => Some("switched off"),
                Some(State::NotInstalled) => Some("not installed"),
                Some(State::SignedOut) => Some("signed out"),
                _ => None,
            };
            let models = status
                .map(|s| s.models.clone())
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| parzi_providers::cached_models(id));
            Seat { id, models, closed }
        })
        .collect()
}

/// What `models.list` returns: every id here works as `model` in `session.spawn`.
pub(crate) fn text(cfg: &ParziConfig, statuses: &[ProviderStatus], own: &str) -> String {
    let seats = seats(cfg, statuses);
    let mut lines = vec![if own.trim().is_empty() {
        "Pass one of these ids as `model` to session.spawn.".to_string()
    } else {
        format!(
            "Pass one of these ids as `model` to session.spawn. Leave `model` out to use your own: {own}."
        )
    }];
    for seat in seats.iter().filter(|s| s.closed.is_none()) {
        lines.push(format!("{}:", display_name(seat.id)));
        if seat.models.is_empty() {
            lines.push(format!("- {} (it picks its own model)", seat.id));
        }
        let default = seat.default_model(cfg);
        for model in &seat.models {
            let mark = if default == Some(model.id.as_str()) {
                " (default)"
            } else {
                ""
            };
            lines.push(format!("- {}/{}{mark}", seat.id, model.id));
        }
    }
    let closed: Vec<String> = seats
        .iter()
        // The custom agent slot is empty until someone adds one.
        .filter(|s| !(s.id == "acp" && s.closed == Some("not installed")))
        .filter_map(|s| {
            s.closed
                .map(|why| format!("{} ({why})", display_name(s.id)))
        })
        .collect();
    if !closed.is_empty() {
        lines.push(format!("Not available: {}.", closed.join(", ")));
    }
    lines.join("\n")
}

/// Turns what an agent asked for into a spec that can run, or says why not.
/// `own` is the asking session's model, which a bare name for its own provider means.
pub(crate) fn resolve(
    spec: &str,
    cfg: &ParziConfig,
    statuses: &[ProviderStatus],
    own: &str,
) -> Result<String, String> {
    pick(spec.trim(), cfg, &seats(cfg, statuses), own)
}

fn pick(spec: &str, cfg: &ParziConfig, seats: &[Seat], own: &str) -> Result<String, String> {
    let Some((id, model)) = split_spec(spec) else {
        return anywhere(spec, seats);
    };
    let name = display_name(id);
    let Some(seat) = seats.iter().find(|s| s.id == id) else {
        return Err(format!("{name} can't run here; {SEE_LIST}"));
    };
    if let Some(why) = seat.closed {
        return Err(format!("{name} is {why}; {SEE_LIST}"));
    }
    if seat.models.is_empty() {
        // It has never said what it offers, so there is nothing to check against.
        return Ok(model.map_or_else(|| id.to_string(), |m| format!("{id}/{m}")));
    }
    let Some(model) = model else {
        if own.split('/').next().and_then(canonical_id) == Some(id) && own.contains('/') {
            return Ok(own.to_string());
        }
        return seat.default_model(cfg).map_or_else(
            || {
                Err(format!(
                    "`{spec}` is an agent, not a model, and {name} has no default; {SEE_LIST}"
                ))
            },
            |m| Ok(format!("{id}/{m}")),
        );
    };
    // OpenCode's own ids start with a provider too: `opencode/muse` can be one whole.
    seat.find(&model)
        .or_else(|| seat.find(spec))
        .map(|m| format!("{id}/{m}"))
        .ok_or_else(|| format!("{name} has no model `{model}`; {SEE_LIST}"))
}

/// A model named without its agent, found by asking who offers it.
fn anywhere(wanted: &str, seats: &[Seat]) -> Result<String, String> {
    let found: Vec<String> = seats
        .iter()
        .filter(|s| s.closed.is_none())
        .filter_map(|s| s.find(wanted).map(|m| format!("{}/{m}", s.id)))
        .collect();
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(format!("no working agent offers `{wanted}`; {SEE_LIST}")),
        many => Err(format!(
            "`{wanted}` is on more than one agent, pick one of: {}",
            many.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, is_default: bool) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            name: id.into(),
            is_default,
            efforts: vec![],
            images: None,
        }
    }

    fn status(id: &str, state: State, models: Vec<ModelInfo>) -> ProviderStatus {
        let mut s = ProviderStatus::new(id, state, "");
        s.models = models;
        s
    }

    fn board() -> Vec<ProviderStatus> {
        vec![
            status(
                "claude",
                State::Ready,
                vec![model("opus", false), model("sonnet", true)],
            ),
            status(
                "opencode",
                State::Error,
                vec![
                    model("opencode/muse-1", false),
                    model("opencode-go/muse-1", false),
                    model("anthropic/opus", false),
                ],
            ),
            status("grok", State::Ready, vec![model("sonnet", true)]),
            status("codex", State::SignedOut, vec![model("gpt-5", true)]),
            status("cursor", State::NotInstalled, vec![]),
            status("antigravity", State::Disabled, vec![]),
            status("acp", State::NotInstalled, vec![]),
        ]
    }

    fn ask(spec: &str, own: &str) -> Result<String, String> {
        resolve(spec, &ParziConfig::default(), &board(), own)
    }

    #[test]
    fn a_listed_model_is_used_as_it_is_listed() {
        assert_eq!(ask("claude/opus", "").unwrap(), "claude/opus");
        assert_eq!(ask("claude/OPUS", "").unwrap(), "claude/opus");
        assert_eq!(
            ask("opencode/anthropic/opus", "").unwrap(),
            "opencode/anthropic/opus"
        );
    }

    #[test]
    fn an_opencode_id_works_with_or_without_its_agent() {
        assert_eq!(
            ask("opencode/muse-1", "").unwrap(),
            "opencode/opencode/muse-1"
        );
        assert_eq!(
            ask("opencode-go/muse-1", "").unwrap(),
            "opencode/opencode-go/muse-1"
        );
        assert_eq!(
            ask("opencode/opencode/muse-1", "").unwrap(),
            "opencode/opencode/muse-1"
        );
    }

    #[test]
    fn a_bare_agent_becomes_a_real_model_or_is_refused() {
        assert_eq!(ask("claude", "").unwrap(), "claude/sonnet");
        assert_eq!(
            ask("opencode", "opencode/anthropic/opus").unwrap(),
            "opencode/anthropic/opus",
            "the asker's own model, when it is on that agent"
        );
        let refused = ask("opencode", "claude/opus").unwrap_err();
        assert!(refused.contains("not a model"), "{refused}");
    }

    #[test]
    fn an_agent_that_cannot_run_is_refused_with_the_reason() {
        assert!(ask("codex", "").unwrap_err().contains("signed out"));
        assert!(ask("codex/gpt-5", "").unwrap_err().contains("signed out"));
        assert!(ask("cursor", "").unwrap_err().contains("not installed"));
        assert!(ask("antigravity", "").unwrap_err().contains("switched off"));
    }

    #[test]
    fn a_setting_that_switches_an_agent_off_beats_its_last_status() {
        let mut cfg = ParziConfig::default();
        cfg.providers.entry("claude".into()).or_default().enabled = false;
        let refused = resolve("claude/opus", &cfg, &board(), "").unwrap_err();
        assert!(refused.contains("switched off"), "{refused}");
    }

    #[test]
    fn a_model_nobody_offers_is_refused() {
        assert!(ask("claude/gpt-5", "")
            .unwrap_err()
            .contains("no model `gpt-5`"));
        assert!(ask("made-up", "").unwrap_err().contains("no working agent"));
    }

    #[test]
    fn a_model_named_alone_goes_to_whoever_offers_it() {
        assert_eq!(ask("opus", "").unwrap(), "claude/opus");
        let many = ask("sonnet", "").unwrap_err();
        assert!(
            many.contains("claude/sonnet") && many.contains("grok/sonnet"),
            "{many}"
        );
        assert!(
            ask("gpt-5", "").is_err(),
            "a signed out agent's models don't count"
        );
    }

    #[test]
    fn an_agent_that_never_listed_models_is_taken_at_its_word() {
        let seats = [Seat {
            id: "grok",
            models: vec![],
            closed: None,
        }];
        let cfg = ParziConfig::default();
        assert_eq!(
            pick("grok/grok-9", &cfg, &seats, "").unwrap(),
            "grok/grok-9"
        );
        assert_eq!(pick("grok", &cfg, &seats, "").unwrap(), "grok");
    }

    #[test]
    fn the_list_names_real_models_and_what_is_missing() {
        let out = text(&ParziConfig::default(), &board(), "claude/opus");
        assert!(out.contains("use your own: claude/opus"), "{out}");
        assert!(out.contains("- claude/sonnet (default)"), "{out}");
        assert!(out.contains("- opencode/opencode-go/muse-1"), "{out}");
        assert!(!out.contains("gpt-5"), "signed out models stay off: {out}");
        assert!(
            out.contains(
                "Not available: Codex (signed out), Antigravity (switched off), Cursor (not installed)."
            ),
            "{out}"
        );
        assert!(!out.contains("ACP"), "an empty custom slot is noise: {out}");
    }
}
