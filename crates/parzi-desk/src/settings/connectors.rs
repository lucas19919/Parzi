//! Connectors settings: installed MCP servers and per-tool policy.
//! Saves land in `config.toml` under `mcp.servers` and apply instantly.

use std::collections::HashMap;

use parzi_core::config::{McpServerCfg, ParziConfig};

/// Longest server name accepted (matches the web UI's paste path).
pub const NAME_LIMIT: usize = 48;
/// `timeout_ms` bounds: 1 second to 10 minutes.
pub const TIMEOUT_MIN_MS: u64 = 1_000;
pub const TIMEOUT_MAX_MS: u64 = 600_000;
pub const TOOL_MODES: &[&str] = &["auto", "ask", "deny"];
/// `mode` value that clears the override (the tool follows lane policy).
pub const MODE_INHERIT: &str = "inherit";

#[derive(Debug, Clone, Default)]
pub struct ConnectorsState {
    pub servers: HashMap<String, McpServerCfg>,
    pub status: String,
    pub draft_name: String,
    pub draft_command: String,
    pub draft_args: String,
}

impl ConnectorsState {
    #[must_use]
    pub fn from_config(cfg: &ParziConfig) -> Self {
        Self {
            servers: cfg.mcp.servers.clone(),
            ..Default::default()
        }
    }

    /// Read from `config.toml`; failures fall back to empty.
    #[must_use]
    pub fn load() -> Self {
        Self::from_config(&ParziConfig::load().unwrap_or_default())
    }

    pub fn apply_to(&self, cfg: &mut ParziConfig) {
        cfg.mcp.servers = self.servers.clone();
    }

    /// Every server must be valid; the first failure names its server.
    ///
    /// # Errors
    /// The first invalid server and why.
    pub fn validate(&self) -> Result<(), String> {
        for (name, srv) in &self.servers {
            validate_server(name, srv).map_err(|e| format!("{name}: {e}"))?;
        }
        Ok(())
    }

    /// Validate, merge into the on-disk config and save.
    ///
    /// # Errors
    /// Validation or I/O errors; `status` describes the outcome.
    pub fn save(&mut self) -> Result<(), String> {
        let outcome = self.save_inner();
        self.status = match &outcome {
            Ok(()) => "Saved connectors".to_string(),
            Err(e) => e.clone(),
        };
        outcome
    }

    fn save_inner(&self) -> Result<(), String> {
        self.validate()?;
        let mut cfg = ParziConfig::load().map_err(|e| format!("load config: {e}"))?;
        self.apply_to(&mut cfg);
        cfg.save().map_err(|e| format!("save config: {e}"))?;
        Ok(())
    }

    #[must_use]
    pub fn sorted_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.servers.keys().cloned().collect();
        names.sort();
        names
    }

    /// Insert the draft row as a new server and clear the draft.
    ///
    /// # Errors
    /// Bad names, missing commands, or duplicates.
    pub fn add_draft(&mut self) -> Result<(), String> {
        let name = self.draft_name.trim().to_string();
        validate_name(&name)?;
        if self.draft_command.trim().is_empty() {
            return Err("command is required".to_string());
        }
        if self.servers.contains_key(&name) {
            return Err(format!("`{name}` already exists"));
        }
        let mut srv = McpServerCfg::default();
        srv.command = self.draft_command.trim().to_string();
        srv.args = split_args(&self.draft_args);
        srv.timeout_ms = 30_000;
        srv.enabled = true;
        self.servers.insert(name.clone(), srv);
        self.draft_name.clear();
        self.draft_command.clear();
        self.draft_args.clear();
        self.status = format!("Added {name}");
        Ok(())
    }

    pub fn remove_server(&mut self, name: &str) {
        self.servers.remove(name);
        self.status = format!("Removed {name}");
    }

    /// Flip a server's enabled flag.
    ///
    /// # Errors
    /// Unknown server names.
    pub fn set_enabled(&mut self, name: &str, on: bool) -> Result<(), String> {
        let Some(srv) = self.servers.get_mut(name) else {
            return Err(format!("no server `{name}`"));
        };
        srv.enabled = on;
        self.status = format!("{name} {}", if on { "enabled" } else { "disabled" });
        Ok(())
    }

    /// Exposure toggle (deny-list while allow is empty, else allow-list).
    ///
    /// # Errors
    /// Unknown server names.
    pub fn set_tool_exposed(&mut self, server: &str, tool: &str, on: bool) -> Result<(), String> {
        let Some(srv) = self.servers.get_mut(server) else {
            return Err(format!("no server `{server}`"));
        };
        if on {
            srv.deny.retain(|t| t != tool);
            if !srv.allow.is_empty() && !srv.allow.iter().any(|t| t == tool) {
                srv.allow.push(tool.to_string());
            }
        } else if !srv.allow.is_empty() {
            srv.allow.retain(|t| t != tool);
        } else if !srv.deny.iter().any(|t| t == tool) {
            srv.deny.push(tool.to_string());
        }
        Ok(())
    }

    /// Per-tool override: `auto` | `ask` | `deny`; `inherit` clears it.
    ///
    /// # Errors
    /// Unknown servers or bad modes.
    pub fn set_tool_mode(&mut self, server: &str, tool: &str, mode: &str) -> Result<(), String> {
        if mode != MODE_INHERIT && !TOOL_MODES.contains(&mode) {
            return Err(format!("bad tool mode `{mode}`"));
        }
        let Some(srv) = self.servers.get_mut(server) else {
            return Err(format!("no server `{server}`"));
        };
        if mode == MODE_INHERIT {
            srv.tool_modes.remove(tool);
        } else {
            srv.tool_modes.insert(tool.to_string(), mode.to_string());
        }
        Ok(())
    }
}

/// Letters, numbers, dashes: same gate as the web UI's manual form.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > NAME_LIMIT {
        return Err(format!("name: 1-{NAME_LIMIT} chars"));
    }
    let first = name.chars().next().unwrap_or_default();
    if !first.is_ascii_alphanumeric() {
        return Err("name: must start with a letter or number".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("name: letters, numbers, dashes".to_string());
    }
    Ok(())
}

/// Full per-server validity: name, command, timeout and tool modes.
pub fn validate_server(name: &str, srv: &McpServerCfg) -> Result<(), String> {
    validate_name(name)?;
    if srv.command.trim().is_empty() {
        return Err("command is required".to_string());
    }
    if !(TIMEOUT_MIN_MS..=TIMEOUT_MAX_MS).contains(&srv.timeout_ms) {
        return Err("timeout: 1-600 seconds".to_string());
    }
    for (tool, mode) in &srv.tool_modes {
        if !TOOL_MODES.contains(&mode.as_str()) {
            return Err(format!("tool `{tool}` mode `{mode}` invalid"));
        }
        if tool.trim().is_empty() {
            return Err("tool names must not be blank".to_string());
        }
    }
    Ok(())
}

/// Split an args line on whitespace (same as the web UI's manual form).
#[must_use]
pub fn split_args(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

/// Parse `KEY=value` lines (one per line) for the server editor.
///
/// # Errors
/// The first line without a `KEY=value` shape.
pub fn parse_env_block(text: &str) -> Result<HashMap<String, String>, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (key, value) = trimmed
            .split_once('=')
            .ok_or_else(|| format!("bad env line (need KEY=value): {trimmed}"))?;
        if key.trim().is_empty() {
            return Err(format!("bad env line (need KEY=value): {trimmed}"));
        }
        out.insert(key.trim().to_string(), value.trim().to_string());
    }
    Ok(out)
}

/// Friendly name from a command line (`npx -y @scope/mcp-server-x` -> `x`).
#[must_use]
pub fn sanitize_name(raw: &str) -> String {
    let mut slug = String::new();
    let mut dash = true;
    for c in raw.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            dash = false;
        } else if !dash {
            slug.push('-');
            dash = true;
        }
    }
    slug.trim_matches('-')
        .chars()
        .take(NAME_LIMIT)
        .collect::<String>()
}

#[must_use]
pub fn exposure_summary(srv: &McpServerCfg) -> String {
    if !srv.deny.is_empty() && srv.allow.is_empty() {
        format!("{} hidden", srv.deny.len())
    } else if !srv.allow.is_empty() {
        format!("{} pinned", srv.allow.len())
    } else {
        "all exposed".to_string()
    }
}

pub fn show(ui: &mut egui::Ui, s: &mut ConnectorsState) {
    ui.heading("Connectors");
    ui.label("MCP servers Parzi can spawn. Stored under mcp.servers; saves apply instantly.");
    ui.separator();
    if s.servers.is_empty() {
        ui.label("No servers configured: add one below.");
    }
    for name in s.sorted_names() {
        let Some(srv) = s.servers.get(&name).cloned() else {
            continue;
        };
        let cmd = if srv.args.is_empty() {
            srv.command.clone()
        } else {
            format!("{} {}", srv.command, srv.args.join(" "))
        };
        ui.horizontal(|ui| {
            let mut on = srv.enabled;
            ui.checkbox(&mut on, "");
            if on != srv.enabled {
                let _ = s.set_enabled(&name, on).map_err(|e| s.status = e);
            }
            ui.strong(&name);
            ui.monospace(&cmd);
            let secs = (srv.timeout_ms / 1000).max(1);
            ui.label(format!("{secs}s - {}", exposure_summary(&srv)));
            if ui.small_button("remove").clicked() {
                s.remove_server(&name);
            }
        });
    }
    ui.separator();
    ui.label("Add server");
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut s.draft_name).hint_text("name"));
        ui.add(egui::TextEdit::singleline(&mut s.draft_command).hint_text("command"));
        ui.add(egui::TextEdit::singleline(&mut s.draft_args).hint_text("args"));
        if ui.button("Add").clicked() {
            let _ = s.add_draft().map_err(|e| s.status = e);
        }
    });
    if !s.status.is_empty() {
        ui.label(s.status.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(command: &str) -> McpServerCfg {
        let mut s = McpServerCfg::default();
        s.command = command.into();
        s.timeout_ms = 30_000;
        s
    }

    #[test]
    fn names_gate() {
        assert!(validate_name("filesystem").is_ok());
        assert!(validate_name("a-b_c9").is_ok());
        for bad in ["", "-x", "has space", "semi;colon"] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
        assert!(validate_name(&"x".repeat(NAME_LIMIT + 1)).is_err());
    }

    #[test]
    fn server_validation_rejects() {
        assert!(validate_server("fs", &server("npx")).is_ok());
        assert!(validate_server("fs", &server("")).is_err());
        let mut s = server("npx");
        s.timeout_ms = 0;
        assert!(validate_server("fs", &s).is_err());
        let mut s = server("npx");
        s.tool_modes.insert("read".into(), "maybe".into());
        assert!(validate_server("fs", &s).is_err());
    }

    #[test]
    fn exposure_toggle_mirrors_web_rules() {
        let mut st = ConnectorsState::default();
        st.servers.insert("fs".into(), server("npx"));
        st.set_tool_exposed("fs", "read", false).expect("hide");
        assert_eq!(st.servers["fs"].deny, vec!["read"]);
        st.set_tool_exposed("fs", "read", true).expect("show");
        assert!(st.servers["fs"].deny.is_empty());
        st.servers.get_mut("fs").expect("fs").allow = vec!["read".into(), "write".into()];
        st.set_tool_exposed("fs", "read", false).expect("hide");
        assert_eq!(st.servers["fs"].allow, vec!["write"]);
        assert!(st.set_tool_exposed("nope", "read", true).is_err());
    }

    #[test]
    fn tool_modes_and_env_block() {
        let mut st = ConnectorsState::default();
        st.servers.insert("fs".into(), server("npx"));
        st.set_tool_mode("fs", "write", "ask").expect("ask");
        assert_eq!(st.servers["fs"].tool_modes["write"], "ask");
        st.set_tool_mode("fs", "write", "inherit").expect("inherit");
        assert!(!st.servers["fs"].tool_modes.contains_key("write"));
        assert!(st.set_tool_mode("fs", "write", "maybe").is_err());
        let env = parse_env_block("KEY=value\nEMPTY=\n\nOTHER = spaced ").expect("env");
        assert_eq!(env["KEY"], "value");
        assert_eq!(env["OTHER"], "spaced");
        assert!(parse_env_block("NOEQUALS").is_err());
    }

    #[test]
    fn add_draft_round_trip_and_summary() {
        let mut st = ConnectorsState::default();
        st.draft_name = "fetch".into();
        st.draft_command = "npx".into();
        st.draft_args = "-y serve".into();
        st.add_draft().expect("add");
        assert!(st.draft_name.is_empty());
        assert_eq!(exposure_summary(&st.servers["fetch"]), "all exposed");
        st.validate().expect("valid");
        let mut cfg = ParziConfig::default();
        st.apply_to(&mut cfg);
        let back = ConnectorsState::from_config(&cfg);
        assert!(back.servers.contains_key("fetch"));
        st.draft_name = "fetch".into();
        st.draft_command = "npx".into();
        assert!(st.add_draft().is_err(), "duplicate rejected");
    }

    #[test]
    fn sanitize_and_split() {
        let slug = sanitize_name("MCP Server @scope/filesystem!!");
        assert_eq!(slug, "mcp-server-scope-filesystem");
        assert_eq!(split_args(" -y  @pkg/serve "), vec!["-y", "@pkg/serve"]);
    }
}
