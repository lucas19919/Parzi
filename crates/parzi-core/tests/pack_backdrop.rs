use parzi_core::theme::{self, Theme};

#[test]
fn a_theme_keeps_your_wallpaper_position_unless_it_sets_its_own() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("PARZI_HOME", dir.path());
    let root = parzi_core::paths::ensure_dirs().unwrap();

    let mut mine = Theme::default();
    mine.background.height = 0.45;
    mine.background.y = 0.2;
    mine.save().unwrap();

    let standard = theme::apply_pack("graphite").unwrap();
    assert_eq!(standard.background.height, 0.45);
    assert_eq!(standard.background.y, 0.2);

    let custom = root.join("themes").join("custom");
    std::fs::create_dir_all(&custom).unwrap();
    std::fs::write(
        custom.join("theme.toml"),
        "[background]\nimage = \"\"\nheight = 0.25\n",
    )
    .unwrap();
    let own = theme::apply_pack("custom").unwrap();
    assert_eq!(own.background.height, 0.25, "this pack sets its own height");
    assert_eq!(own.background.y, 0.2, "and leaves the position alone");
}
