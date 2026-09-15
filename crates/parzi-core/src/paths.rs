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

pub fn sessions_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("sessions"))
}

pub fn backgrounds_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("backgrounds"))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("logs"))
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
        "sessions",
        "backgrounds",
        "plugins",
        "logs",
        "themes",
        "attachments",
    ] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    seed_default_background(&root)?;
    seed_builtin_packs(&root)?;
    Ok(root)
}

/// Copy the shipped default backgrounds in on first run. Never overwrites
/// user files: anything already in `backgrounds/` is left alone.
fn seed_default_background(root: &std::path::Path) -> Result<()> {
    // (bundled file name, dev-tree source name). Release installers place
    // them beside the executable; both layouts are best-effort seeds.
    for name in [
        "eva-crosses.jpg",
        "code-geass.jpg",
        "death-note.jpg",
        "itachi.jpg",
        "rei-dark.jpg",
        "uchiha.jpg",
    ] {
        let dest = root.join("backgrounds").join(name);
        if dest.exists() {
            continue;
        }
        // Shipped in dev at <repo>/assets/backgrounds/<name>.
        let mut candidates: Vec<PathBuf> = vec![PathBuf::from("assets/backgrounds").join(name)];
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join(name));
            }
        }
        for c in candidates {
            if c.exists() {
                std::fs::copy(&c, &dest)?;
                break;
            }
        }
    }
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
[background]\nimage = \"backgrounds/eva-crosses.jpg\"\ndim = 0.62\nvignette = 0.48\nblur = 0.0\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
        (
            "code-geass",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#08080C\"\nstage = \"#0C0C12\"\naccent = \"#E51616\"\ntext = \"#EDEDF2\"\n\
text_dim = \"#9AA0AE\"\nbar = \"#141419\"\nborder = \"#26262E\"\n\n\
[background]\nimage = \"backgrounds/code-geass.jpg\"\ndim = 0.55\nvignette = 0.50\nblur = 0.0\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
        (
            "death-note",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#14090B\"\nstage = \"#1A0D10\"\naccent = \"#E5484D\"\ntext = \"#F5E9E4\"\n\
text_dim = \"#B59A95\"\nbar = \"#241215\"\nborder = \"#422024\"\n\n\
[background]\nimage = \"backgrounds/death-note.jpg\"\ndim = 0.68\nvignette = 0.50\nblur = 0.0\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
        (
            "itachi",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#070C0E\"\nstage = \"#0B1114\"\naccent = \"#E52A2A\"\ntext = \"#E9EFF2\"\n\
text_dim = \"#8FA0A8\"\nbar = \"#121A1E\"\nborder = \"#24333A\"\n\n\
[background]\nimage = \"backgrounds/itachi.jpg\"\ndim = 0.60\nvignette = 0.55\nblur = 0.0\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
        (
            "rei",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#080A12\"\nstage = \"#0C0F1A\"\naccent = \"#8EA2FF\"\ntext = \"#E8EDF7\"\n\
text_dim = \"#8E9BB5\"\nbar = \"#121828\"\nborder = \"#26304A\"\n\n\
[background]\nimage = \"backgrounds/rei-dark.jpg\"\ndim = 0.50\nvignette = 0.50\nblur = 0.0\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
        (
            "uchiha",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#0A0A0C\"\nstage = \"#0F0F12\"\naccent = \"#E5484D\"\ntext = \"#EDEDF2\"\n\
text_dim = \"#9AA0AE\"\nbar = \"#17171B\"\nborder = \"#2A2A30\"\n\n\
[background]\nimage = \"backgrounds/uchiha.jpg\"\ndim = 0.50\nvignette = 0.45\nblur = 0.0\n\n\
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
