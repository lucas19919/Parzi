use std::path::PathBuf;

use crate::error::{ParziError, Result};

/// All Parzi state lives under `~/.parzi` (or `$PARZI_HOME` when set for
/// hermetic tests). Filesystem is the truth.
pub fn parzi_dir() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("PARZI_HOME") {
        if !home.is_empty() {
            return Ok(PathBuf::from(home));
        }
    }
    dirs::home_dir()
        .map(|h| h.join(".parzi"))
        .ok_or_else(|| ParziError::Config("cannot locate home directory".into()))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("config.toml"))
}

pub fn theme_path() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("theme.toml"))
}

pub fn user_css_path() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("user.css"))
}

pub fn projects_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("projects"))
}

/// Hub workspaces: one small git repo per workspace (PLAN §1.1), each with
/// `workspace.toml` and `projects/<slug>/`.
pub fn workspaces_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("workspaces"))
}

pub fn sessions_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("sessions"))
}

pub fn backgrounds_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("backgrounds"))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("logs"))
}

/// Derived files only: everything here can be deleted and re-rendered
/// (today: the pre-scaled, pre-blurred wallpaper texture).
pub fn cache_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("cache"))
}

pub fn worktrees_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("worktrees"))
}

pub fn project_knowledge_path(project: &str) -> Result<PathBuf> {
    Ok(projects_dir()?.join(project).join("KNOWLEDGE.md"))
}

/// Create the full tree. Idempotent. Also seeds the default background.
pub fn ensure_dirs() -> Result<PathBuf> {
    let root = parzi_dir()?;
    for sub in [
        "projects",
        "workspaces",
        "sessions",
        "backgrounds",
        "plugins",
        "logs",
        "themes",
        "attachments",
        "cache",
    ] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    seed_default_background(&root)?;
    seed_builtin_packs(&root)?;
    Ok(root)
}

/// Copy shipped default backgrounds in on first run. The bundle currently
/// ships no artwork (see THIRD_PARTY_NOTICES): the loop stays so a future
/// default can be added in one place. Never overwrites user files.
fn seed_default_background(_root: &std::path::Path) -> Result<()> {
    Ok(())
}

/// Ship built-in packs (t3code environment-theme discipline: invalid files
/// are skipped, never fatal). Never overwrites user packs.
fn seed_builtin_packs(root: &std::path::Path) -> Result<()> {
    let packs: &[(&str, &str)] = &[
        (
            "eva-crosses",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#0D0708\"\nstage = \"#120B0C\"\naccent = \"#E5484D\"\ntext = \"#F5EDED\"\n\
text_dim = \"#A89A9B\"\nbar = \"#1D1214\"\nborder = \"#33201F\"\n\n\
[background]\nimage = \"\"\ndim = 0.62\nvignette = 0.48\nblur = 0.0\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
        (
            "midnight",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#0A0C12\"\nstage = \"#0E1118\"\naccent = \"#7C8CFF\"\ntext = \"#E8EAF0\"\n\
text_dim = \"#8B93A7\"\nbar = \"#161B26\"\nborder = \"#262D3D\"\n\n\
[background]\nimage = \"\"\ndim = 0.60\nvignette = 0.45\nblur = 0.0\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
        (
            "grey",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#101012\"\nstage = \"#171719\"\naccent = \"#B8BCC8\"\ntext = \"#EDEDEF\"\n\
text_dim = \"#9A9AA2\"\nbar = \"#1F1F23\"\nborder = \"#2E2E35\"\n\n\
[background]\nimage = \"\"\ndim = 0.60\nvignette = 0.40\nblur = 0.0\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
        (
            "light",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#F2F3F5\"\nstage = \"#FFFFFF\"\naccent = \"#4F5EE0\"\ntext = \"#1A1D24\"\n\
text_dim = \"#5B6472\"\nbar = \"#E9EBEF\"\nborder = \"#D5D9E0\"\n\n\
[background]\nimage = \"\"\ndim = 0.50\nvignette = 0.35\nblur = 0.0\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
    ];
    for (name, toml) in packs {
        let dir = root.join("themes").join(name);
        let marker = dir.join("theme.toml");
        if marker.exists() {
            continue;
        }
        std::fs::create_dir_all(&dir)?;
        std::fs::write(marker, toml)?;
    }
    retire_legacy_packs(root);
    Ok(())
}

/// Packs that used to ship but no longer do. Removed only while still
/// untouched (their signature accent line intact, no art, no user.css) so an
/// edited or renamed copy is never taken from the user.
fn retire_legacy_packs(root: &std::path::Path) {
    const LEGACY: &[(&str, &str)] = &[
        ("moody-midnight", "accent = \"#7C8CFF\""),
        ("tokyo-night", "accent = \"#7AA2F7\""),
        ("catppuccin-mocha", "accent = \"#CBA6F7\""),
        ("dracula", "accent = \"#BD93F9\""),
        ("nordic-frost", "accent = \"#88C0D0\""),
        ("oled-black", "accent = \"#10B981\""),
        ("rose-pine", "accent = \"#EB6F92\""),
        ("asuka", "image = \"backgrounds/asuka.png\""),
        ("eva-end", "image = \"backgrounds/eva-end.webp\""),
        ("eva-unit01", "image = \"backgrounds/eva-unit01.webp\""),
        ("shinkai-city", "image = \"backgrounds/shinkai-city.jpg\""),
        ("rei", "image = \"backgrounds/rei.jpg\""),
        ("code-geass", "image = \"backgrounds/code-geass.jpg\""),
        ("death-note", "image = \"backgrounds/death-note.jpg\""),
        ("itachi", "image = \"backgrounds/itachi.jpg\""),
        ("rei", "image = \"backgrounds/rei-dark.jpg\""),
        ("uchiha", "image = \"backgrounds/uchiha.jpg\""),
        ("cyberpunk-noir", "accent = \"#00F0FF\""),
        ("emerald-matrix", "text_dim = \"#6EE7B7\""),
    ];
    for (name, signature) in LEGACY {
        let dir = root.join("themes").join(name);
        let Ok(raw) = std::fs::read_to_string(dir.join("theme.toml")) else {
            continue;
        };
        let extras = std::fs::read_dir(&dir)
            .map(|rd| rd.flatten().count())
            .unwrap_or(0);
        if raw.contains(signature) && extras == 1 {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
