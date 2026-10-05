use std::path::{Path, PathBuf};

use crate::process;

const ANTIGRAVITY_VERSION: &str = "1.3.0";
const ANTIGRAVITY_EXE: &str = if cfg!(windows) {
    "agy_acp_server.exe"
} else {
    "agy_acp_server.par"
};
const DONE: &str = "Done. Go back to Parzi.";

pub fn install_script(id: &str) -> Option<String> {
    let win = cfg!(windows);
    Some(match id {
        "claude" if win => "irm https://claude.ai/install.ps1 | iex".into(),
        "claude" => "curl -fsSL https://claude.ai/install.sh | bash".into(),
        "codex" => "npm install -g @openai/codex".into(),
        "opencode" => "npm install -g opencode-ai".into(),
        "grok" if win => "irm https://x.ai/cli/install.ps1 | iex".into(),
        "grok" => "curl -fsSL https://x.ai/cli/install.sh | bash".into(),
        "cursor" if win => "irm 'https://cursor.com/install?win32=true' | iex".into(),
        "cursor" => "curl -fsS https://cursor.com/install | bash".into(),
        "antigravity" => antigravity_install()?,
        _ => return None,
    })
}

pub fn login_script(id: &str, binary: &str) -> Option<String> {
    let (program, args): (&str, &[&str]) = match id {
        "claude" => ("claude", &["auth", "login"]),
        "codex" => ("codex", &["login"]),
        "opencode" => ("opencode", &["auth", "login"]),
        "grok" => ("grok", &["login"]),
        "cursor" => ("cursor-agent", &["login"]),
        _ => return None,
    };
    let chosen = if binary.trim().is_empty() {
        program
    } else {
        binary
    };
    let path = process::resolve(chosen).unwrap_or_else(|| PathBuf::from(chosen));
    Some(format!("{} {}", invoke(&path), args.join(" ")))
}

pub(crate) fn antigravity_program() -> Option<PathBuf> {
    let base = parzi_core::paths::parzi_dir()
        .ok()?
        .join("agents")
        .join("antigravity");
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for entry in std::fs::read_dir(base).ok()?.flatten() {
        let exe = entry.path().join(ANTIGRAVITY_EXE);
        if !exe.is_file() {
            continue;
        }
        let key = version_key(&entry.file_name().to_string_lossy());
        if best.as_ref().is_none_or(|(k, _)| key > *k) {
            best = Some((key, exe));
        }
    }
    best.map(|(_, exe)| exe)
}

pub(crate) fn installed_version(program: &Path) -> Option<String> {
    let name = program.parent()?.file_name()?.to_string_lossy().to_string();
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
        .then_some(name)
}

fn version_key(name: &str) -> Vec<u32> {
    name.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn invoke(program: &Path) -> String {
    let p = program.display().to_string();
    if cfg!(windows) {
        format!("& '{}'", p.replace('\'', "''"))
    } else {
        format!("'{}'", p.replace('\'', r"'\''"))
    }
}

fn antigravity_install() -> Option<String> {
    let (folder, platform) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => ("windows", "windows-arm64"),
        ("windows", _) => ("windows", "windows-x86_64"),
        ("macos", "aarch64") => ("macos", "darwin-arm64"),
        ("macos", _) => ("macos", "darwin-x86_64"),
        _ => return None,
    };
    let url = format!(
        "https://dl.google.com/agy-extensions/releases/{folder}/agy-acp-server-{ANTIGRAVITY_VERSION}-{platform}.zip"
    );
    let dir = parzi_core::paths::parzi_dir()
        .ok()?
        .join("agents")
        .join("antigravity")
        .join(ANTIGRAVITY_VERSION)
        .display()
        .to_string();
    Some(if cfg!(windows) {
        let dir = dir.replace('\'', "''");
        format!(
            "$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue'; \
             $zip = Join-Path $env:TEMP 'antigravity-acp.zip'; \
             Write-Host 'Downloading Google Antigravity (about 120 MB)...'; \
             Invoke-WebRequest -UseBasicParsing -Uri '{url}' -OutFile $zip; \
             Expand-Archive -Force -Path $zip -DestinationPath '{dir}'; \
             Remove-Item $zip; Write-Host '{DONE}'"
        )
    } else {
        let dir = dir.replace('\'', r"'\''");
        format!(
            "curl -fL '{url}' -o /tmp/antigravity-acp.zip && mkdir -p '{dir}' && \
             unzip -o -q /tmp/antigravity-acp.zip -d '{dir}' && \
             chmod +x '{dir}/{ANTIGRAVITY_EXE}' && rm /tmp/antigravity-acp.zip && echo '{DONE}'"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_agent_has_an_official_install() {
        for id in crate::PROVIDERS {
            let script = install_script(id).unwrap_or_else(|| panic!("{id}"));
            assert!(!script.contains('"'), "{id}: {script}");
        }
        assert!(install_script("antigravity")
            .unwrap()
            .contains("dl.google.com/agy-extensions/releases/"));
    }

    #[test]
    fn logins_run_the_agents_own_command_and_antigravity_signs_in_over_acp() {
        assert!(login_script("codex", "").unwrap().ends_with(" login"));
        assert!(login_script("cursor", "").unwrap().ends_with(" login"));
        assert!(login_script("antigravity", "").is_none());
    }

    #[test]
    fn quotes_in_a_path_cannot_end_the_string() {
        let line = invoke(Path::new("C:/it's/agent.cmd"));
        if cfg!(windows) {
            assert_eq!(line, "& 'C:/it''s/agent.cmd'");
        } else {
            assert_eq!(line, r"'C:/it'\''s/agent.cmd'");
        }
    }

    #[test]
    fn the_newest_installed_version_wins() {
        assert!(version_key("1.10.0") > version_key("1.9.2"));
        assert_eq!(
            installed_version(Path::new("/x/agents/antigravity/1.3.0/agy")).as_deref(),
            Some("1.3.0")
        );
        assert_eq!(installed_version(Path::new("/usr/bin/agy")), None);
    }
}
