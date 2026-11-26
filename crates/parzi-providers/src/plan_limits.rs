//! Plan limits for the ACP agents, read the way each one's own `/usage`
//! reads them: Grok's credit window, OpenCode Go's 5-hour, weekly and monthly
//! spend, and Antigravity's per-group 5-hour and weekly quota.
//!
//! Each agent's own sign-in is read from disk and never written: an expired
//! one reads as no answer until the agent signs itself in again. None of the
//! endpoints is public API, so anything unexpected is no answer, not an error.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::billing;
use crate::types::UsageWindow;

/// The plan limits of the ACP agent `id`, or `None` when it has none to read.
/// Blocking: call it off the async runtime.
pub(crate) fn read(id: &str) -> Option<Vec<UsageWindow>> {
    let windows = match id {
        "grok" => grok_windows(&get(
            "https://cli-chat-proxy.grok.com/v1/billing?format=credits",
            &[
                ("Authorization", &format!("Bearer {}", grok_token()?)),
                ("x-xai-token-auth", "xai-grok-cli"),
            ],
        )?),
        "opencode" => opencode_go_windows(&get(
            "https://opencode.ai/zen/go/v1/usage",
            &[("Authorization", &format!("Bearer {}", opencode_go_key()?))],
        )?),
        "antigravity" => antigravity(),
        _ => vec![],
    };
    (!windows.is_empty()).then_some(windows)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(15)))
        .user_agent(concat!("parzi/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn reply_json(reply: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Option<Value> {
    let mut reply = reply
        .inspect_err(|e| tracing::debug!("plan limits were not read: {e}"))
        .ok()?;
    if !reply.status().is_success() {
        tracing::debug!("plan limits were not read: HTTP {}", reply.status());
        return None;
    }
    let text = reply
        .body_mut()
        .with_config()
        .limit(512 * 1024)
        .read_to_string()
        .ok()?;
    serde_json::from_str(&text).ok()
}

fn get(url: &str, headers: &[(&str, &str)]) -> Option<Value> {
    let mut req = agent().get(url).header("Accept", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    reply_json(req.call())
}

/// Google answers the quota only to Antigravity itself, so this call goes
/// out as the Antigravity client does.
fn antigravity_post(url: &str, token: &str, body: &Value) -> Option<Value> {
    reply_json(
        agent()
            .post(url)
            .header("User-Agent", "antigravity")
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Authorization", &format!("Bearer {token}"))
            .send(body.to_string()),
    )
}

fn read_json(path: PathBuf) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn epoch(t: &Value) -> Option<u64> {
    let t = chrono::DateTime::parse_from_rfc3339(t.as_str()?).ok()?;
    u64::try_from(t.timestamp()).ok()
}

fn window(label: &str, used_percent: f64, resets_at: Option<u64>) -> UsageWindow {
    UsageWindow {
        label: label.to_string(),
        used_percent: used_percent.clamp(0.0, 100.0),
        resets_at,
        ..Default::default()
    }
}

// Grok: the SuperGrok credit window that `/usage` inside grok shows.

/// Grok's own sign-in, as `grok login` saves it, while it is still valid.
fn grok_token() -> Option<String> {
    let home = std::env::var_os("GROK_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".grok")))?;
    let auth = read_json(home.join("auth.json"))?;
    let entries = auth.as_object()?;
    let entry = entries
        .iter()
        .find(|(k, _)| k.starts_with("https://auth.x.ai::"))
        .or_else(|| {
            entries
                .iter()
                .find(|(k, _)| *k == "https://accounts.x.ai/sign-in")
        })?
        .1;
    if entry
        .get("expires_at")
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t.as_str()?).ok())
        .is_some_and(|t| t <= chrono::Utc::now())
    {
        return None;
    }
    entry
        .get("key")
        .and_then(Value::as_str)
        .filter(|k| !k.is_empty())
        .map(str::to_string)
}

fn grok_windows(v: &Value) -> Vec<UsageWindow> {
    let Some(config) = v.get("config") else {
        return vec![];
    };
    let used = config
        .get("creditUsagePercent")
        .and_then(Value::as_f64)
        .or_else(|| {
            let cap = config.pointer("/onDemandCap/val").and_then(Value::as_f64)?;
            let spent = config
                .pointer("/onDemandUsed/val")
                .and_then(Value::as_f64)?;
            (cap > 0.0).then(|| spent / cap * 100.0)
        });
    let Some(used) = used else {
        return vec![];
    };
    let period = config.get("currentPeriod");
    let label = match period.and_then(|p| p.get("type")).and_then(Value::as_str) {
        Some("USAGE_PERIOD_TYPE_WEEKLY") => "Weekly",
        Some("USAGE_PERIOD_TYPE_MONTHLY") => "Monthly",
        Some("USAGE_PERIOD_TYPE_DAILY") => "Daily",
        _ => "Credits",
    };
    let resets_at = period
        .and_then(|p| p.get("end"))
        .and_then(epoch)
        .or_else(|| config.get("billingPeriodEnd").and_then(epoch));
    vec![window(label, used, resets_at)]
}

// OpenCode Go: the 5-hour, weekly and monthly spend the console shows.

/// The OpenCode Go key from OpenCode's own login. Zen alone has no limits.
fn opencode_go_key() -> Option<String> {
    read_json(billing::opencode_data()?.join("auth.json"))?
        .pointer("/opencode-go/key")
        .and_then(Value::as_str)
        .filter(|k| !k.is_empty())
        .map(str::to_string)
}

fn opencode_go_windows(v: &Value) -> Vec<UsageWindow> {
    let Some(usage) = v.get("usage") else {
        return vec![];
    };
    [
        ("rolling", "Session"),
        ("weekly", "Weekly"),
        ("monthly", "Monthly"),
    ]
    .iter()
    .filter_map(|(key, label)| {
        let w = usage.get(*key)?;
        let used = w.get("percent").and_then(Value::as_f64)?;
        Some(window(label, used, w.get("resetsAt").and_then(epoch)))
    })
    .collect()
}

// Antigravity: the Google AI plan's quota per model group.

fn antigravity() -> Vec<UsageWindow> {
    let Some((token, project)) = antigravity_token() else {
        return vec![];
    };
    let body = project.map_or_else(
        || serde_json::json!({}),
        |p| serde_json::json!({ "project": p }),
    );
    antigravity_post(
        "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
        &token,
        &body,
    )
    .map(|v| antigravity_windows(&v))
    .unwrap_or_default()
}

/// A short-lived access token from the Antigravity ACP server's own sign-in.
/// It stays in memory; the saved sign-in is not touched.
fn antigravity_token() -> Option<(String, Option<String>)> {
    let saved = read_json(
        dirs::home_dir()?
            .join(".gemini")
            .join("antigravity-acp")
            .join("acp_token.json"),
    )?;
    let field = |k: &str| {
        saved
            .get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    let token_uri = field("token_uri").unwrap_or("https://oauth2.googleapis.com/token");
    if !token_uri.starts_with("https://") {
        return None;
    }
    let form = [
        ("client_id", field("client_id")?),
        ("client_secret", field("client_secret")?),
        ("refresh_token", field("refresh_token")?),
        ("grant_type", "refresh_token"),
    ];
    let reply = reply_json(agent().post(token_uri).send_form(form))?;
    let token = reply.get("access_token")?.as_str()?.to_string();
    Some((token, field("project_id").map(str::to_string)))
}

fn antigravity_windows(v: &Value) -> Vec<UsageWindow> {
    let groups = v
        .get("groups")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let mut out = vec![];
    for group in groups {
        let name = group_name(
            group
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        for bucket in group
            .get("buckets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(left) = bucket.get("remainingFraction").and_then(Value::as_f64) else {
                continue;
            };
            let span = match bucket.get("window").and_then(Value::as_str) {
                Some("5h") => "Session",
                Some("weekly") => "Weekly",
                Some("daily") => "Daily",
                Some("monthly") => "Monthly",
                _ => continue,
            };
            // Each group's windows limit only its own models.
            let mut w = window(
                span,
                (1.0 - left) * 100.0,
                bucket.get("resetTime").and_then(epoch),
            );
            w.models = group_models(&name);
            out.push(w);
        }
    }
    // Session before Weekly within each group, as the other agents show them.
    out.sort_by_key(|w| w.label != "Session");
    out
}

/// The words a group's model ids contain: "Claude/GPT" limits ids with
/// "claude" or "gpt" in them.
fn group_models(name: &str) -> Vec<String> {
    name.split('/')
        .map(|w| w.trim().to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// "Claude and GPT models" reads as "Claude/GPT".
fn group_name(display: &str) -> String {
    display
        .trim()
        .trim_end_matches(" models")
        .trim_end_matches(" Models")
        .replace(" and ", "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn grok_shows_its_credit_window() {
        let v = json!({"config": {
            "currentPeriod": {"type": "USAGE_PERIOD_TYPE_WEEKLY", "start": "2026-11-21T14:57:11.189323+00:00", "end": "2026-11-28T14:57:11.189323+00:00"},
            "creditUsagePercent": 18.0,
            "onDemandCap": {"val": 0}, "onDemandUsed": {"val": 0},
            "productUsage": [{"product": "GrokBuild", "usagePercent": 15.0}],
            "billingPeriodEnd": "2026-11-28T14:57:11.189323+00:00"
        }});
        let w = grok_windows(&v);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].label, "Weekly");
        assert!((w[0].used_percent - 18.0).abs() < f64::EPSILON);
        assert_eq!(w[0].resets_at, Some(1_795_877_831));
        // No percent and no cap: unknown, not 0%.
        assert!(grok_windows(
            &json!({"config": {"onDemandCap": {"val": 0}, "onDemandUsed": {"val": 0}}})
        )
        .is_empty());
        let monthly = grok_windows(
            &json!({"config": {"currentPeriod": {"type": "USAGE_PERIOD_TYPE_MONTHLY"}, "creditUsagePercent": 140}}),
        );
        assert_eq!(
            (monthly[0].label.as_str(), monthly[0].used_percent),
            ("Monthly", 100.0)
        );
    }

    #[test]
    fn opencode_go_shows_session_weekly_and_monthly() {
        let v = json!({"usage": {
            "rolling": {"status": "ok", "percent": 0, "resetsAt": "2026-11-27T00:31:59.129Z"},
            "weekly": {"status": "ok", "percent": 0, "resetsAt": "2026-11-30T00:00:00.000Z"},
            "monthly": {"status": "ok", "percent": 6, "resetsAt": "2026-12-04T12:21:21.000Z"}
        }});
        let w = opencode_go_windows(&v);
        let labels: Vec<&str> = w.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, ["Session", "Weekly", "Monthly"]);
        assert!((w[2].used_percent - 6.0).abs() < f64::EPSILON);
        assert_eq!(w[1].resets_at, Some(1_795_996_800));
        assert!(opencode_go_windows(&json!({"error": "nope"})).is_empty());
    }

    #[test]
    fn antigravity_shows_each_group_by_window() {
        let v = json!({"groups": [
            {"displayName": "Claude and GPT models", "buckets": [
                {"bucketId": "3p-weekly", "window": "weekly", "resetTime": "2026-12-03T19:32:00Z", "remainingFraction": 0.75},
                {"bucketId": "3p-5h", "window": "5h", "resetTime": "2026-11-27T00:32:00Z", "remainingFraction": 1}
            ]},
            {"displayName": "Gemini models", "buckets": [
                {"bucketId": "gemini-5h", "window": "5h", "remainingFraction": 0.4},
                {"bucketId": "gemini-odd", "window": "5h"}
            ]}
        ]});
        let w = antigravity_windows(&v);
        let rows: Vec<(&str, String)> = w
            .iter()
            .map(|w| (w.label.as_str(), w.models.join(",")))
            .collect();
        assert_eq!(
            rows,
            [
                ("Session", "claude,gpt".to_string()),
                ("Session", "gemini".to_string()),
                ("Weekly", "claude,gpt".to_string())
            ]
        );
        assert!((w[2].used_percent - 25.0).abs() < 1e-9);
        assert!((w[1].used_percent - 60.0).abs() < 1e-9);
        assert_eq!(w[0].resets_at, Some(1_795_739_520));
        assert!(antigravity_windows(&json!({})).is_empty());
    }
}
