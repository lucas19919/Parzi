use parzi_core::{system, theme};

fn test_home() {
    static HOME: std::sync::Once = std::sync::Once::new();
    HOME.call_once(|| {
        let dir = std::env::temp_dir().join(format!("parzi-test-scopes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
    });
}

fn solid_png(path: &std::path::Path, rgb: [u8; 3]) {
    let img = image::RgbImage::from_pixel(16, 16, image::Rgb(rgb));
    img.save(path).unwrap();
}

#[test]
fn global_system_file_is_capped_and_blank_counts_as_missing() {
    test_home();
    assert!(system::global().is_none());

    let path = parzi_core::paths::parzi_dir().unwrap().join("SYSTEM.md");
    std::fs::write(&path, "Be direct.\n").unwrap();
    assert!(system::global().unwrap().contains("Be direct"));

    let big = "x".repeat(system::SYSTEM_CAP + 100);
    std::fs::write(&path, &big).unwrap();
    let capped = system::global().unwrap();
    assert!(capped.ends_with("…(truncated)"));
    assert!(capped.len() < big.len());

    std::fs::write(&path, "   \n").unwrap();
    assert!(system::global().is_none());
}

#[test]
fn packs_rename_and_backgrounds_delete() {
    test_home();
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
    assert!(theme::rename_pack("midnight", "nope").is_err());
    assert!(theme::rename_pack("ghost", "nope").is_err());
    assert!(theme::rename_pack("my-look-2", "my-look-2").is_err());
    assert!(theme::rename_pack("my-look-2", "../evil").is_err());
    let bgs = parzi_core::paths::backgrounds_dir().unwrap();
    std::fs::create_dir_all(&bgs).unwrap();
    std::fs::write(bgs.join("pic.png"), [137u8, 80, 78, 71]).unwrap();
    let mut theme = theme::Theme::load().unwrap_or_default();
    theme.background.image = "backgrounds/pic.png".into();
    theme.save().unwrap();
    let after = theme::delete_background("pic.png").unwrap();
    assert!(after.background.image.is_empty());
    assert!(!bgs.join("pic.png").exists());
    assert!(theme::delete_background("pic.png")
        .unwrap()
        .background
        .image
        .is_empty());
    assert!(theme::delete_background("../evil.png").is_err());
    assert!(theme::delete_background("").is_err());
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
