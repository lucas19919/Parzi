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
    for name in ["asuka.png", "eva-crosses.jpg"] {
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
            "moody-midnight",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#07070B\"\nstage = \"#0B0B10\"\naccent = \"#7C8CFF\"\ntext = \"#EDEDF2\"\n\
text_dim = \"#9AA0AE\"\nbar = \"#1A1D24\"\nborder = \"#2A2E3A\"\n\n\
[background]\nimage = \"backgrounds/asuka.png\"\ndim = 0.65\nvignette = 0.50\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
        (
            "eva-crosses",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#0D0708\"\nstage = \"#120B0C\"\naccent = \"#E5484D\"\ntext = \"#F5EDED\"\n\
text_dim = \"#A89A9B\"\nbar = \"#1D1214\"\nborder = \"#33201F\"\n\n\
[background]\nimage = \"backgrounds/eva-crosses.jpg\"\ndim = 0.62\nvignette = 0.48\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
        (
            "tokyo-night",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#16161E\"\nstage = \"#1A1B26\"\naccent = \"#7AA2F7\"\ntext = \"#C0CAF5\"\n\
text_dim = \"#7982A9\"\nbar = \"#24283B\"\nborder = \"#3B4261\"\n\n\
[background]\nimage = \"\"\ndim = 0.60\nvignette = 0.45\n\n\
[glass]\nopacity = 0.88\nradius = 12\nblur_px = 22\nshadow = true\n",
        ),
        (
            "catppuccin-mocha",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#11111B\"\nstage = \"#181825\"\naccent = \"#CBA6F7\"\ntext = \"#CDD6F4\"\n\
text_dim = \"#9399B2\"\nbar = \"#1E1E2E\"\nborder = \"#313244\"\n\n\
[background]\nimage = \"\"\ndim = 0.62\nvignette = 0.45\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n",
        ),
        (
            "dracula",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#1E1F29\"\nstage = \"#282A36\"\naccent = \"#BD93F9\"\ntext = \"#F8F8F2\"\n\
text_dim = \"#989BB0\"\nbar = \"#343746\"\nborder = \"#44475A\"\n\n\
[background]\nimage = \"\"\ndim = 0.60\nvignette = 0.40\n\n\
[glass]\nopacity = 0.86\nradius = 12\nblur_px = 18\nshadow = true\n",
        ),
        (
            "nordic-frost",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#1E222A\"\nstage = \"#242933\"\naccent = \"#88C0D0\"\ntext = \"#ECEFF4\"\n\
text_dim = \"#8FBCBB\"\nbar = \"#2E3440\"\nborder = \"#3B4252\"\n\n\
[background]\nimage = \"\"\ndim = 0.58\nvignette = 0.40\n\n\
[glass]\nopacity = 0.85\nradius = 12\nblur_px = 18\nshadow = true\n",
        ),
        (
            "oled-black",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#000000\"\nstage = \"#030303\"\naccent = \"#10B981\"\ntext = \"#F9FAFB\"\n\
text_dim = \"#9CA3AF\"\nbar = \"#0A0A0A\"\nborder = \"#27272A\"\n\n\
[background]\nimage = \"\"\ndim = 0.80\nvignette = 0.60\n\n\
[glass]\nopacity = 0.92\nradius = 12\nblur_px = 16\nshadow = true\n",
        ),
        (
            "rose-pine",
            "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#12101B\"\nstage = \"#191724\"\naccent = \"#EB6F92\"\ntext = \"#E0DEF4\"\n\
text_dim = \"#908CAA\"\nbar = \"#211F2D\"\nborder = \"#393552\"\n\n\
[background]\nimage = \"\"\ndim = 0.62\nvignette = 0.48\n\n\
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
