//! GitHub for the workspace wizard: a token in the keyring, orgs and repos
//! over the REST API, and clone-or-map for a repo the workspace names.
//!
//! Domainless by design (hub PLAN.md §1.1): there is no Parzi OAuth app. The
//! user pastes a personal access token, or lends us the one `gh` already
//! holds. The token lives in the OS keyring under `parzi/github` and never
//! reaches the webview — the UI only ever sees a login name.
//!
//! This lives in parzi-providers, not in the shell (AUDIT S-1): a vendor
//! client holding a credential is exactly what this crate is for, and the
//! Tauri commands over it are one-line forwards.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

mod askpass;
mod checkout;
pub use checkout::{clone_and_map, prepare, verify_map, Prepared};

const SERVICE: &str = "parzi";
const ACCOUNT: &str = "github";
const API: &str = "https://api.github.com";
const PER_PAGE: usize = 100;
/// 10 pages × 100 = 1000 repos; past that the picker's search is the answer.
const MAX_PAGES: usize = 10;
/// GitHub's own rule for a login: 1–39 of `[A-Za-z0-9-]`.
const MAX_LOGIN: usize = 39;

/// One organisation the signed-in user belongs to.
#[derive(Debug, Clone, Serialize)]
pub struct Org {
    pub login: String,
    pub name: String,
}

/// One repository, reduced to what `workspace.toml` stores.
#[derive(Debug, Clone, Serialize)]
pub struct Repo {
    pub name: String,
    pub full_name: String,
    pub default_branch: String,
    pub clone_url: String,
    pub private: bool,
}

/// Who we are and how we got there — the UI's whole view of the credential.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub connected: bool,
    pub login: String,
    /// `token` (pasted or from gh) or `none`.
    pub source: String,
    /// `gh` is on PATH, so "Use gh" is worth offering.
    pub gh: bool,
}

#[derive(Deserialize)]
struct ApiUser {
    login: String,
}

#[derive(Deserialize)]
struct ApiOrg {
    login: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct ApiRepo {
    name: String,
    full_name: String,
    #[serde(default)]
    default_branch: Option<String>,
    #[serde(default)]
    clone_url: Option<String>,
    #[serde(default)]
    private: bool,
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(concat!("parzi/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client")
    })
}

/// The stored token, if the keyring has one.
#[must_use]
pub fn token() -> Option<String> {
    keyring::Entry::new(SERVICE, ACCOUNT)
        .ok()?
        .get_password()
        .ok()
        .filter(|t| !t.trim().is_empty())
}

fn store(tok: &str) -> Result<(), String> {
    keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| e.to_string())?
        .set_password(tok)
        .map_err(|e| e.to_string())
}

/// A GitHub login (user or org): 1–39 of `[A-Za-z0-9-]`, nothing else. A
/// login is interpolated into an API path, so a `/`, a `?` or a `..` would
/// address a different endpoint entirely — it is checked, never escaped.
#[must_use]
pub fn valid_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= MAX_LOGIN
        && login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Is the GitHub CLI on PATH? Decides whether the UI offers "Use gh".
pub async fn gh_present() -> bool {
    tokio::process::Command::new("gh")
        .arg("--version")
        .output()
        .await
        .is_ok_and(|o| o.status.success())
}

/// The token `gh auth login` already produced, so nobody pastes anything.
async fn gh_token() -> Result<String, String> {
    let out = tokio::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .await
        .map_err(|e| format!("gh: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "gh auth token failed — run `gh auth login` first ({})",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let tok = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if tok.is_empty() {
        return Err("gh returned no token — run `gh auth login` first".into());
    }
    Ok(tok)
}

async fn get_json<T: serde::de::DeserializeOwned>(tok: &str, url: &str) -> Result<T, String> {
    let res = client()
        .get(url)
        .bearer_auth(tok)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        let msg: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        let detail = msg
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(status.as_str());
        return Err(format!("GitHub {status}: {detail}"));
    }
    res.json::<T>().await.map_err(|e| e.to_string())
}

async fn login_of(tok: &str) -> Result<String, String> {
    let u: ApiUser = get_json(tok, &format!("{API}/user")).await?;
    Ok(u.login)
}

/// Accept a pasted token, or take the one `gh` holds when `paste` is empty.
/// Verifies it against `/user` before storing, so a typo fails here and not
/// three steps later.
pub async fn connect(paste: Option<String>) -> Result<Status, String> {
    let tok = match paste
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
    {
        Some(p) => p,
        None => gh_token().await?,
    };
    let login = login_of(&tok).await?;
    store(&tok)?;
    Ok(Status {
        connected: true,
        login,
        source: "token".into(),
        gh: gh_present().await,
    })
}

/// Current credential state. One network call when a token exists, none when
/// it does not.
pub async fn status() -> Status {
    let gh = gh_present().await;
    match token() {
        Some(tok) => match login_of(&tok).await {
            Ok(login) => Status {
                connected: true,
                login,
                source: "token".into(),
                gh,
            },
            Err(_) => Status {
                connected: false,
                login: String::new(),
                source: "none".into(),
                gh,
            },
        },
        None => Status {
            connected: false,
            login: String::new(),
            source: "none".into(),
            gh,
        },
    }
}

fn need_token() -> Result<String, String> {
    token().ok_or_else(|| "not connected to GitHub".to_string())
}

/// Organisations, newest first as GitHub returns them.
pub async fn orgs() -> Result<Vec<Org>, String> {
    let tok = need_token()?;
    let mut out = Vec::new();
    for page in 1..=MAX_PAGES {
        let url = format!("{API}/user/orgs?per_page={PER_PAGE}&page={page}");
        let batch: Vec<ApiOrg> = get_json(&tok, &url).await?;
        let n = batch.len();
        out.extend(batch.into_iter().map(|o| Org {
            name: o.name.unwrap_or_else(|| o.login.clone()),
            login: o.login,
        }));
        if n < PER_PAGE {
            break;
        }
    }
    Ok(out)
}

/// Repos for one org, or the signed-in user's own when `org` is empty. The
/// org is validated before it reaches the URL: the wizard sends what a picker
/// chose, but the command behind this is callable with anything.
pub async fn repos(org: &str) -> Result<Vec<Repo>, String> {
    if !org.is_empty() && !valid_login(org) {
        return Err("not a GitHub organisation name".to_string());
    }
    let tok = need_token()?;
    let mut out = Vec::new();
    for page in 1..=MAX_PAGES {
        let url = if org.is_empty() {
            format!("{API}/user/repos?affiliation=owner,collaborator&sort=updated&per_page={PER_PAGE}&page={page}")
        } else {
            format!("{API}/orgs/{org}/repos?sort=updated&per_page={PER_PAGE}&page={page}")
        };
        let batch: Vec<ApiRepo> = get_json(&tok, &url).await?;
        let n = batch.len();
        out.extend(batch.into_iter().map(|r| {
            Repo {
                clone_url: r
                    .clone_url
                    .unwrap_or_else(|| format!("https://github.com/{}.git", r.full_name)),
                default_branch: r.default_branch.unwrap_or_else(|| "main".into()),
                name: r.name,
                full_name: r.full_name,
                private: r.private,
            }
        }));
        if n < PER_PAGE {
            break;
        }
    }
    Ok(out)
}

/// Parzi's own repo: agent self-reports land here, labelled, not mixed in
/// with human issues.
const SELF_REPO: &str = "lucas19919/Parzi";
/// Triage label for machine-filed issues. Humans file with `bug` from the app.
const AGENT_LABEL: &str = "agent-report";

#[derive(Deserialize)]
struct ApiIssue {
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    number: Option<u64>,
}

/// Validate a self-report before it touches the network: real titles and
/// bodies only, hard caps so a runaway loop cannot paste megabytes.
pub fn check_report(title: &str, body: &str) -> Result<(String, String), String> {
    let title = title.trim().to_string();
    let body = body.trim().to_string();
    if title.chars().count() < 10 {
        return Err("give the issue a real title (10+ characters)".into());
    }
    if title.chars().count() > 200 {
        return Err("title is over 200 characters".into());
    }
    if body.chars().count() < 40 {
        return Err(
            "describe the problem (40+ characters): what happened, what you expected".into(),
        );
    }
    if body.chars().count() > 8000 {
        return Err("body is over 8000 characters".into());
    }
    Ok((title, body))
}

async fn post_json<T: serde::de::DeserializeOwned>(
    tok: &str,
    url: &str,
    payload: &serde_json::Value,
) -> Result<T, String> {
    let res = client()
        .post(url)
        .bearer_auth(tok)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        let msg: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        let detail = msg
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(status.as_str());
        return Err(format!("GitHub {status}: {detail}"));
    }
    res.json::<T>().await.map_err(|e| e.to_string())
}

/// File an issue on Parzi itself — the agent self-report path. Uses the
/// stored token, borrowing `gh`'s when none is stored, so it works exactly
/// where the workspace wizard works. `client` names the calling harness
/// (from MCP `initialize`) for triage. Returns the issue URL.
pub async fn report_issue(title: &str, body: &str, client: &str) -> Result<String, String> {
    let (title, body) = check_report(title, body)?;
    let tok = match token() {
        Some(t) => t,
        None => gh_token().await?,
    };
    let reporter = client.trim();
    let footer = if reporter.is_empty() {
        "*Filed by an agent via `parzi mcp`.*".to_string()
    } else {
        format!("*Filed by an agent via `parzi mcp` ({reporter}).*")
    };
    let payload = serde_json::json!({
        "title": title,
        "body": format!("{body}\n\n---\n{footer}"),
        "labels": [AGENT_LABEL],
    });
    let issue: ApiIssue =
        post_json(&tok, &format!("{API}/repos/{SELF_REPO}/issues"), &payload).await?;
    issue
        .html_url
        .or_else(|| {
            issue
                .number
                .map(|n| format!("https://github.com/{SELF_REPO}/issues/{n}"))
        })
        .ok_or_else(|| "GitHub filed the issue but returned no URL".to_string())
}

#[cfg(test)]
mod tests {
    use super::{check_report, valid_login, MAX_LOGIN};

    #[test]
    fn a_login_is_letters_digits_and_dashes_and_nothing_else() {
        assert!(valid_login("acme"));
        assert!(valid_login("Lucas-19919"));
        assert!(valid_login(&"a".repeat(MAX_LOGIN)));
        // Anything that could re-address the URL is refused, not escaped.
        assert!(!valid_login(""));
        assert!(!valid_login(&"a".repeat(MAX_LOGIN + 1)));
        assert!(!valid_login("org/repos?per_page=1"));
        assert!(!valid_login("../../user"));
        assert!(!valid_login("org repo"));
        assert!(!valid_login("org%2f"));
        assert!(!valid_login("org#frag"));
    }

    #[test]
    fn self_reports_need_a_real_title_and_body() {
        assert!(check_report("short", "x".repeat(50).as_str()).is_err());
        assert!(check_report("a proper title here", "too short").is_err());
        assert!(check_report("   ", "x".repeat(50).as_str()).is_err());
        let (t, b) = check_report(
            "  Workspace picker eats clicks  ",
            "The picker opens but every click closes it. Expected the menu to stay open for a pick.",
        )
        .expect("valid report");
        assert_eq!(t, "Workspace picker eats clicks");
        assert!(b.starts_with("The picker opens"));
    }
}
