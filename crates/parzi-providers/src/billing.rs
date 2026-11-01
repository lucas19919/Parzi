use std::path::{Path, PathBuf};

use parzi_core::config::Billing;
use serde_json::Value;

use crate::types::{ProviderStatus, State};

const OPENCODE_PLAN: &str = "opencode-go";
/// Agents that take one key. OpenCode keeps a key per provider in its own login.
const TAKES_KEY: &[&str] = &["claude", "codex", "grok", "antigravity", "cursor"];
const MAX_KEY: usize = 4096;

/// What an agent's process gets so that it bills what the user chose.
#[derive(Clone, Default)]
pub(crate) struct AgentEnv {
    pub set: Vec<(String, String)>,
    pub remove: &'static [&'static str],
}

/// Variables that make an agent bill a key or a gateway instead of its login.
fn key_vars(id: &str) -> &'static [&'static str] {
    match id {
        "claude" => &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
        ],
        "codex" => &["OPENAI_API_KEY", "CODEX_API_KEY", "OPENAI_BASE_URL"],
        "grok" => &["XAI_API_KEY", "GROK_CODE_XAI_API_KEY"],
        "antigravity" => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        "cursor" => &["CURSOR_API_KEY"],
        _ => &[],
    }
}

pub(crate) fn agent_env(id: &str, billing: Billing) -> AgentEnv {
    if billing == Billing::ApiKey {
        return match (saved_key(id), keys_dir()) {
            (Some(key), Some(dir)) => AgentEnv {
                set: key_env(id, &key, &dir.join(id)),
                remove: &[],
            },
            _ => AgentEnv::default(),
        };
    }
    // OpenCode reads a key for every provider it knows, so it gets a list of
    // what may bill instead: removing names would never catch them all, and
    // would take keys from the commands it runs for the user.
    let set = if id == "opencode" {
        let config = serde_json::json!({"enabled_providers": opencode_plans()});
        vec![("OPENCODE_CONFIG_CONTENT".to_string(), config.to_string())]
    } else {
        vec![]
    };
    AgentEnv {
        set,
        remove: key_vars(id),
    }
}

/// Codex and Antigravity read their login from a folder, so a key gets a folder
/// of its own and the subscription login beside it stays as it was.
fn key_env(id: &str, key: &str, home: &Path) -> Vec<(String, String)> {
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    let home = home.display().to_string();
    match id {
        "claude" => vec![pair("ANTHROPIC_API_KEY", key)],
        "codex" => vec![pair("CODEX_HOME", &home)],
        "grok" => vec![pair("XAI_API_KEY", key)],
        "antigravity" => vec![pair("GEMINI_HOME", &home), pair("GEMINI_API_KEY", key)],
        "cursor" => vec![pair("CURSOR_API_KEY", key)],
        _ => vec![],
    }
}

fn keys_dir() -> Option<PathBuf> {
    Some(parzi_core::paths::parzi_dir().ok()?.join("keys"))
}

fn key_in(dir: &Path, id: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(format!("{id}.key"))).ok()?;
    Some(text.trim().to_string()).filter(|k| !k.is_empty())
}

pub(crate) fn saved_key(id: &str) -> Option<String> {
    key_in(&keys_dir()?, id)
}

/// The agents with a saved key. The keys themselves never leave this module
/// except into that agent's own process.
pub fn saved_keys() -> Vec<String> {
    TAKES_KEY
        .iter()
        .filter(|id| saved_key(id).is_some())
        .map(|id| (*id).to_string())
        .collect()
}

pub fn save_key(id: &str, key: Option<&str>) -> Result<(), String> {
    let dir = keys_dir().ok_or("cannot locate the Parzi folder")?;
    write_key(&dir, id, key)
}

fn write_key(dir: &Path, id: &str, key: Option<&str>) -> Result<(), String> {
    if !TAKES_KEY.contains(&id) {
        return Err(format!(
            "{} does not take a key here",
            crate::display_name(id)
        ));
    }
    let path = dir.join(format!("{id}.key"));
    let home = dir.join(id);
    let Some(key) = key else {
        for gone in [std::fs::remove_file(&path), std::fs::remove_dir_all(&home)] {
            match gone {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(format!("could not remove the key: {e}"));
                }
                _ => {}
            }
        }
        return Ok(());
    };
    let key = key.trim();
    if key.is_empty()
        || key.len() > MAX_KEY
        || key.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("That does not look like an API key.".into());
    }
    let write = |file: &Path, text: &str| {
        parzi_core::atomic_write_private(file, text.as_bytes()).map_err(|e| e.to_string())
    };
    match id {
        "codex" => {
            let auth = serde_json::json!({"auth_mode": "apikey", "OPENAI_API_KEY": key});
            write(&home.join("auth.json"), &auth.to_string())?;
        }
        "antigravity" => {
            let settings = serde_json::json!({"auth": {"type": "gemini-api-key"}});
            let file = home.join("antigravity-acp").join("settings.json");
            write(&file, &settings.to_string())?;
        }
        _ => {}
    }
    write(&path, key)
}

pub(crate) fn opencode_data() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("share")))?;
    Some(data.join("opencode"))
}

fn opencode_plans() -> Vec<String> {
    let text = opencode_data()
        .and_then(|d| std::fs::read_to_string(d.join("auth.json")).ok())
        .unwrap_or_default();
    plans_in(&text)
}

/// OpenCode Go is a flat plan and an OAuth login is someone's plan. Zen and
/// stored API keys are billed per token.
fn plans_in(auth: &str) -> Vec<String> {
    let parsed: Value = serde_json::from_str(auth).unwrap_or(Value::Null);
    let mut plans = vec![OPENCODE_PLAN.to_string()];
    if let Some(entries) = parsed.as_object() {
        plans.extend(
            entries
                .iter()
                .filter(|(_, e)| e.get("type").and_then(Value::as_str) == Some("oauth"))
                .map(|(id, _)| id.clone()),
        );
    }
    plans.sort();
    plans.dedup();
    plans
}

pub(crate) fn opencode_allows(model: &str) -> bool {
    model_allowed(model, &opencode_plans())
}

fn model_allowed(model: &str, plans: &[String]) -> bool {
    model
        .split_once('/')
        .is_some_and(|(provider, _)| plans.iter().any(|p| p == provider))
}

pub(crate) fn key_login(name: &str) -> String {
    format!(
        "{name}'s login bills per token. Sign in with your subscription, \
         or choose API key in Settings, Providers."
    )
}

pub(crate) fn is_key_label(account: &str) -> bool {
    matches!(account, "API key" | "Gemini API key" | "Agent Platform")
}

/// An agent on its subscription never bills a key by surprise: a login that
/// would is shown as not signed in, with the two ways forward.
pub(crate) fn enforce(mut s: ProviderStatus, billing: Billing) -> ProviderStatus {
    let keyed = s.account.as_deref().is_some_and(is_key_label);
    match billing {
        Billing::Subscription if s.state == State::Ready && keyed => {
            s.hint = key_login(crate::display_name(&s.provider));
            s.state = State::SignedOut;
            s.usage.clear();
        }
        Billing::ApiKey
            if s.state == State::SignedOut && TAKES_KEY.contains(&s.provider.as_str()) =>
        {
            s.hint = "Add an API key to use this agent.".into();
        }
        _ => {}
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_keys(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("parzi-keys-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_key_reaches_each_agent_the_way_it_reads_one() {
        let home = Path::new("/keys/x");
        let has = |id: &str, name: &str, value: &str| {
            let env = key_env(id, "k1", home);
            assert!(
                env.iter().any(|(k, v)| k == name && v == value),
                "{id}: {env:?}"
            );
        };
        has("claude", "ANTHROPIC_API_KEY", "k1");
        has("grok", "XAI_API_KEY", "k1");
        has("cursor", "CURSOR_API_KEY", "k1");
        has("antigravity", "GEMINI_API_KEY", "k1");
        has("antigravity", "GEMINI_HOME", &home.display().to_string());
        has("codex", "CODEX_HOME", &home.display().to_string());
        assert!(key_env("opencode", "k1", home).is_empty());
    }

    #[test]
    fn saving_a_key_keeps_it_apart_and_removing_it_leaves_nothing() {
        let dir = temp_keys("save");
        write_key(&dir, "codex", Some("  sk-abc\n")).unwrap();
        assert_eq!(key_in(&dir, "codex").as_deref(), Some("sk-abc"));
        let auth: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("codex/auth.json")).unwrap())
                .unwrap();
        assert_eq!(auth["auth_mode"], "apikey");
        assert_eq!(auth["OPENAI_API_KEY"], "sk-abc");

        write_key(&dir, "antigravity", Some("g-abc")).unwrap();
        let settings =
            std::fs::read_to_string(dir.join("antigravity/antigravity-acp/settings.json"));
        assert!(settings.unwrap().contains("gemini-api-key"));

        write_key(&dir, "codex", None).unwrap();
        assert_eq!(key_in(&dir, "codex"), None);
        assert!(!dir.join("codex").exists());
        write_key(&dir, "codex", None).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_something_shaped_like_a_key_is_saved() {
        let dir = temp_keys("shape");
        for bad in ["", "   ", "two words", "line\nbreak"] {
            assert!(write_key(&dir, "claude", Some(bad)).is_err(), "{bad:?}");
        }
        assert!(write_key(&dir, "opencode", Some("k")).is_err());
        assert!(write_key(&dir, "acp", Some("k")).is_err());
        assert_eq!(key_in(&dir, "claude"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signed_out_points_at_the_key_only_when_a_key_is_the_choice() {
        let out = |id: &str| ProviderStatus::new(id, State::SignedOut, "");
        assert!(enforce(out("claude"), Billing::ApiKey)
            .hint
            .contains("API key"));
        assert!(enforce(out("claude"), Billing::Subscription)
            .hint
            .is_empty());
        assert!(enforce(out("opencode"), Billing::ApiKey).hint.is_empty());
    }

    #[test]
    fn a_subscription_removes_the_keys_and_no_saved_key_adds_nothing() {
        let sub = agent_env("claude", Billing::Subscription);
        assert!(sub.remove.contains(&"ANTHROPIC_API_KEY"));
        assert!(sub.remove.contains(&"ANTHROPIC_AUTH_TOKEN"));
        assert!(agent_env("claude", Billing::ApiKey).remove.is_empty());
    }

    #[test]
    fn every_agent_with_a_key_of_its_own_has_it_removed() {
        for (id, var) in [
            ("codex", "CODEX_API_KEY"),
            ("grok", "XAI_API_KEY"),
            ("antigravity", "GEMINI_API_KEY"),
            ("cursor", "CURSOR_API_KEY"),
        ] {
            assert!(agent_env(id, Billing::Subscription).remove.contains(&var));
        }
    }

    #[test]
    fn opencode_keeps_plans_and_drops_zen_and_keys() {
        let auth = r#"{"opencode":{"type":"api"},"opencode-go":{"type":"api"},
            "anthropic":{"type":"api"},"github-copilot":{"type":"oauth"}}"#;
        let plans = plans_in(auth);
        assert_eq!(plans, ["github-copilot", "opencode-go"]);
        assert!(model_allowed("opencode-go/kimi", &plans));
        assert!(model_allowed("github-copilot/gpt", &plans));
        assert!(!model_allowed("opencode/claude-fable-5", &plans));
        assert!(!model_allowed("anthropic/claude", &plans));
        assert!(!model_allowed("no-provider", &plans));
        assert_eq!(plans_in("not json"), [OPENCODE_PLAN]);
    }

    #[test]
    fn opencode_is_only_limited_on_a_subscription() {
        let sub = agent_env("opencode", Billing::Subscription);
        assert_eq!(sub.set[0].0, "OPENCODE_CONFIG_CONTENT");
        assert!(sub.set[0].1.contains(OPENCODE_PLAN));
        assert!(agent_env("opencode", Billing::ApiKey).set.is_empty());
    }

    #[test]
    fn a_key_login_is_refused_only_on_a_subscription() {
        let keyed = || {
            let mut s = ProviderStatus::new("claude", State::Ready, "");
            s.account = Some("API key".into());
            s
        };
        let refused = enforce(keyed(), Billing::Subscription);
        assert_eq!(refused.state, State::SignedOut);
        assert!(refused.hint.contains("Claude"), "{}", refused.hint);
        assert_eq!(enforce(keyed(), Billing::ApiKey).state, State::Ready);

        let mut plan = ProviderStatus::new("claude", State::Ready, "");
        plan.account = Some("Claude Max".into());
        assert_eq!(enforce(plan, Billing::Subscription).state, State::Ready);
    }
}
