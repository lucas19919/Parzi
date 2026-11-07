use std::collections::HashMap;
use std::io::BufRead;
use std::time::Duration;

use serde_json::{json, Value};

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

/// How long the goodbye to a server may take; it runs on its own thread.
const GOODBYE_WAIT: Duration = Duration::from_secs(3);
const REPLY_LIMIT: u64 = 32 << 20;
const ERROR_LIMIT: u64 = 64 << 10;

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

/// Why one exchange did not end in an answer.
enum Failure {
    Http(HttpError),
    /// The server no longer knows our session: start a new one.
    Expired,
}

impl From<HttpError> for Failure {
    fn from(e: HttpError) -> Self {
        Self::Http(e)
    }
}

/// One Streamable HTTP connection: one keep-alive agent, the session the
/// server gave, and how to start that session again.
pub struct HttpConn {
    name: String,
    url: String,
    headers: HashMap<String, String>,
    auth: Auth,
    session: Option<String>,
    protocol: Option<String>,
    gh_token: Option<String>,
    agent: ureq::Agent,
    init: Option<Value>,
}

type Reply = ureq::http::Response<ureq::Body>;

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
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into(),
            init: None,
        }
    }

    pub fn set_protocol(&mut self, v: &str) {
        self.protocol = Some(v.to_string());
    }

    /// Sends one message; the answer to it when it is a request. When the
    /// server has ended the session, a new one is started and the message
    /// sent once more.
    pub fn send(&mut self, msg: &Value, timeout: Duration) -> Result<Option<Value>, HttpError> {
        if msg.get("method").and_then(Value::as_str) == Some("initialize") {
            self.init = Some(msg.clone());
            self.session = None;
        }
        match self.exchange(msg, timeout) {
            Ok(v) => Ok(v),
            Err(Failure::Http(e)) => Err(e),
            Err(Failure::Expired) => {
                self.renew(timeout)?;
                match self.exchange(msg, timeout) {
                    Ok(v) => Ok(v),
                    Err(Failure::Http(e)) => Err(e),
                    Err(Failure::Expired) => Err(HttpError::Other(
                        "the server ended the session again right after it started".into(),
                    )),
                }
            }
        }
    }

    fn renew(&mut self, timeout: Duration) -> Result<(), HttpError> {
        let Some(init) = self.init.clone() else {
            return Err(HttpError::Other(
                "the server ended this connection; it reconnects on the next call".into(),
            ));
        };
        tracing::debug!(server = %self.name, "mcp: the server ended the session; starting a new one");
        self.session = None;
        let expired = || HttpError::Other("the server would not start a new session".into());
        let reply = match self.exchange(&init, timeout) {
            Ok(r) => r,
            Err(Failure::Http(e)) => return Err(e),
            Err(Failure::Expired) => return Err(expired()),
        };
        if let Some(err) = reply.as_ref().and_then(|r| r.get("error")) {
            return Err(HttpError::Other(format!(
                "the server would not start a new session: {err}"
            )));
        }
        let ready = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        match self.exchange(&ready, timeout) {
            Ok(_) => Ok(()),
            Err(Failure::Http(e)) => Err(e),
            Err(Failure::Expired) => Err(expired()),
        }
    }

    fn exchange(&mut self, msg: &Value, timeout: Duration) -> Result<Option<Value>, Failure> {
        let static_auth = self
            .headers
            .keys()
            .any(|k| k.eq_ignore_ascii_case("authorization"));
        let mut retried = false;
        loop {
            let bearer = self.bearer()?;
            let mut res = self.post(msg, bearer.as_deref(), timeout)?;
            let status = res.status().as_u16();
            let header = |k: &str| {
                res.headers()
                    .get(k)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            };
            let (www_auth, session, ctype) = (
                header("www-authenticate"),
                header("mcp-session-id"),
                header("content-type").unwrap_or_default(),
            );
            let wall = status == 401 || (status == 403 && www_auth.is_some());
            if wall || !(200..300).contains(&status) {
                let body = read_text(&mut res, ERROR_LIMIT).unwrap_or_default();
                if let Some(err) = refused(static_auth, status, www_auth.as_deref(), &body) {
                    return Err(err.into());
                }
                if wall {
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
                    return Err(HttpError::SignIn(www_auth).into());
                }
                if status == 404 && self.session.is_some() {
                    self.session = None;
                    return Err(Failure::Expired);
                }
                if status == 405 && msg.get("method").and_then(Value::as_str) == Some("initialize")
                {
                    return Err(HttpError::Other(format!(
                        "{} does not take MCP over HTTP POST (an older SSE-only server?); try its /mcp address",
                        self.url
                    ))
                    .into());
                }
                let detail: String = body.chars().take(200).collect();
                return Err(
                    HttpError::Other(format!("the server answered {status}: {detail}")).into(),
                );
            }
            if let Some(s) = session {
                self.session = Some(s);
            }
            // A notification is done once accepted (202): no body to wait for.
            let Some(id) = msg.get("id").filter(|i| !i.is_null()) else {
                return Ok(None);
            };
            if status == 202 || status == 204 {
                return Err(HttpError::Other(
                    "the server accepted the request but sent no answer".into(),
                )
                .into());
            }
            if ctype.contains("text/event-stream") {
                let reader = res.body_mut().with_config().limit(REPLY_LIMIT).reader();
                return answer_in_stream(std::io::BufReader::new(reader), id)
                    .map(Some)
                    .map_err(Failure::Http);
            }
            let body = read_text(&mut res, REPLY_LIMIT)?;
            if body.trim().is_empty() {
                return Err(HttpError::Other("the server sent no reply".into()).into());
            }
            let replies = match serde_json::from_str::<Value>(&body) {
                Ok(Value::Array(all)) => all,
                Ok(one) => vec![one],
                Err(e) => {
                    return Err(
                        HttpError::Other(format!("the server's reply was not JSON: {e}")).into(),
                    )
                }
            };
            return replies
                .into_iter()
                .find(|r| r.get("id") == Some(id))
                .map(Some)
                .ok_or_else(|| {
                    HttpError::Other("the server's reply did not answer the request".into()).into()
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

    fn post(
        &self,
        msg: &Value,
        bearer: Option<&str>,
        timeout: Duration,
    ) -> Result<Reply, HttpError> {
        let mut req = self
            .agent
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
        req.config()
            .timeout_global(Some(timeout))
            .build()
            .send(msg.to_string())
            .map_err(|e| HttpError::Other(format!("could not reach {}: {e}", self.url)))
    }
}

impl Drop for HttpConn {
    /// Tells the server this session is over, so it can free it.
    fn drop(&mut self) {
        let Some(session) = self.session.take() else {
            return;
        };
        let (agent, url, name) = (self.agent.clone(), self.url.clone(), self.name.clone());
        let mut headers: Vec<(String, String)> = self
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        headers.push(("Mcp-Session-Id".into(), session));
        if let Some(p) = &self.protocol {
            headers.push(("MCP-Protocol-Version".into(), p.clone()));
        }
        let gh = self.gh_token.clone();
        let fixed = self
            .headers
            .keys()
            .any(|k| k.eq_ignore_ascii_case("authorization"));
        let _ = std::thread::Builder::new()
            .name("mcp-goodbye".into())
            .spawn(move || {
                let bearer = if fixed {
                    None
                } else {
                    gh.or_else(|| crate::mcp_oauth::bearer(&name))
                };
                let mut req = agent.delete(&url);
                for (k, v) in &headers {
                    req = req.header(k.as_str(), v.as_str());
                }
                if let Some(b) = bearer {
                    req = req.header("Authorization", format!("Bearer {b}").as_str());
                }
                let _ = req
                    .config()
                    .timeout_global(Some(GOODBYE_WAIT))
                    .build()
                    .call();
            });
    }
}

fn read_text(res: &mut Reply, limit: u64) -> Result<String, HttpError> {
    res.body_mut()
        .with_config()
        .limit(limit)
        .read_to_string()
        .map_err(|e| HttpError::Other(format!("reading the reply failed: {e}")))
}

/// Reads an event stream until the answer to `id` arrives, then stops: a
/// server may keep the stream open after it.
fn answer_in_stream(mut reader: impl BufRead, id: &Value) -> Result<Value, HttpError> {
    let mut data: Vec<String> = vec![];
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| HttpError::Other(format!("reading the reply failed: {e}")))?;
        let text = line.trim_end_matches(['\n', '\r']);
        if n == 0 || text.is_empty() {
            if !data.is_empty() {
                let event = data.join("\n");
                data.clear();
                if let Some(answer) = answer_in(&event, id) {
                    return Ok(answer);
                }
            }
            if n == 0 {
                return Err(HttpError::Other(
                    "the server's reply did not answer the request".into(),
                ));
            }
            continue;
        }
        if let Some(d) = text.strip_prefix("data:") {
            data.push(d.strip_prefix(' ').unwrap_or(d).to_string());
        }
    }
}

fn answer_in(event: &str, id: &Value) -> Option<Value> {
    let v = serde_json::from_str::<Value>(event).ok()?;
    let all = match v {
        Value::Array(all) => all,
        one => vec![one],
    };
    all.into_iter().find(|m| {
        let answers = m.get("id") == Some(id) && m.get("method").is_none();
        if !answers {
            let what = m
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("a message");
            tracing::debug!("mcp: the server sent {what} on the reply stream");
        }
        answers
    })
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

fn refused(
    static_auth: bool,
    status: u16,
    www_auth: Option<&str>,
    body: &str,
) -> Option<HttpError> {
    let wall = status == 401 || (status == 403 && www_auth.is_some());
    if !wall {
        return None;
    }
    if static_auth || (www_auth.is_none() && body.to_ascii_lowercase().contains("bearer")) {
        return Some(HttpError::Other(bearer_detail(body)));
    }
    None
}

fn bearer_detail(body: &str) -> String {
    let owned = serde_json::from_str::<Value>(body).ok().and_then(|v| {
        v.get("detail")
            .or_else(|| v.get("message"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    });
    let text = owned.unwrap_or_else(|| "the server rejected the bearer token".into());
    text.chars().take(200).collect()
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
    fn a_bearer_rejection_is_not_a_sign_in() {
        let body = r#"{"detail":"MCP bearer required"}"#;
        assert_eq!(
            refused(true, 401, None, body).unwrap().to_string(),
            "MCP bearer required"
        );
        assert_eq!(
            refused(false, 401, None, body).unwrap().to_string(),
            "MCP bearer required"
        );
        assert!(refused(false, 401, Some("Bearer realm=\"x\""), body).is_none());
        assert!(refused(false, 401, None, "no").is_none());
        assert!(refused(false, 200, None, body).is_none());
        assert_eq!(
            bearer_detail("not json"),
            "the server rejected the bearer token"
        );
    }

    #[test]
    fn an_event_stream_yields_its_messages() {
        let body = "event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\r\n\r\nid: 2\ndata: {\"jsonrpc\":\"2.0\",\ndata: \"id\":1,\"result\":{}}\n\n";
        let got = events(body);
        assert_eq!(got.len(), 2);
        assert_eq!(got[1]["id"], 1);
    }

    /// A reader that hands out what it has, then blocks as an open stream would.
    struct OpenStream(std::io::Cursor<Vec<u8>>);

    impl std::io::Read for OpenStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.0.read(buf)? {
                0 => panic!("read past the answer: the stream is still open"),
                n => Ok(n),
            }
        }
    }

    #[test]
    fn the_answer_ends_the_read_even_on_an_open_stream() {
        let stream = "data: {\"jsonrpc\":\"2.0\",\"id\":\"s1\",\"method\":\"ping\"}\n\n\
                      data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n\n";
        let reader = std::io::BufReader::with_capacity(
            8,
            OpenStream(std::io::Cursor::new(stream.as_bytes().to_vec())),
        );
        let got = answer_in_stream(reader, &json!(7)).unwrap();
        assert_eq!(got["result"]["ok"], true);
        let none = answer_in_stream(std::io::BufReader::new(&b"data: {}\n\n"[..]), &json!(7));
        assert!(none.is_err());
    }
}
