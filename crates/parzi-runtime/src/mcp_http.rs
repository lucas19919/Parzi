
use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum HttpError {
    SignIn(Option<String>),
    Other(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignIn(_) => f.write_str(SIGN_IN),
            Self::Other(e) => f.write_str(e),
        }
    }
}

pub const SIGN_IN: &str = "sign-in needed";

static CHALLENGES: std::sync::Mutex<Option<HashMap<String, String>>> = std::sync::Mutex::new(None);

pub fn remember_challenge(name: &str, header: Option<String>) {
    if let (Ok(mut m), Some(h)) = (CHALLENGES.lock(), header) {
        m.get_or_insert_with(HashMap::new)
            .insert(name.to_string(), h);
    }
}

#[must_use]
pub fn last_challenge(name: &str) -> Option<String> {
    CHALLENGES.lock().ok()?.as_ref()?.get(name).cloned()
}

#[derive(Debug, Clone, PartialEq)]
enum Auth {
    Headers,
    Gh,
}

pub struct HttpConn {
    name: String,
    url: String,
    headers: HashMap<String, String>,
    auth: Auth,
    session: Option<String>,
    protocol: Option<String>,
    gh_token: Option<String>,
}

impl HttpConn {
    #[must_use]
    pub fn new(name: &str, cfg: &parzi_core::config::McpServerCfg) -> Self {
        Self {
            name: name.to_string(),
            url: cfg.url.clone(),
            headers: cfg.headers.clone(),
            auth: if cfg.auth == "gh" {
                Auth::Gh
            } else {
                Auth::Headers
            },
            session: None,
            protocol: None,
            gh_token: None,
        }
    }

    pub fn set_protocol(&mut self, v: &str) {
        self.protocol = Some(v.to_string());
    }

    pub fn send(&mut self, msg: &Value, timeout: Duration) -> Result<Option<Value>, HttpError> {
        let mut retried = false;
        loop {
            let bearer = self.bearer()?;
            let (status, www_auth, session, ctype, body) =
                self.post(msg, bearer.as_deref(), timeout)?;
            if status == 401 || (status == 403 && www_auth.is_some()) {
                if !retried
                    && self.auth == Auth::Headers
                    && crate::mcp_oauth::refresh(&self.name).is_ok()
                {
                    retried = true;
                    continue;
                }
                if !retried && self.auth == Auth::Gh {
                    self.gh_token = None;
                    retried = true;
                    continue;
                }
                return Err(HttpError::SignIn(www_auth));
            }
            if status == 404 && self.session.is_some() {
                self.session = None;
                return Err(HttpError::Other(
                    "the server ended this connection; it reconnects on the next call".into(),
                ));
            }
            if !(200..300).contains(&status) {
                let detail: String = body.chars().take(200).collect();
                return Err(HttpError::Other(format!(
                    "the server answered {status}: {detail}"
                )));
            }
            if let Some(s) = session {
                self.session = Some(s);
            }
            let Some(id) = msg.get("id") else {
                return Ok(None);
            };
            if status == 202 || body.trim().is_empty() {
                return Err(HttpError::Other("the server sent no reply".into()));
            }
            let replies = if ctype.contains("text/event-stream") {
                events(&body)
            } else {
                match serde_json::from_str::<Value>(&body) {
                    Ok(Value::Array(all)) => all,
                    Ok(one) => vec![one],
                    Err(e) => {
                        return Err(HttpError::Other(format!(
                            "the server's reply was not JSON: {e}"
                        )))
                    }
                }
            };
            return replies
                .into_iter()
                .find(|r| r.get("id") == Some(id))
                .map(Some)
                .ok_or_else(|| {
                    HttpError::Other("the server's reply did not answer the request".into())
                });
        }
    }

    fn bearer(&mut self) -> Result<Option<String>, HttpError> {
        match self.auth {
            Auth::Gh => {
                if self.gh_token.is_none() {
                    self.gh_token = Some(gh_token()?);
                }
                Ok(self.gh_token.clone())
            }
            Auth::Headers
                if self
                    .headers
                    .keys()
                    .any(|k| k.eq_ignore_ascii_case("authorization")) =>
            {
                Ok(None)
            }
            Auth::Headers => Ok(crate::mcp_oauth::bearer(&self.name)),
        }
    }

    #[allow(clippy::type_complexity)]
    fn post(
        &self,
        msg: &Value,
        bearer: Option<&str>,
        timeout: Duration,
    ) -> Result<(u16, Option<String>, Option<String>, String, String), HttpError> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build()
            .into();
        let mut req = agent
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");
        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        if let Some(b) = bearer {
            req = req.header("Authorization", format!("Bearer {b}").as_str());
        }
        if let Some(s) = &self.session {
            req = req.header("Mcp-Session-Id", s.as_str());
        }
        if let Some(p) = &self.protocol {
            req = req.header("MCP-Protocol-Version", p.as_str());
        }
        let mut res = req
            .send(msg.to_string())
            .map_err(|e| HttpError::Other(format!("could not reach {}: {e}", self.url)))?;
        let header = |k: &str| {
            res.headers()
                .get(k)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let (www, session, ctype) = (
            header("www-authenticate"),
            header("mcp-session-id"),
            header("content-type"),
        );
        let body = res
            .body_mut()
            .with_config()
            .limit(32 << 20)
            .read_to_string()
            .map_err(|e| HttpError::Other(format!("reading the reply failed: {e}")))?;
        Ok((
            res.status().as_u16(),
            www,
            session,
            ctype.unwrap_or_default(),
            body,
        ))
    }
}

#[must_use]
pub fn events(body: &str) -> Vec<Value> {
    let mut out = vec![];
    let mut data: Vec<&str> = vec![];
    let flush = |data: &mut Vec<&str>, out: &mut Vec<Value>| {
        if !data.is_empty() {
            if let Ok(v) = serde_json::from_str::<Value>(&data.join("\n")) {
                out.push(v);
            }
            data.clear();
        }
    };
    for line in body.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            flush(&mut data, &mut out);
        } else if let Some(d) = line.strip_prefix("data:") {
            data.push(d.strip_prefix(' ').unwrap_or(d));
        }
    }
    flush(&mut data, &mut out);
    out
}

fn gh_token() -> Result<String, HttpError> {
    let gh = parzi_providers::process::which("gh").ok_or_else(|| {
        HttpError::Other(
            "GitHub connects through the GitHub CLI (gh), which isn't installed".into(),
        )
    })?;
    let mut cmd = std::process::Command::new(gh);
    cmd.args(["auth", "token"])
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(parzi_providers::process::CREATE_NO_WINDOW);
    }
    let out = cmd
        .output()
        .map_err(|e| HttpError::Other(format!("gh did not run: {e}")))?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || token.is_empty() {
        return Err(HttpError::Other(
            "the GitHub CLI isn't signed in: run `gh auth login`".into(),
        ));
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_stream_yields_its_messages() {
        let body = "event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\r\n\r\nid: 2\ndata: {\"jsonrpc\":\"2.0\",\ndata: \"id\":1,\"result\":{}}\n\n";
        let got = events(body);
        assert_eq!(got.len(), 2);
        assert_eq!(got[1]["id"], 1);
    }
}
