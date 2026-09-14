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
}
