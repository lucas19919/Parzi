//! What the desk's control socket and `parzi serve` answer the same way:
//! request arguments, the token check, and the session ops that need no
//! window. Each socket keeps only what is its own (tabs and dialogs on the
//! desk, sync and the mirror on serve).

use serde_json::{json, Value};

use crate::orchestrator::Orchestrator;
use crate::tools::{humanize_tool_call, ToolCallInfo};

/// What a run hears when nobody answered its question in time.
pub const NO_ANSWER: &str = "the user didn't answer. Decide yourself and say what you assumed";

/// Characters of `session.md` a `session.show` or `session.export` returns.
const TRANSCRIPT_CAP: usize = 20_000;

#[must_use]
pub fn fail(error: impl Into<String>) -> Value {
    json!({ "ok": false, "error": error.into() })
}

/// A string argument, trimmed; missing is empty.
#[must_use]
pub fn str_arg(req: &Value, key: &str) -> String {
    req.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

#[must_use]
pub fn bool_arg(req: &Value, key: &str, dflt: bool) -> bool {
    req.get(key).and_then(Value::as_bool).unwrap_or(dflt)
}

/// Constant-time: the reply time must not reveal how much of a guess matched.
#[must_use]
pub fn tokens_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// A waiting approval as `approval.list` and the event streams show it.
#[must_use]
pub fn approval_row(key: &str, call: &ToolCallInfo) -> Value {
    json!({
        "key": key,
        "session": call.session,
        "name": call.name,
        "lane": call.lane,
        "label": humanize_tool_call(&call.name, &call.args),
    })
}

/// A full key, or a prefix (4 characters or more) only one key has: what
/// `parzi approval allow <short>` sends.
pub fn match_key<'a, I>(keys: I, asked: &str) -> Result<String, String>
where
    I: IntoIterator<Item = &'a String>,
{
    let asked = asked.trim();
    if asked.is_empty() {
        return Err("an approval key is required".into());
    }
    let mut hits = vec![];
    for k in keys {
        if k == asked {
            return Ok(k.clone());
        }
        if asked.len() >= 4 && k.starts_with(asked) {
            hits.push(k.clone());
        }
    }
    match hits.len() {
        1 => Ok(hits.remove(0)),
        0 => Err("approval expired or unknown".into()),
        _ => Err("that prefix fits more than one approval".into()),
    }
}

/// The project a folder's sessions belong to: the brain project whose
/// folder holds it, else `default`. Network and device paths are never
/// looked at, and the vault scan runs off the async threads.
pub async fn project_for(cwd: &str) -> String {
    if cwd.trim().is_empty() || parzi_core::paths::is_network_path(cwd) {
        return "default".into();
    }
    let cwd = cwd.to_string();
    tokio::task::spawn_blocking(move || parzi_core::brain::project_for_folder(&cwd))
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "default".into())
}

fn resolve(orch: &Orchestrator, req: &Value) -> Result<String, Value> {
    orch.store()
        .resolve_id(&str_arg(req, "id"))
        .map_err(|e| fail(e.to_string()))
}

macro_rules! resolved {
    ($orch:expr, $req:expr) => {
        match resolve($orch, $req) {
            Ok(id) => id,
            Err(v) => return v,
        }
    };
}

#[must_use]
pub fn session_list(orch: &Orchestrator) -> Value {
    match orch.store().list() {
        Ok(rows) => json!({ "ok": true, "sessions": rows }),
        Err(e) => fail(e.to_string()),
    }
}

/// The session and its transcript, rendered now: `session.md` on disk lags
/// a running or renamed session.
#[must_use]
pub fn session_show(orch: &Orchestrator, req: &Value, export: bool) -> Value {
    let id = resolved!(orch, req);
    let meta = match orch.store().get(&id) {
        Ok(m) => m,
        Err(e) => return fail(e.to_string()),
    };
    let md = match orch.store().transcript_md(&id) {
        Ok(md) => md,
        Err(e) => return fail(e.to_string()),
    };
    let text: String = md.chars().take(TRANSCRIPT_CAP).collect();
    if export {
        json!({ "ok": true, "id": id, "text": text })
    } else {
        json!({ "ok": true, "session": meta, "text": text })
    }
}

pub async fn session_kill(orch: &Orchestrator, req: &Value) -> Value {
    let id = resolved!(orch, req);
    match orch.kill(&id).await {
        Ok(()) => json!({ "ok": true, "id": id }),
        Err(e) => fail(e.to_string()),
    }
}

#[must_use]
pub fn session_fork(orch: &Orchestrator, req: &Value) -> Value {
    let id = resolved!(orch, req);
    let at = req
        .get("at")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok());
    match orch.fork(&id, at) {
        Ok(meta) => json!({ "ok": true, "id": meta.id, "session": meta }),
        Err(e) => fail(e.to_string()),
    }
}

#[must_use]
pub fn session_rename(orch: &Orchestrator, req: &Value) -> Value {
    let id = resolved!(orch, req);
    match orch.rename(&id, &str_arg(req, "title")) {
        Ok(()) => json!({ "ok": true, "id": id }),
        Err(e) => fail(e.to_string()),
    }
}

/// Deletes without asking: the desk asks the person before it calls this.
pub async fn session_delete(orch: &Orchestrator, req: &Value) -> Value {
    let id = resolved!(orch, req);
    match orch.delete_thread(&id).await {
        Ok(n) => json!({ "ok": true, "id": id, "removed": n }),
        Err(e) => fail(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_compare_rejects_a_near_miss() {
        assert!(tokens_equal("abc", "abc"));
        assert!(!tokens_equal("abc", "abd"));
        assert!(!tokens_equal("abc", "abcd"));
        assert!(!tokens_equal("", "a"));
    }

    #[test]
    fn keys_match_whole_or_by_a_unique_prefix() {
        let keys = ["abcd-1234".to_string(), "abcd-5678".to_string()];
        assert_eq!(match_key(&keys, "abcd-5678").unwrap(), "abcd-5678");
        assert_eq!(match_key(&keys, " abcd-1 ").unwrap(), "abcd-1234");
        assert!(match_key(&keys, "abcd").is_err(), "ambiguous");
        assert!(match_key(&keys, "ab").is_err(), "too short");
        assert!(match_key(&keys, "").is_err());
        assert!(match_key(&keys, "zzzz").is_err());
    }
}
