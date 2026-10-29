
use serde::{Deserialize, Serialize};
use serde_json::Value;

const REGISTRY: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryAgent {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

pub fn registry_url() -> &'static str {
    REGISTRY
}

pub fn parse_registry(text: &str) -> Result<Vec<RegistryAgent>, String> {
    parse_registry_for(text, platform_key())
}

pub fn parse_registry_for(text: &str, platform: &str) -> Result<Vec<RegistryAgent>, String> {
    let root: Value =
        serde_json::from_str(text).map_err(|e| format!("registry is not JSON: {e}"))?;
    let Some(agents) = root.get("agents").and_then(Value::as_array) else {
        return Err("registry has no agents".into());
    };
    let mut out = Vec::new();
    for agent in agents {
        if let Some(row) = one(agent, platform) {
            out.push(row);
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    Ok(out)
}

fn platform_key() -> &'static str {
    if cfg!(windows) {
        if cfg!(target_arch = "aarch64") {
            "windows-aarch64"
        } else {
            "windows-x86_64"
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "darwin-aarch64"
        } else {
            "darwin-x86_64"
        }
    } else if cfg!(target_arch = "aarch64") {
        "linux-aarch64"
    } else {
        "linux-x86_64"
    }
}

fn one(agent: &Value, platform: &str) -> Option<RegistryAgent> {
    let id = agent.get("id")?.as_str()?.to_string();
    let name = agent
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_string();
    let dist = agent.get("distribution")?;
    if let Some(npx) = dist.get("npx") {
        let package = npx.get("package")?.as_str()?;
        return Some(RegistryAgent {
            id,
            name,
            command: npm_bin(package),
            args: strings(npx.get("args")),
        });
    }
    let binary = dist.get("binary")?;
    let plat = binary
        .get(platform)
        .or_else(|| binary.get("windows-x86_64"))?;
    let cmd = plat.get("cmd")?.as_str()?;
    let command = std::path::Path::new(cmd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(cmd)
        .trim_start_matches("./")
        .to_string();
    if command.is_empty() {
        return None;
    }
    Some(RegistryAgent {
        id,
        name,
        command,
        args: strings(plat.get("args")),
    })
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn npm_bin(spec: &str) -> String {
    match package_name(spec) {
        "@google/gemini-cli" => "gemini".into(),
        "@github/copilot" => "copilot".into(),
        other => other.rsplit('/').next().unwrap_or(other).to_string(),
    }
}

fn package_name(spec: &str) -> &str {
    if let Some(rest) = spec.strip_prefix('@') {
        match rest.rfind('@') {
            Some(i) => &spec[..i + 1],
            None => spec,
        }
    } else {
        spec.split('@').next().unwrap_or(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_and_copilot_are_their_commands() {
        let text = r#"{
            "agents": [
                {"id": "gemini", "name": "Gemini CLI", "distribution": {"npx": {"package": "@google/gemini-cli@0.66.0", "args": ["--acp"]}}},
                {"id": "github-copilot-cli", "name": "Copilot", "distribution": {"npx": {"package": "@github/copilot@1.0.105", "args": ["--acp"]}}},
                {"id": "pi-acp", "name": "pi ACP", "distribution": {"npx": {"package": "pi-acp@0.0.34"}}},
                {"id": "goose", "name": "goose", "distribution": {"binary": {"windows-x86_64": {"cmd": "./goose.exe", "args": ["acp"]}}}}
            ]
        }"#;
        let rows = parse_registry_for(text, "windows-x86_64").unwrap();
        let gemini = rows.iter().find(|r| r.id == "gemini").unwrap();
        assert_eq!(gemini.command, "gemini");
        assert_eq!(gemini.args, vec!["--acp"]);
        let copilot = rows.iter().find(|r| r.id == "github-copilot-cli").unwrap();
        assert_eq!(copilot.command, "copilot");
        assert_eq!(copilot.args, vec!["--acp"]);
        let pi = rows.iter().find(|r| r.id == "pi-acp").unwrap();
        assert_eq!(pi.command, "pi-acp");
        let goose = rows.iter().find(|r| r.id == "goose").unwrap();
        assert_eq!(goose.command, "goose.exe");
        assert_eq!(goose.args, vec!["acp"]);
    }

    #[test]
    fn a_registry_without_agents_is_an_error() {
        assert!(parse_registry_for("{}", "windows-x86_64").is_err());
        assert!(parse_registry_for("not json", "windows-x86_64").is_err());
    }
}
