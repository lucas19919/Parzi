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

/// Whether this platform's file systems ignore case (by default).
const FOLDS_CASE: bool = cfg!(any(windows, target_os = "macos"));

/// One spelling of a path for comparing two: forward slashes, no `./` in
/// front, no verbatim prefix (`\\?\`), no slash at the end, and case folded
/// where the file system folds it.
#[must_use]
pub fn path_key(raw: &str) -> String {
    let s = raw.replace('\\', "/");
    let s = s
        .strip_prefix("//?/UNC/")
        .map(|r| format!("//{r}"))
        .or_else(|| s.strip_prefix("//?/").map(str::to_string))
        .unwrap_or(s);
    let s = s.trim_start_matches("./").trim_end_matches('/');
    if FOLDS_CASE {
        s.to_lowercase()
    } else {
        s.to_string()
    }
}

/// Whether two paths name the same file, by their `path_key`.
#[must_use]
pub fn same_path(a: &str, b: &str) -> bool {
    path_key(a) == path_key(b)
}

/// `s` without `prefix`, compared char by char and without case where the
/// file system folds it. Byte offsets stay `s`'s own, so the rest keeps its
/// spelling.
#[must_use]
pub fn strip_prefix_fold<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let mut rest = s.char_indices();
    for want in prefix.chars() {
        let (_, got) = rest.next()?;
        let same = want == got || (FOLDS_CASE && want.to_lowercase().eq(got.to_lowercase()));
        if !same {
            return None;
        }
    }
    Some(rest.next().map_or("", |(i, _)| &s[i..]))
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn paths_compare_by_one_spelling() {
        assert!(same_path("./src\\a.rs", "src/a.rs"));
        assert!(same_path("src/", "src"));
        assert_eq!(path_key(r"\\?\C:\x\"), path_key("C:/x"));
        assert_eq!(path_key(r"\\?\UNC\host\s"), "//host/s");
        assert_eq!(same_path("SRC/A.rs", "src/a.rs"), FOLDS_CASE);
        assert_eq!(strip_prefix_fold("src/a.rs", "src"), Some("/a.rs"));
        assert_eq!(strip_prefix_fold("src", "src"), Some(""));
        assert_eq!(strip_prefix_fold("sr", "src"), None);
        assert_eq!(strip_prefix_fold("SRC/x", "src").is_some(), FOLDS_CASE);
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
        "dark",
        "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#000000\"\nstage = \"#000000\"\naccent = \"#E8E8E8\"\ntext = \"#F2F2F2\"\n\
text_dim = \"#8A8A8A\"\nbar = \"#0A0A0A\"\nborder = \"#1F1F1F\"\n\n\
[background]\nimage = \"\"\ndim = 0.60\nvignette = 0.40\nblur = 0.0\n",
    ),
    (
        "graphite",
        "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#0F0F0F\"\nstage = \"#161616\"\naccent = \"#E8E8E8\"\ntext = \"#ECECEC\"\n\
text_dim = \"#9B9B9B\"\nbar = \"#1C1C1C\"\nborder = \"#2A2A2A\"\n\n\
[background]\nimage = \"\"\ndim = 0.66\nvignette = 0.50\nblur = 0.0\n",
    ),
    (
        "light",
        "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#FAFAFA\"\nstage = \"#FFFFFF\"\naccent = \"#000000\"\ntext = \"#171717\"\n\
text_dim = \"#666666\"\nbar = \"#FAFAFA\"\nborder = \"#EAEAEA\"\n\n\
[background]\nimage = \"\"\ndim = 0.40\nvignette = 0.08\nblur = 0.0\ngrain = 0.0\n",
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
const OLD_SEEDS: &[(&str, &str)] = &[
    (
        "light",
        "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#F2F3F5\"\nstage = \"#FFFFFF\"\naccent = \"#4F5EE0\"\ntext = \"#1A1D24\"\n\
text_dim = \"#5B6472\"\nbar = \"#E9EBEF\"\nborder = \"#D5D9E0\"\n\n\
[background]\nimage = \"\"\ndim = 0.50\nvignette = 0.35\nblur = 0.0\n",
    ),
    (
        "light",
        "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#F2F3F5\"\nstage = \"#FFFFFF\"\naccent = \"#4F5EE0\"\ntext = \"#1A1D24\"\n\
text_dim = \"#5B6472\"\nbar = \"#E9EBEF\"\nborder = \"#D5D9E0\"\n\n\
[background]\nimage = \"\"\ndim = 0.40\nvignette = 0.12\nblur = 0.0\n",
    ),
    (
        "light",
        "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#E9EBEF\"\nstage = \"#FFFFFF\"\naccent = \"#2A2E35\"\ntext = \"#16181D\"\n\
text_dim = \"#4B5563\"\nbar = \"#E2E5EA\"\nborder = \"#C9CFD8\"\n\n\
[background]\nimage = \"\"\ndim = 0.40\nvignette = 0.12\nblur = 0.0\n",
    ),
    (
        "light",
        "[font]\nfamily = \"Inter\"\nsize = 14\nmono = \"JetBrains Mono\"\nmono_size = 13\n\n\
[colors]\nsidebar = \"#FAFAFA\"\nstage = \"#FFFFFF\"\naccent = \"#000000\"\ntext = \"#171717\"\n\
text_dim = \"#666666\"\nbar = \"#FAFAFA\"\nborder = \"#EAEAEA\"\n\n\
[background]\nimage = \"\"\ndim = 0.40\nvignette = 0.12\nblur = 0.0\n",
    ),
];

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
        ("tokyo-night", "accent = \"#7AA2F7\""),
        ("slate", "accent = \"#CBD5E1\""),
        ("grey", "accent = \"#E6E8EE\""),
        ("full-dark", "accent = \"#5EEAD4\""),
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
        for (_, old) in OLD_SEEDS {
            std::fs::write(&marker, old).unwrap();
            refresh_builtin_defaults(root.path());
            assert_eq!(std::fs::read_to_string(&marker).unwrap(), current);

            std::fs::write(&marker, format!("{old}{GLASS_TAIL}")).unwrap();
            refresh_builtin_defaults(root.path());
            assert_eq!(std::fs::read_to_string(&marker).unwrap(), current);
        }

        // Same values, one user edit elsewhere: theirs now, left alone.
        let touched = OLD_SEEDS[0].1.replace("#4F5EE0", "#FF0000");
        std::fs::write(&marker, &touched).unwrap();
        refresh_builtin_defaults(root.path());
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), touched);
    }
}
