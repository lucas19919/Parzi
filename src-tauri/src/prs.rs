//! Open pull requests for a folder's repository, from `gh`, cached for a
//! minute: the Changes card shows the one on the session's branch.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::AppState;

const PRS_FRESH: Duration = Duration::from_secs(60);
const GH_BUDGET: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrInfo {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub head: String,
    pub state: String,
    pub draft: bool,
    pub checks: String,
}

fn checks_of(rollup: &Value) -> &'static str {
    let Some(list) = rollup.as_array().filter(|l| !l.is_empty()) else {
        return "none";
    };
    let word = |c: &Value, k: &str| {
        c.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_uppercase()
    };
    let mut pending = false;
    for c in list {
        let (state, status) = (word(c, "state"), word(c, "status"));
        let verdict = if !state.is_empty() {
            state
        } else if !status.is_empty() && status != "COMPLETED" {
            "PENDING".to_string()
        } else {
            word(c, "conclusion")
        };
        match verdict.as_str() {
            "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED"
            | "STARTUP_FAILURE" => return "fail",
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => {}
            _ => pending = true,
        }
    }
    if pending {
        "pending"
    } else {
        "pass"
    }
}

pub(crate) fn parse_prs(json: &str) -> Vec<PrInfo> {
    let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(json) else {
        return vec![];
    };
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    rows.iter()
        .filter_map(|r| {
            let number = r.get("number").and_then(Value::as_u64)?;
            let merged = r.get("mergedAt").is_some_and(|m| !m.is_null())
                || s(r, "state").eq_ignore_ascii_case("merged");
            let state = if merged {
                "merged"
            } else if s(r, "state").eq_ignore_ascii_case("open") {
                "open"
            } else {
                "closed"
            };
            Some(PrInfo {
                number,
                title: s(r, "title"),
                url: s(r, "url"),
                head: s(r, "headRefName"),
                state: state.into(),
                draft: r.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
                checks: checks_of(r.get("statusCheckRollup").unwrap_or(&Value::Null)).into(),
            })
        })
        .collect()
}

type PrCache = HashMap<String, (Instant, Vec<PrInfo>)>;
static PRS: Mutex<Option<PrCache>> = Mutex::new(None);

fn cache_key(cwd: &str) -> String {
    let k = cwd.trim().replace('\\', "/");
    let k = k.trim_end_matches('/');
    if cfg!(windows) {
        k.to_lowercase()
    } else {
        k.to_string()
    }
}

fn cached(cwd: &str) -> Option<Vec<PrInfo>> {
    let g = PRS.lock().unwrap_or_else(PoisonError::into_inner);
    let (at, prs) = g.as_ref()?.get(&cache_key(cwd))?;
    (at.elapsed() < PRS_FRESH).then(|| prs.clone())
}

fn remember(cwd: &str, prs: Vec<PrInfo>) {
    let mut g = PRS.lock().unwrap_or_else(PoisonError::into_inner);
    let map = g.get_or_insert_with(HashMap::new);
    map.retain(|_, (at, _)| at.elapsed() < PRS_FRESH);
    map.insert(cache_key(cwd), (Instant::now(), prs));
}

async fn gh_prs(dir: &Path) -> Option<String> {
    let mut cmd = tokio::process::Command::new("gh");
    cmd.args([
        "pr",
        "list",
        "--state",
        "all",
        "--limit",
        "100",
        "--json",
        "number,title,url,headRefName,state,isDraft,mergedAt,statusCheckRollup",
    ])
    .current_dir(dir)
    .env("GH_PROMPT_DISABLED", "1")
    .stdin(Stdio::null())
    .stderr(Stdio::null())
    .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
    let out = tokio::time::timeout(GH_BUDGET, cmd.output())
        .await
        .ok()?
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[tauri::command]
pub async fn branch_prs(state: State<'_, AppState>, cwd: String) -> Result<Vec<PrInfo>, String> {
    if cwd.trim().is_empty() || crate::files::remote_or_device(&cwd) {
        return Ok(vec![]);
    }
    if let Some(hit) = cached(&cwd) {
        return Ok(hit);
    }
    let orch = state.orch.clone();
    let raw = cwd.clone();
    let dir = tauri::async_runtime::spawn_blocking(move || {
        crate::files::confined_path(orch.store(), &raw).or_else(|e| {
            let path = std::path::PathBuf::from(raw.trim());
            let project = crate::files::plain_local(&path)
                && path.is_dir()
                && parzi_core::brain::project_for_folder(&raw).is_some();
            if project {
                Ok(path)
            } else {
                Err(e)
            }
        })
    })
    .await;
    let Ok(Ok(dir)) = dir else {
        return Ok(vec![]);
    };
    let prs = gh_prs(&dir)
        .await
        .map(|j| parse_prs(&j))
        .unwrap_or_default();
    remember(&cwd, prs.clone());
    Ok(prs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIXTURE: &str = r#"[
      {"number": 42, "title": "Session names", "url": "https://github.com/o/r/pull/42",
       "headRefName": "plan/names", "state": "OPEN", "isDraft": false, "mergedAt": null,
       "statusCheckRollup": [
         {"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "SUCCESS"},
         {"__typename": "StatusContext", "context": "ci", "state": "SUCCESS"}
       ]},
      {"number": 41, "title": "Panel", "url": "https://github.com/o/r/pull/41",
       "headRefName": "plan/panel", "state": "OPEN", "isDraft": true, "mergedAt": null,
       "statusCheckRollup": [
         {"__typename": "CheckRun", "name": "test", "status": "IN_PROGRESS", "conclusion": ""},
         {"__typename": "CheckRun", "name": "lint", "status": "COMPLETED", "conclusion": "SUCCESS"}
       ]},
      {"number": 40, "title": "Viz", "url": "https://github.com/o/r/pull/40",
       "headRefName": "plan/viz", "state": "MERGED", "isDraft": false,
       "mergedAt": "2026-10-01T10:00:00Z", "statusCheckRollup": []},
      {"number": 39, "title": "Old", "url": "https://github.com/o/r/pull/39",
       "headRefName": "feature/x", "state": "CLOSED", "isDraft": false, "mergedAt": null,
       "statusCheckRollup": [
         {"__typename": "CheckRun", "status": "COMPLETED", "conclusion": "FAILURE"},
         {"__typename": "StatusContext", "state": "PENDING"}
       ]},
      {"title": "no number, skipped"}
    ]"#;

    #[test]
    fn gh_rows_become_badges() {
        let prs = parse_prs(FIXTURE);
        assert_eq!(prs.len(), 4);
        assert_eq!(
            prs[0],
            PrInfo {
                number: 42,
                title: "Session names".into(),
                url: "https://github.com/o/r/pull/42".into(),
                head: "plan/names".into(),
                state: "open".into(),
                draft: false,
                checks: "pass".into(),
            }
        );
        assert_eq!((prs[1].draft, prs[1].checks.as_str()), (true, "pending"));
        assert_eq!(
            (prs[2].state.as_str(), prs[2].checks.as_str()),
            ("merged", "none")
        );
        assert_eq!(
            (prs[3].state.as_str(), prs[3].checks.as_str()),
            ("closed", "fail")
        );
        let v = serde_json::to_value(&prs[0]).unwrap();
        assert_eq!(
            v,
            json!({"number": 42, "title": "Session names", "url": "https://github.com/o/r/pull/42",
                   "head": "plan/names", "state": "open", "draft": false, "checks": "pass"})
        );
        assert!(parse_prs("gh: not logged in").is_empty());
        assert!(parse_prs("{}").is_empty());
    }
}
