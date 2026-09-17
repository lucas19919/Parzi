//! Scoped-file tests: SYSTEM.md chains, rules/ dirs, pack renames and
//! background deletes. They share one hermetic PARZI_HOME per binary because
//! the variable is process-global — per-test overrides race every
//! ambient-home test in the lib binary (each lib test binary is one process).
//! Fixture names are unique per test; tests in this file run in parallel.

use std::path::PathBuf;

use parzi_core::{rules, system, theme, workspace};

/// One temp PARZI_HOME for this binary, set before any path is read.
fn test_home() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("parzi-test-scopes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("PARZI_HOME", &dir);
    dir
}

fn solid_png(path: &std::path::Path, rgb: [u8; 3]) {
    let img = image::RgbImage::from_pixel(16, 16, image::Rgb(rgb));
    img.save(path).unwrap();
}

#[test]
fn chains_stack_user_then_workspace_and_cap_missing() {
    test_home();
    // Nothing on disk: empty chains, no errors.
    assert!(system::global().is_none());
    assert!(system::for_chat("default").is_empty());
    assert!(system::for_role("acme").is_empty());

    std::fs::write(
        parzi_core::paths::parzi_dir().unwrap().join("SYSTEM.md"),
        "Be direct.\n",
    )
    .unwrap();
    // A real hub workspace, so the chat key counts as one.
    workspace::create_for("acme", workspace::Kind::Solo, "tester").unwrap();
    let ws = workspace::dir("acme");
    std::fs::write(ws.join("SYSTEM.md"), "Runner: bun, never npm.\n").unwrap();

    let chat = system::for_chat("acme");
    assert_eq!(chat.len(), 2);
    assert_eq!(chat[0].scope, "user");
    assert!(chat[0].text.contains("Be direct"));
    assert_eq!(chat[1].scope, "acme");
    assert!(chat[1].text.contains("bun"));
    // Inbox and unknown keys get the global file only.
    assert_eq!(system::for_chat("default").len(), 1);
    assert_eq!(system::for_chat("nope").len(), 1);
    // Unsafe names never become paths.
    assert!(system::workspace("../evil").is_none());

    let role = system::for_role("acme");
    assert_eq!(role.len(), 2);
    assert_eq!(role[1].scope, "acme");

    // Long files cap with a note instead of flooding context.
    let big = "x".repeat(system::SYSTEM_CAP + 100);
    std::fs::write(ws.join("SYSTEM.md"), &big).unwrap();
    let capped = system::for_chat("acme");
    assert_eq!(capped.len(), 2);
    assert!(capped[1].text.ends_with("…(truncated)"));
    assert!(capped[1].text.len() < big.len());
    // Blank files count as missing.
    std::fs::write(ws.join("SYSTEM.md"), "   \n").unwrap();
    assert_eq!(system::for_chat("acme").len(), 1);
}

#[test]
fn rules_scope_to_hub_and_legacy_dirs() {
    test_home();
    workspace::create_for("w1", workspace::Kind::Solo, "t").unwrap();
    let dir = workspace::dir("w1").join("rules");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("fe.md"),
        "---\npaths:\n  - \"apps/web/**\"\n---\nWeb rules.\n",
    )
    .unwrap();
    std::fs::write(dir.join("zz.txt"), "not markdown, never read").unwrap();
    let hit = rules::matching("w1", &["apps/web/a.ts"]);
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].name, "fe.md");
    assert!(rules::matching("w1", &["apps/api/a.ts"]).is_empty());
    // Inbox and unknown keys match nothing, never error.
    assert!(rules::matching("default", &["a.ts"]).is_empty());
    assert!(rules::matching("", &["a.ts"]).is_empty());
    assert!(rules::matching("../evil", &["a.ts"]).is_empty());
    // Legacy projects read their own rules/.
    let leg = parzi_core::paths::projects_dir().unwrap().join("oldp");
    std::fs::create_dir_all(leg.join("rules")).unwrap();
    std::fs::write(leg.join("rules").join("r.md"), "Legacy rules.\n").unwrap();
    let leg_hit = rules::matching("oldp", &["whatever.rs"]);
    assert_eq!(leg_hit.len(), 1);
    assert_eq!(leg_hit[0].body, "Legacy rules.");
}

#[test]
fn packs_rename_and_backgrounds_delete() {
    test_home();
    // A pack to rename: seed the theme file directly.
    let dir = theme::themes_dir().unwrap().join("my-look");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("theme.toml"), "parzi = 1\n").unwrap();
    assert!(theme::rename_pack("my-look", "my-look-2").is_ok());
    assert!(!dir.join("theme.toml").exists());
    assert!(theme::themes_dir()
        .unwrap()
        .join("my-look-2")
        .join("theme.toml")
        .exists());
    // Built-ins, missing packs and collisions are refused.
    assert!(theme::rename_pack("midnight", "nope").is_err());
    assert!(theme::rename_pack("ghost", "nope").is_err());
    assert!(theme::rename_pack("my-look-2", "my-look-2").is_err());
    assert!(theme::rename_pack("my-look-2", "../evil").is_err());
    // A background deletes; the live wallpaper falls back to solid.
    let bgs = parzi_core::paths::backgrounds_dir().unwrap();
    std::fs::create_dir_all(&bgs).unwrap();
    std::fs::write(bgs.join("pic.png"), [137u8, 80, 78, 71]).unwrap();
    let mut theme = theme::Theme::load().unwrap_or_default();
    theme.background.image = "backgrounds/pic.png".into();
    theme.save().unwrap();
    let after = theme::delete_background("pic.png").unwrap();
    assert!(after.background.image.is_empty());
    assert!(!bgs.join("pic.png").exists());
    // Missing files are no-ops, bad names are refused, never panics.
    assert!(theme::delete_background("pic.png")
        .unwrap()
        .background
        .image
        .is_empty());
    assert!(theme::delete_background("../evil.png").is_err());
    assert!(theme::delete_background("").is_err());
    // A colliding import keeps both pictures: the newcomer gets a suffix.
    std::fs::write(bgs.join("dusk.png"), [137u8, 80, 78, 71]).unwrap();
    let red = bgs.join("red-src.png");
    solid_png(&red, [220, 30, 30]);
    let bytes = std::fs::read(&red).unwrap();
    let first = theme::import_background("dusk.png", &bytes).unwrap();
    let second = theme::import_background("dusk.png", &bytes).unwrap();
    assert_eq!(first, "dusk-2.png");
    assert_eq!(second, "dusk-3.png");
    assert!(bgs.join("dusk.png").exists());
}
