use parzi_core::theme;

// Own process, own home (see builtin_packs.rs).
fn fresh_home() {
    let dir = std::env::temp_dir().join(format!("parzi-test-pack-art-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("PARZI_HOME", &dir);
}

#[test]
fn applying_a_saved_pack_reuses_its_wallpaper_instead_of_copying_it() {
    fresh_home();
    parzi_core::paths::ensure_dirs().unwrap();
    let bgs = parzi_core::paths::backgrounds_dir().unwrap();
    std::fs::create_dir_all(&bgs).unwrap();
    image::RgbImage::from_pixel(4, 4, image::Rgb([20, 40, 60]))
        .save(bgs.join("wall.png"))
        .unwrap();
    theme::set_background("wall.png").unwrap();

    theme::save_pack("vinland").unwrap();
    let applied = theme::apply_pack("vinland").unwrap();

    assert_eq!(applied.background.image, "backgrounds/wall.png");
    assert_eq!(
        theme::list_backgrounds().unwrap(),
        vec!["wall.png".to_string()]
    );
}
