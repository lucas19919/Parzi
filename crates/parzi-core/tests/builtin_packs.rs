use parzi_core::theme;

// Own process, own home: this binary holds a single test, so no other
// test can race the directories it asserts on.
fn fresh_home() {
    let dir = std::env::temp_dir().join(format!("parzi-test-packs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("PARZI_HOME", &dir);
}

#[test]
fn builtin_packs_parse_and_ship_no_wallpaper() {
    fresh_home();
    parzi_core::paths::ensure_dirs().unwrap();
    let packs = theme::list_pack_infos().unwrap();
    assert_eq!(
        packs.iter().filter(|p| p.builtin).count(),
        4,
        "builtin set changed; update this list: {:?}",
        packs.iter().map(|p| &p.name).collect::<Vec<_>>()
    );
    for want in ["tokyo-night", "full-dark", "grey", "light"] {
        let p = packs
            .iter()
            .find(|p| p.name == want)
            .unwrap_or_else(|| panic!("missing builtin pack {want}"));
        assert!(p.builtin, "{want} must stay builtin");
        assert!(!p.has_art, "{want} must not bundle a wallpaper");
    }
    // No pre-made backgrounds ride along: the folder starts empty and
    // users upload their own.
    let bgs = parzi_core::paths::backgrounds_dir().unwrap();
    let n = std::fs::read_dir(&bgs).map(|rd| rd.count()).unwrap_or(0);
    assert_eq!(n, 0, "backgrounds/ must start empty");
}
