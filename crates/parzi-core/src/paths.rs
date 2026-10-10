use std::path::PathBuf;

use crate::error::{ParziError, Result};

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

/// A UNC share or a device namespace (`\\host\x`, `//host/x`, `\\?\`,
/// `\\.\`, `\??\`). Touching one makes Windows connect out and offer the
/// user's login hash, so such paths are refused before any file call.
#[must_use]
pub fn is_network_path(raw: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    let t = raw.trim_start();
    let mut chars = t.chars();
    let sep = |c: Option<char>| matches!(c, Some('\\' | '/'));
    (sep(chars.next()) && sep(chars.next())) || t.starts_with("\\??\\")
}

#[cfg(all(test, windows))]
mod network_tests {
    #[test]
    fn shares_and_device_paths_are_network_paths() {
        for p in [
            r"\\host\s\a.png",
            "//host/s",
            r"\\?\C:\x",
            r"\\.\pipe\x",
            r"\??\C:\x",
            r"\/host",
        ] {
            assert!(super::is_network_path(p), "{p}");
        }
        for p in [r"C:\Users\a.png", "notes/a.md", r"\single", ""] {
            assert!(!super::is_network_path(p), "{p}");
        }
    }
}

pub fn config_path() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("config.toml"))
}

pub(crate) fn theme_path() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("theme.toml"))
}

pub(crate) fn user_css_path() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("user.css"))
}

pub fn scratch_dir(session_id: &str) -> Result<PathBuf> {
    Ok(parzi_dir()?.join("scratch").join(session_id))
}

pub fn sessions_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("sessions"))
}

pub fn backgrounds_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("backgrounds"))
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("cache"))
}

pub fn brain_dir() -> Result<PathBuf> {
    Ok(parzi_dir()?.join("brain"))
}

pub fn ensure_dirs() -> Result<PathBuf> {
    let root = parzi_dir()?;
    for sub in [
        "sessions",
        "backgrounds",
        "logs",
        "themes",
        "attachments",
        "cache",
    ] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    seed_builtin_packs(&root)?;
    Ok(root)
}

/// Built-in packs as (name, theme.toml). The single list: seeding, the
/// builtin flag and the rename/delete/save guards all read it.
pub(crate) const BUILTIN_PACKS: &[(&str, &str)] = &[
    (
        "tokyo-night",
        "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#16161E\"\nstage = \"#1A1B26\"\naccent = \"#7AA2F7\"\ntext = \"#C0CAF5\"\n\
text_dim = \"#8B93B8\"\nbar = \"#1F2330\"\nborder = \"#2A2E3F\"\n\n\
[background]\nimage = \"\"\ndim = 0.60\nvignette = 0.45\nblur = 0.0\n",
    ),
    (
        "full-dark",
        "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#000000\"\nstage = \"#000000\"\naccent = \"#5EEAD4\"\ntext = \"#F2F2F2\"\n\
text_dim = \"#8A8A8A\"\nbar = \"#0A0A0A\"\nborder = \"#1F1F1F\"\n\n\
[background]\nimage = \"\"\ndim = 0.60\nvignette = 0.40\nblur = 0.0\n",
    ),
    (
        "grey",
        "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#101012\"\nstage = \"#171719\"\naccent = \"#E6E8EE\"\ntext = \"#EDEDEF\"\n\
text_dim = \"#9A9AA2\"\nbar = \"#1F1F23\"\nborder = \"#2E2E35\"\n\n\
[background]\nimage = \"\"\ndim = 0.60\nvignette = 0.40\nblur = 0.0\n",
    ),
    (
        "light",
        "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#F2F3F5\"\nstage = \"#FFFFFF\"\naccent = \"#4F5EE0\"\ntext = \"#1A1D24\"\n\
text_dim = \"#5B6472\"\nbar = \"#E9EBEF\"\nborder = \"#D5D9E0\"\n\n\
[background]\nimage = \"\"\ndim = 0.40\nvignette = 0.12\nblur = 0.0\n",
    ),
];

fn seed_builtin_packs(root: &std::path::Path) -> Result<()> {
    // Retire first so removed packs reseed fresh below instead of
    // vanishing for a launch.
    retire_legacy_packs(root);
    for (name, toml) in BUILTIN_PACKS {
        let dir = root.join("themes").join(name);
        let marker = dir.join("theme.toml");
        if marker.exists() {
            continue;
        }
        std::fs::create_dir_all(&dir)?;
        std::fs::write(marker, toml)?;
    }
    refresh_builtin_defaults(root);
    Ok(())
}

/// Earlier seeds of a builtin, matched as whole files.
const OLD_SEEDS: &[(&str, &str)] = &[(
    "light",
    "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#F2F3F5\"\nstage = \"#FFFFFF\"\naccent = \"#4F5EE0\"\ntext = \"#1A1D24\"\n\
text_dim = \"#5B6472\"\nbar = \"#E9EBEF\"\nborder = \"#D5D9E0\"\n\n\
[background]\nimage = \"\"\ndim = 0.50\nvignette = 0.35\nblur = 0.0\n",
)];

/// Seeds from before the glass section was dropped ended with this.
const GLASS_TAIL: &str = "\n[glass]\nopacity = 0.85\nradius = 12\nblur_px = 20\nshadow = true\n";

/// Builtins are defaults: when a seeded pack still matches an earlier
/// seed byte-for-byte, move it to the current seed so refreshed defaults
/// (like light's gentler veil) reach existing installs. Anything the
/// user touched is left alone.
fn refresh_builtin_defaults(root: &std::path::Path) {
    for (name, old) in OLD_SEEDS {
        let Some((_, seed)) = BUILTIN_PACKS.iter().find(|(n, _)| n == name) else {
            continue;
        };
        let marker = root.join("themes").join(name).join("theme.toml");
        let Ok(raw) = std::fs::read_to_string(&marker) else {
            continue;
        };
        if raw == *old || raw.strip_suffix(GLASS_TAIL) == Some(old) {
            let _ = crate::atomic_write(&marker, seed.as_bytes());
        }
    }
}

fn retire_legacy_packs(root: &std::path::Path) {
    const LEGACY: &[(&str, &str)] = &[
        ("ember", "accent = \"#E5484D\""),
        ("midnight", "accent = \"#7C8CFF\""),
        ("forest", "accent = \"#34D399\""),
        ("grape", "accent = \"#A78BFA\""),
        ("sand", "accent = \"#E8B64C\""),
        ("eva-crosses", "accent = \"#E5484D\""),
        ("moody-midnight", "accent = \"#7C8CFF\""),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_untouched_old_seeds_are_refreshed() {
        let root = tempfile::tempdir().unwrap();
        let light = root.path().join("themes").join("light");
        std::fs::create_dir_all(&light).unwrap();
        let marker = light.join("theme.toml");
        let current = BUILTIN_PACKS.iter().find(|(n, _)| *n == "light").unwrap().1;
        let old = OLD_SEEDS[0].1;

        std::fs::write(&marker, old).unwrap();
        refresh_builtin_defaults(root.path());
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), current);

        std::fs::write(&marker, format!("{old}{GLASS_TAIL}")).unwrap();
        refresh_builtin_defaults(root.path());
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), current);

        // Same values, one user edit elsewhere: theirs now, left alone.
        let touched = old.replace("#4F5EE0", "#FF0000");
        std::fs::write(&marker, &touched).unwrap();
        refresh_builtin_defaults(root.path());
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), touched);
    }
}
