//! Plugins v0.1: packs, no code execution.
//! Kinds: `commands` (slash packs), `theme` (theme.toml + backgrounds),
//! `mcp-pack` (pre-wired server configs). Manifest-gated, versioned.

use serde::{Deserialize, Serialize};

use parzi_core::error::{ParziError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    #[serde(default = "d_version")]
    pub version: String,
    /// commands | theme | mcp-pack
    pub kind: String,
    #[serde(default)]
    pub entry: String,
}

fn d_version() -> String {
    "0.1.0".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandPack {
    #[serde(default)]
    pub command: Vec<SlashCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub prompt: String,
}

#[derive(Debug, Clone)]
pub struct Plugin {
    pub manifest: PluginMeta,
    pub dir: std::path::PathBuf,
    pub enabled: bool,
}

fn plugins_dir() -> Result<std::path::PathBuf> {
    Ok(parzi_core::paths::parzi_dir()?.join("plugins"))
}

/// Scan installed plugins. A broken manifest disables that plugin, loudly.
pub fn scan() -> Result<Vec<Plugin>> {
    let root = plugins_dir()?;
    let mut out = vec![];
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for e in entries.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let mf = e.path().join("parzi-plugin.toml");
        if !mf.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&mf)?;
        let manifest: PluginManifest = toml::from_str(&text).map_err(|err| {
            ParziError::Config(format!("plugin {}: {err}", e.file_name().to_string_lossy()))
        })?;
        if !matches!(
            manifest.plugin.kind.as_str(),
            "commands" | "theme" | "mcp-pack"
        ) {
            return Err(ParziError::Config(format!(
                "plugin {}: unknown kind `{}`",
                manifest.plugin.name, manifest.plugin.kind
            )));
        }
        let disabled = e.path().join("disabled").exists();
        out.push(Plugin {
            manifest: manifest.plugin,
            dir: e.path(),
            enabled: !disabled,
        });
    }
    Ok(out)
}

/// All slash commands from enabled `commands` packs.
pub fn slash_commands() -> Result<Vec<SlashCommand>> {
    let mut out = vec![];
    for p in scan()?
        .into_iter()
        .filter(|p| p.enabled && p.manifest.kind == "commands")
    {
        let entry = if p.manifest.entry.is_empty() {
            "commands.toml".into()
        } else {
            p.manifest.entry.clone()
        };
        let text = std::fs::read_to_string(p.dir.join(entry))?;
        let pack: CommandPack = toml::from_str(&text)?;
        out.extend(pack.command);
    }
    Ok(out)
}

/// Toggle without uninstalling. Creates/removes a `disabled` marker file.
pub fn set_enabled(name: &str, enabled: bool) -> Result<()> {
    for p in scan()? {
        if p.manifest.name == name {
            let marker = p.dir.join("disabled");
            if enabled {
                let _ = std::fs::remove_file(marker);
            } else {
                std::fs::write(marker, "")?;
            }
            return Ok(());
        }
    }
    Err(ParziError::Config(format!("plugin not found: {name}")))
}

/// Pack names double as directory names: keep them portable.
pub fn validate_pack_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(ParziError::Config(
            "skill name: letters, numbers, dashes, underscores (max 64)".into(),
        ))
    }
}

fn pack_dir(name: &str) -> Result<std::path::PathBuf> {
    validate_pack_name(name)?;
    Ok(plugins_dir()?.join(name))
}

/// Entry file for a pack, defaulting like the runtime. Rejects traversal so
/// a hand-edited manifest can never escape the pack directory.
fn entry_for(manifest: &PluginMeta) -> Result<String> {
    let entry = if manifest.entry.is_empty() {
        "commands.toml".to_string()
    } else {
        manifest.entry.clone()
    };
    if entry.contains(['/', '\\']) || entry.contains("..") {
        return Err(ParziError::Config(format!(
            "plugin {}: bad entry `{entry}`",
            manifest.name
        )));
    }
    Ok(entry)
}

/// Scaffold a new `commands` skill pack: manifest plus a commented starter.
/// Errors when the name is taken.
pub fn create_commands_pack(name: &str) -> Result<()> {
    let dir = pack_dir(name)?;
    if dir.exists() {
        return Err(ParziError::Config(format!(
            "a skill named `{name}` already exists"
        )));
    }
    std::fs::create_dir_all(&dir)?;
    let manifest = format!(
        "[plugin]\nname = \"{name}\"\nversion = \"0.1.0\"\nkind = \"commands\"\nentry = \"commands.toml\"\n"
    );
    std::fs::write(dir.join("parzi-plugin.toml"), manifest)?;
    let starter = "# One [[command]] block per slash command. Uncomment and adapt:\n# [[command]]\n# name = \"summarize\"\n# description = \"Summarize the current thread\"\n# prompt = \"Summarize this conversation in five bullets.\"\n";
    std::fs::write(dir.join("commands.toml"), starter)?;
    Ok(())
}

/// Slash commands inside one pack. Only `commands` packs qualify; a missing
/// entry file simply means no commands yet.
pub fn commands_for(name: &str) -> Result<Vec<SlashCommand>> {
    let dir = pack_dir(name)?;
    let plugin = scan()?
        .into_iter()
        .find(|p| p.manifest.name == name)
        .map(|p| p.manifest)
        .ok_or_else(|| ParziError::Config(format!("skill not found: {name}")))?;
    if plugin.kind != "commands" {
        return Err(ParziError::Config(format!(
            "`{name}` is a {} pack, not a skill",
            plugin.kind
        )));
    }
    let entry = entry_for(&plugin)?;
    let text = match std::fs::read_to_string(dir.join(&entry)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    Ok(toml::from_str::<CommandPack>(&text)
        .map_err(|e| ParziError::Config(format!("skill {name}: {e}")))?
        .command)
}

fn validate_command(c: &SlashCommand) -> Result<()> {
    let name_ok = !c.name.is_empty()
        && c.name.len() <= 32
        && c.name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_');
    if !name_ok {
        return Err(ParziError::Config(format!(
            "bad command name `{}`: lowercase letters, numbers, dashes",
            c.name
        )));
    }
    if c.description.trim().is_empty() || c.description.len() > 200 {
        return Err(ParziError::Config(format!(
            "command `{}` needs a short description",
            c.name
        )));
    }
    if c.prompt.trim().is_empty() || c.prompt.len() > 8000 {
        return Err(ParziError::Config(format!(
            "command `{}` needs a prompt (max 8k)",
            c.name
        )));
    }
    Ok(())
}

/// Replace a skill's command list wholesale. Validates every row first so a
/// bad save never half-writes the pack.
pub fn save_commands(name: &str, commands: &[SlashCommand]) -> Result<()> {
    if commands.len() > 50 {
        return Err(ParziError::Config("max 50 commands per skill".into()));
    }
    for c in commands {
        validate_command(c)?;
    }
    let dir = pack_dir(name)?;
    let plugin = scan()?
        .into_iter()
        .find(|p| p.manifest.name == name)
        .map(|p| p.manifest)
        .ok_or_else(|| ParziError::Config(format!("skill not found: {name}")))?;
    if plugin.kind != "commands" {
        return Err(ParziError::Config(format!(
            "`{name}` is a {} pack, not a skill",
            plugin.kind
        )));
    }
    let entry = entry_for(&plugin)?;
    let mut out =
        String::from("# Managed by Parzi → Settings → Skills. One block per slash command.\n");
    for c in commands {
        out.push_str("\n[[command]]\n");
        out.push_str(&format!("name = {}\n", toml_value_string(&c.name)));
        out.push_str(&format!(
            "description = {}\n",
            toml_value_string(&c.description)
        ));
        out.push_str(&format!("prompt = {}\n", toml_value_string(&c.prompt)));
    }
    std::fs::write(dir.join(&entry), out)?;
    Ok(())
}

/// Minimal TOML basic-string escape for values we write ourselves.
fn toml_value_string(s: &str) -> String {
    let mut esc = String::with_capacity(s.len() + 2);
    esc.push('"');
    for ch in s.chars() {
        match ch {
            '"' => esc.push_str("\\\""),
            '\\' => esc.push_str("\\\\"),
            '\n' => esc.push_str("\\n"),
            '\r' => esc.push_str("\\r"),
            '\t' => esc.push_str("\\t"),
            c if (c as u32) < 0x20 => esc.push_str(&format!("\\u{:04X}", c as u32)),
            c => esc.push(c),
        }
    }
    esc.push('"');
    esc
}

pub fn themes() -> Result<Vec<(String, std::path::PathBuf)>> {
    Ok(scan()?
        .into_iter()
        .filter(|p| p.enabled && p.manifest.kind == "theme")
        .map(|p| (p.manifest.name.clone(), p.dir.clone()))
        .collect())
}

/// Lowercase, dash-separated version of a free-typed name.
fn sanitize_pack_name(raw: &str) -> Result<String> {
    let cand: String = raw
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cand = cand.trim_matches(|c| c == '-' || c == '_').to_string();
    validate_pack_name(&cand)?;
    Ok(cand)
}

/// Parse a SKILL.md file (Anthropic's skill format) into one slash command:
/// frontmatter `name:`/`description:` plus the markdown body as the prompt.
pub fn parse_skill_md(text: &str, fallback_name: &str) -> Result<SlashCommand> {
    let t = text.trim().replace("\r\n", "\n");
    let t = t.trim();
    let (front, body) = if let Some(rest) = t.strip_prefix("---") {
        match rest.find("\n---") {
            Some(i) => (
                rest[..i].trim().to_string(),
                rest[i + 4..].trim().to_string(),
            ),
            None => (String::new(), t.to_string()),
        }
    } else {
        (String::new(), t.to_string())
    };
    let mut name = String::new();
    let mut description = String::new();
    for line in front.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().trim_matches('"').trim_matches('\'').trim();
            match k.trim().to_lowercase().as_str() {
                "name" => name = v.to_string(),
                "description" => description = v.to_string(),
                _ => {}
            }
        }
    }
    if body.is_empty() {
        return Err(ParziError::Config(
            "that markdown has no body — nothing to run".into(),
        ));
    }
    if description.is_empty() {
        let first = body
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))
            .unwrap_or("Custom skill");
        description = first.chars().take(200).collect();
    }
    let raw_name = if name.is_empty() {
        fallback_name
    } else {
        &name
    };
    let clean: String = raw_name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let clean = clean.trim_matches('-').to_string();
    let cmd = SlashCommand {
        name: clean,
        description,
        prompt: body,
    };
    validate_command(&cmd)?;
    Ok(cmd)
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledSkill {
    pub name: String,
    pub commands: usize,
}

/// Paste-anything installer: a `commands.toml` block or a `SKILL.md` file.
/// Returns the installed pack name and command count.
pub fn install_pasted_skill(pack_name: &str, text: &str) -> Result<InstalledSkill> {
    let t = text.trim();
    if t.is_empty() {
        return Err(ParziError::Config("paste a skill first".into()));
    }
    if t.starts_with('{') || t.contains("[[command]]") {
        let pack: CommandPack = toml::from_str(t)
            .map_err(|e| ParziError::Config(format!("couldn't read that TOML: {e}")))?;
        if pack.command.is_empty() {
            return Err(ParziError::Config("no [[command]] blocks in there".into()));
        }
        for c in &pack.command {
            validate_command(c)?;
        }
        let name = if pack_name.trim().is_empty() {
            pack.command[0].name.clone()
        } else {
            sanitize_pack_name(pack_name.trim())?
        };
        validate_pack_name(&name)?;
        create_commands_pack(&name)?;
        save_commands(&name, &pack.command)?;
        let n = pack.command.len();
        return Ok(InstalledSkill { name, commands: n });
    }
    let hint = if pack_name.trim().is_empty() {
        "skill"
    } else {
        pack_name.trim()
    };
    let cmd = parse_skill_md(t, hint)?;
    let name = sanitize_pack_name(if pack_name.trim().is_empty() {
        &cmd.name
    } else {
        pack_name.trim()
    })?;
    create_commands_pack(&name)?;
    save_commands(&name, std::slice::from_ref(&cmd))?;
    Ok(InstalledSkill { name, commands: 1 })
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillInstallReport {
    pub installed: Vec<String>,
    pub skipped: Vec<String>,
}

/// Accept an https repo URL or an `owner/repo` GitHub shorthand.
pub fn normalize_git_url(input: &str) -> Result<String> {
    let t = input.trim();
    if t.is_empty() {
        return Err(ParziError::Config("paste a GitHub repo URL".into()));
    }
    if t.chars().any(char::is_whitespace) {
        return Err(ParziError::Config("that URL has spaces in it".into()));
    }
    if t.starts_with("git@") || t.starts_with("ssh://") {
        return Err(ParziError::Config(
            "use an https URL (or owner/repo) — ssh needs keys".into(),
        ));
    }
    if let Some(rest) = t.strip_prefix("https://") {
        if rest.is_empty() {
            return Err(ParziError::Config("that URL is empty".into()));
        }
        let mut out = t.trim_end_matches('/').to_string();
        if !out.ends_with(".git") {
            out.push_str(".git");
        }
        return Ok(out);
    }
    let parts: Vec<&str> = t.split('/').collect();
    if parts.len() == 2
        && !parts[0].is_empty()
        && !parts[1].is_empty()
        && parts.iter().all(|p| {
            p.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        })
    {
        return Ok(format!(
            "https://github.com/{}/{}.git",
            parts[0],
            parts[1].trim_end_matches(".git")
        ));
    }
    Err(ParziError::Config(
        "paste an https repo URL or owner/repo".into(),
    ))
}

fn skill_marker(dir: &std::path::Path) -> bool {
    dir.join("parzi-plugin.toml").is_file()
        || dir.join("commands.toml").is_file()
        || dir.join("SKILL.md").is_file()
        || dir.join("skill.md").is_file()
}

/// Find installable skill folders: the root itself, or marked subfolders
/// down to depth 2 (covers `skills/` library layouts). Never descends into
/// a folder that is already a skill.
fn discover_skill_dirs(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    if skill_marker(root) {
        return vec![root.to_path_buf()];
    }
    let mut out = vec![];
    let mut stack = vec![(root.to_path_buf(), 0u8)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 2 {
            continue;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for e in entries.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if skill_marker(&p) {
                out.push(p);
            } else {
                stack.push((p, depth + 1));
            }
        }
    }
    out
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)?.flatten() {
        if e.file_name() == ".git" {
            continue;
        }
        let to = dst.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &to)?;
        } else {
            std::fs::copy(e.path(), &to)?;
        }
    }
    Ok(())
}

/// Install one discovered folder. `Ok(name)` when installed,
/// `Err("name — reason")` when skipped (never fatal: one bad folder
/// must not sink a whole library).
fn install_skill_dir(src: &std::path::Path) -> std::result::Result<String, String> {
    let fallback = src
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".into());
    // Already a Parzi pack → copy as-is (known kinds only).
    if src.join("parzi-plugin.toml").is_file() {
        let text = std::fs::read_to_string(src.join("parzi-plugin.toml"))
            .map_err(|e| format!("{fallback} — can't read manifest: {e}"))?;
        let mf: PluginManifest =
            toml::from_str(&text).map_err(|e| format!("{fallback} — bad manifest: {e}"))?;
        if !matches!(mf.plugin.kind.as_str(), "commands" | "theme" | "mcp-pack") {
            return Err(format!(
                "{fallback} — unsupported kind `{}`",
                mf.plugin.kind
            ));
        }
        validate_pack_name(&mf.plugin.name).map_err(|e| format!("{fallback} — {e}"))?;
        let dst = plugins_dir()
            .map_err(|e| e.to_string())?
            .join(&mf.plugin.name);
        if dst.exists() {
            return Err(format!("{} — already installed", mf.plugin.name));
        }
        copy_dir(src, &dst).map_err(|e| format!("{} — copy failed: {e}", mf.plugin.name))?;
        return Ok(mf.plugin.name);
    }
    // Bare commands.toml → wrap with a manifest, copy everything.
    if src.join("commands.toml").is_file() {
        let text = std::fs::read_to_string(src.join("commands.toml"))
            .map_err(|e| format!("{fallback} — can't read commands.toml: {e}"))?;
        let pack: CommandPack =
            toml::from_str(&text).map_err(|e| format!("{fallback} — bad commands.toml: {e}"))?;
        if pack.command.is_empty() {
            return Err(format!("{fallback} — no [[command]] blocks"));
        }
        for c in &pack.command {
            validate_command(c).map_err(|e| format!("{fallback} — {e}"))?;
        }
        let name = sanitize_pack_name(&fallback).map_err(|e| format!("{fallback} — {e}"))?;
        let dst = plugins_dir().map_err(|e| e.to_string())?.join(&name);
        if dst.exists() {
            return Err(format!("{name} — already installed"));
        }
        copy_dir(src, &dst).map_err(|e| format!("{name} — copy failed: {e}"))?;
        let manifest = format!(
            "[plugin]\nname = \"{name}\"\nversion = \"0.1.0\"\nkind = \"commands\"\nentry = \"commands.toml\"\n"
        );
        std::fs::write(dst.join("parzi-plugin.toml"), manifest)
            .map_err(|e| format!("{name} — can't write manifest: {e}"))?;
        return Ok(name);
    }
    // SKILL.md → convert to one slash command.
    let md = ["SKILL.md", "skill.md"]
        .into_iter()
        .map(|f| src.join(f))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("{fallback} — no skill files found"))?;
    let text = std::fs::read_to_string(&md)
        .map_err(|e| format!("{fallback} — can't read SKILL.md: {e}"))?;
    let cmd = parse_skill_md(&text, &fallback).map_err(|e| format!("{fallback} — {e}"))?;
    let name = sanitize_pack_name(&fallback).map_err(|e| format!("{fallback} — {e}"))?;
    let dst = plugins_dir().map_err(|e| e.to_string())?.join(&name);
    if dst.exists() {
        return Err(format!("{name} — already installed"));
    }
    create_commands_pack(&name).map_err(|e| format!("{name} — {e}"))?;
    save_commands(&name, std::slice::from_ref(&cmd)).map_err(|e| format!("{name} — {e}"))?;
    Ok(name)
}

/// Clone a skill library and install every skill folder inside.
/// Blocking (runs a `git` subprocess) — callers should spawn_blocking.
pub fn install_skill_from_git(url: &str) -> Result<SkillInstallReport> {
    let url = normalize_git_url(url)?;
    let git_ok = std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !git_ok {
        return Err(ParziError::Config(
            "git isn't installed — install git first, then retry".into(),
        ));
    }
    let tmp = std::env::temp_dir().join(format!("parzi-skills-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let clone = std::process::Command::new("git")
        .args(["clone", "--depth", "1", "--single-branch"])
        .arg(&url)
        .arg(&tmp)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| ParziError::Config(format!("couldn't start git: {e}")))?;
    if !clone.status.success() {
        let _ = std::fs::remove_dir_all(&tmp);
        let stderr = String::from_utf8_lossy(&clone.stderr);
        let first = stderr
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("clone failed");
        return Err(ParziError::Config(format!(
            "couldn't clone that repo: {first}"
        )));
    }
    let mut report = SkillInstallReport {
        installed: vec![],
        skipped: vec![],
    };
    for dir in discover_skill_dirs(&tmp) {
        match install_skill_dir(&dir) {
            Ok(name) => report.installed.push(name),
            Err(reason) => report.skipped.push(reason),
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    if report.installed.is_empty() && report.skipped.is_empty() {
        return Err(ParziError::Config(
            "no skills found in that repo (looked for SKILL.md / commands.toml)".into(),
        ));
    }
    report.installed.sort();
    report.skipped.sort();
    Ok(report)
}

/// Remove a skill pack entirely (the folder goes away).
pub fn delete_skill(name: &str) -> Result<()> {
    let dir = pack_dir(name)?;
    let plugin = scan()?
        .into_iter()
        .find(|p| p.manifest.name == name)
        .map(|p| p.manifest)
        .ok_or_else(|| ParziError::Config(format!("skill not found: {name}")))?;
    if plugin.kind != "commands" {
        return Err(ParziError::Config(format!(
            "`{name}` is a {} pack, not a skill",
            plugin.kind
        )));
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-to-end: scaffold a pack, save commands with tricky characters,
    /// read them back, reject a bad row, then clean up after itself.
    #[test]
    fn skill_pack_round_trip() {
        let name = format!("parzi-test-tmp-{}", std::process::id());
        let dir = plugins_dir().unwrap().join(&name);
        let _ = std::fs::remove_dir_all(&dir);
        create_commands_pack(&name).unwrap();
        assert!(commands_for(&name).unwrap().is_empty());
        let cmds = vec![SlashCommand {
            name: "ship-it".into(),
            description: "Ship it".into(),
            prompt: "Do it.\nMultiline \"quoted\" \\ backslash".into(),
        }];
        save_commands(&name, &cmds).unwrap();
        let back = commands_for(&name).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "ship-it");
        assert_eq!(back[0].prompt, cmds[0].prompt);
        assert!(save_commands(
            &name,
            &[SlashCommand {
                name: "Bad Name!".into(),
                description: "x".into(),
                prompt: "y".into(),
            }]
        )
        .is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Paste flows: TOML block, SKILL.md with frontmatter, and bad input.
    #[test]
    fn paste_skill_install() {
        let toml = "[[command]]\nname = \"pastet\"\ndescription = \"d\"\nprompt = \"p\"\n";
        let inst = install_pasted_skill("parzi-paste-tmp", toml).unwrap();
        assert_eq!(inst.name, "parzi-paste-tmp");
        assert_eq!(inst.commands, 1);
        assert_eq!(commands_for(&inst.name).unwrap().len(), 1);
        delete_skill(&inst.name).unwrap();

        let md = "---\nname: md-skill\ndescription: From markdown\n---\n# Do stuff\nBody here.\n";
        let inst = install_pasted_skill("", md).unwrap();
        assert_eq!(inst.name, "md-skill");
        let cmds = commands_for(&inst.name).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].description, "From markdown");
        assert!(cmds[0].prompt.contains("Body here."));
        delete_skill(&inst.name).unwrap();

        assert!(install_pasted_skill("x", "   ").is_err());
        assert!(install_pasted_skill("x", "{\"oops\": true}").is_err());
    }

    /// Library discovery + install from local folders (no network): a
    /// `skills/` layout with a SKILL.md skill and a commands.toml skill.
    #[test]
    fn discover_and_install_library() {
        let root = std::env::temp_dir().join(format!("parzi-lib-tmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = delete_skill("parzi-fix-alpha");
        let _ = delete_skill("parzi-fix-beta");
        let alpha = root.join("skills").join("parzi-fix-alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::write(alpha.join("SKILL.md"), "---\ndescription: A\n---\nDo A.\n").unwrap();
        let beta = root.join("parzi-fix-beta");
        std::fs::create_dir_all(&beta).unwrap();
        std::fs::write(
            beta.join("commands.toml"),
            "[[command]]\nname = \"b\"\ndescription = \"B\"\nprompt = \"do b\"\n",
        )
        .unwrap();
        let dirs = discover_skill_dirs(&root);
        assert_eq!(dirs.len(), 2);
        let mut names = vec![];
        for d in &dirs {
            names.push(install_skill_dir(d).unwrap());
        }
        assert!(names.contains(&"parzi-fix-alpha".to_string()));
        assert!(names.contains(&"parzi-fix-beta".to_string()));
        // Second pass skips everything as already installed.
        for d in &dirs {
            assert!(install_skill_dir(d).is_err());
        }
        for n in &names {
            delete_skill(n).unwrap();
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn git_url_normalize() {
        assert_eq!(
            normalize_git_url("owner/repo").unwrap(),
            "https://github.com/owner/repo.git"
        );
        assert_eq!(
            normalize_git_url("https://github.com/owner/repo/").unwrap(),
            "https://github.com/owner/repo.git"
        );
        assert!(normalize_git_url("git@github.com:o/r.git").is_err());
        assert!(normalize_git_url("not a url").is_err());
    }
}
