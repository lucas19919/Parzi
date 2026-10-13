//! The desk's side of plans. The plan panel reads a project's plan
//! (`plan_view`) and the pull requests of its folder (`plan_prs`, from
//! `gh`), and hears `parzi://plan` when a plan note changes here or comes
//! in with a brain sync. Plan items the person quotes into the composer
//! reach the prompt as `<plan-item>` blocks after their text.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use parzi_core::brain::Vault;
use parzi_core::plan::{self, PlanItem, PlanView};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::AppState;

/// How long `plan_prs` answers from what `gh` said last.
const PRS_FRESH: Duration = Duration::from_secs(60);
/// `gh` gets this long; a slow network just shows no badges.
const GH_BUDGET: Duration = Duration::from_secs(20);
/// Plan items one message may quote.
pub(crate) const MAX_QUOTES: usize = 4;

/// Plans this process writes (an agent's `plan.write`) reach the window.
pub(crate) fn watch(app: &AppHandle) {
    let app = app.clone();
    plan::on_change(move |slug| emit(&app, slug));
}

fn emit(app: &AppHandle, project: &str) {
    let _ = app.emit("parzi://plan", json!({ "project": project }));
}

/// Notes that changed here (a brain sync, the Brain tab): each plan among
/// them is news for the window, once.
pub(crate) fn synced(app: &AppHandle, paths: &[String]) {
    for slug in changed_plans(paths) {
        emit(app, &slug);
    }
}

fn changed_plans(paths: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for slug in paths.iter().filter_map(|p| plan::project_of_note(p)) {
        if !out.contains(&slug) {
            out.push(slug);
        }
    }
    out
}

#[tauri::command]
pub async fn plan_view(project: String) -> Result<PlanView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let vault = Vault::open().map_err(|e| e.to_string())?;
        Ok(plan::load(&vault, &project))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A pull request as the plan panel badges it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrInfo {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// The head branch's name.
    pub head: String,
    /// open, closed or merged.
    pub state: String,
    pub draft: bool,
    /// pass, fail, pending or none.
    pub checks: String,
}

/// What `statusCheckRollup` says in one word. A check run reports a status
/// and, once completed, a conclusion; a commit status reports a state.
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

/// `gh pr list --json ...` as the panel reads it; anything unreadable is
/// no pull requests.
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

/// What `gh` said about `cwd` within the last minute.
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

/// `gh pr list` in `dir`, in a hidden console. None when `gh` is missing,
/// not signed in, the folder is no GitHub repo, or it takes too long.
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

/// The pull requests of the repo at `cwd`, newest first. Empty, never an
/// error, when `gh` cannot tell (missing, signed out, not GitHub).
#[tauri::command]
pub async fn plan_prs(state: State<'_, AppState>, cwd: String) -> Result<Vec<PrInfo>, String> {
    if cwd.trim().is_empty() || crate::files::remote_or_device(&cwd) {
        return Ok(vec![]);
    }
    if let Some(hit) = cached(&cwd) {
        return Ok(hit);
    }
    let orch = state.orch.clone();
    let raw = cwd.clone();
    let dir = tauri::async_runtime::spawn_blocking(move || {
        crate::files::confined_path(orch.store(), &raw)
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

/// The pull request of an item: on its branch, or on `plan/<id>`. An open
/// one first, then a merged one, then the newest.
fn pr_of<'a>(prs: &'a [PrInfo], item: &PlanItem) -> Option<&'a PrInfo> {
    let own = format!("plan/{}", item.id);
    let mine: Vec<&PrInfo> = prs
        .iter()
        .filter(|p| p.head == own || (!item.branch.is_empty() && p.head == item.branch))
        .collect();
    let rank = |p: &&PrInfo| match p.state.as_str() {
        "open" => 0,
        "merged" => 1,
        _ => 2,
    };
    mine.iter().min_by_key(|p| rank(p)).copied()
}

/// How an item's pull request stands, for its quote block.
fn pr_words(p: &PrInfo) -> String {
    match p.state.as_str() {
        "merged" => format!("PR #{} merged.", p.number),
        "closed" => format!("PR #{} closed.", p.number),
        _ => {
            let open = if p.draft { "open as a draft" } else { "open" };
            let checks = match p.checks.as_str() {
                "pass" => ", checks pass",
                "fail" => ", checks fail",
                "pending" => ", checks running",
                _ => "",
            };
            format!("PR #{} {open}{checks}.", p.number)
        }
    }
}

/// A plan item quoted into the composer.
#[derive(Debug, Clone, Deserialize)]
pub struct Quote {
    pub project: String,
    pub id: String,
}

/// The items `quotes` name, as their plans hold them now (at most
/// `MAX_QUOTES`; one gone from its plan is left out).
pub(crate) fn resolve(quotes: &[Quote]) -> Vec<(String, PlanItem)> {
    let Ok(vault) = Vault::open() else {
        return vec![];
    };
    let mut plans: HashMap<String, Option<plan::Plan>> = HashMap::new();
    let mut out: Vec<(String, PlanItem)> = vec![];
    for q in quotes.iter().take(MAX_QUOTES) {
        let project = q.project.trim().to_string();
        let p = plans
            .entry(project.clone())
            .or_insert_with(|| plan::load(&vault, &project).plan);
        let Some(item) = p.as_ref().and_then(|p| p.item(q.id.trim())) else {
            continue;
        };
        if !out.iter().any(|(pr, i)| *pr == project && i.id == item.id) {
            out.push((project, item.clone()));
        }
    }
    out
}

/// The prompt a send with quoted items carries: the person's text (or, with
/// none, "Work on <title>."), then one `<plan-item>` block per item, so the
/// session's name still comes from the text. `cwd` finds a pull request
/// among those the panel last listed.
pub(crate) fn quoted_prompt(text: &str, items: &[(String, PlanItem)], cwd: &str) -> String {
    let prs = cached(cwd).unwrap_or_default();
    let mut out = match (text.trim(), items.first()) {
        ("", Some((_, first))) => format!("Work on {}.", first.title),
        (t, _) => t.to_string(),
    };
    for (project, item) in items {
        let pr = pr_of(&prs, item).map(pr_words);
        out.push_str("\n\n");
        out.push_str(&plan::quote_block(project, item, pr.as_deref()));
    }
    out
}

/// Whether a new session started from quoted items is named after the
/// first: when the person said little more than "start this".
pub(crate) fn named_after_item(text: &str) -> bool {
    text.split_whitespace().count() <= 3
}

/// The item a session in `project` is linked to: the first quoted item of
/// that project's plan (another project's item is context, not its task).
pub(crate) fn link_for<'a>(project: &str, items: &'a [(String, PlanItem)]) -> Option<&'a str> {
    items
        .iter()
        .find(|(p, _)| p == project)
        .map(|(_, i)| i.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn item(id: &str, branch: &str) -> PlanItem {
        PlanItem {
            id: id.into(),
            title: format!("Item {id}"),
            detail: "Do the thing".into(),
            phase: String::new(),
            needs: vec![],
            state: plan::PlanState::Active,
            branch: branch.into(),
            note: String::new(),
        }
    }

    #[test]
    fn an_item_finds_its_pull_request_by_branch() {
        let prs = parse_prs(FIXTURE);
        assert_eq!(pr_of(&prs, &item("names", "")).map(|p| p.number), Some(42));
        assert_eq!(
            pr_of(&prs, &item("other", "feature/x")).map(|p| p.number),
            Some(39)
        );
        assert_eq!(pr_of(&prs, &item("none", "")), None);
        assert_eq!(pr_words(&prs[0]), "PR #42 open, checks pass.");
        assert_eq!(pr_words(&prs[1]), "PR #41 open as a draft, checks running.");
        assert_eq!(pr_words(&prs[2]), "PR #40 merged.");
    }

    #[test]
    fn quotes_follow_the_text_and_an_empty_send_works_on_the_item() {
        let cwd = "C:/test/quotes";
        remember(cwd, parse_prs(FIXTURE));
        let items = vec![
            ("parzi".to_string(), item("names", "plan/names")),
            ("parzi".to_string(), item("panel", "")),
        ];
        let p = quoted_prompt("why is this stuck?", &items, cwd);
        assert!(p.starts_with("why is this stuck?\n\n<plan-item project=\"parzi\" id=\"names\" title=\"Item names\">\nDo the thing\nState: active. Needs: none. Branch: plan/names. PR #42 open, checks pass.\n</plan-item>"), "{p}");
        assert!(
            p.ends_with("PR #41 open as a draft, checks running.\n</plan-item>"),
            "{p}"
        );
        assert_eq!(p.matches("<plan-item ").count(), 2);
        let p = quoted_prompt("  ", &items[..1], "C:/elsewhere");
        assert!(p.starts_with("Work on Item names.\n\n<plan-item"), "{p}");
        assert!(!p.contains("PR #"), "no list for that folder: {p}");
        assert_eq!(
            parzi_core::text::name_from_prompt(&quoted_prompt("why is this stuck?", &items, cwd)),
            "Why is this stuck",
            "the name comes from the text, not the blocks"
        );
        assert!(named_after_item("start this") && named_after_item(""));
        assert!(!named_after_item("split this into two items please"));
        let mixed = vec![
            ("other".to_string(), item("x", "")),
            ("parzi".to_string(), item("names", "")),
        ];
        assert_eq!(link_for("parzi", &mixed), Some("names"));
        assert_eq!(link_for("default", &mixed), None);
    }

    #[test]
    fn plan_notes_among_changed_paths_are_news_once() {
        let paths = [
            "projects/parzi/plan.md".to_string(),
            "notes/a.md".to_string(),
            "projects/parzi/plan.md".to_string(),
            "projects/other/plan.md".to_string(),
            "projects/parzi/plans/2026-10-13-old.md".to_string(),
        ];
        assert_eq!(changed_plans(&paths), ["parzi", "other"]);
    }
}
