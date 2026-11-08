use parzi_core::theme::{change, Theme, ThemeChange};

/// Both tests change the one theme file, so they take turns.
fn home() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let turn = TURN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    static HOME: std::sync::Once = std::sync::Once::new();
    HOME.call_once(|| {
        let dir =
            std::env::temp_dir().join(format!("parzi-test-theme-change-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
        parzi_core::paths::ensure_dirs().unwrap();
    });
    turn
}

fn ask(v: serde_json::Value) -> Result<String, String> {
    let c: ThemeChange = serde_json::from_value(v).unwrap();
    change(&c).map_err(|e| e.to_string())
}

#[test]
fn an_agent_can_switch_the_theme_and_set_the_look() {
    let _turn = home();
    let listed = ask(serde_json::json!({})).unwrap();
    assert!(
        listed.starts_with("Themes: ") && listed.contains("graphite"),
        "{listed}"
    );

    let mut t = Theme::load().unwrap();
    t.background.fill = "full".into();
    t.background.cards = "glass".into();
    t.save().unwrap();

    let done = ask(serde_json::json!({"pack": "Light"})).unwrap();
    assert!(done.starts_with("Parzi now has the light theme."), "{done}");
    let t = Theme::load().unwrap();
    assert_eq!(
        (t.background.fill.as_str(), t.background.cards.as_str()),
        ("full", "glass"),
        "a pack keeps the person's wallpaper choices"
    );

    let done = ask(serde_json::json!({"pack": "graphite", "accent": "#5b8def", "cards": "Solid"}))
        .unwrap();
    assert!(
        done.contains("the graphite theme, accent #5B8DEF, solid cards"),
        "{done}"
    );
    let t = Theme::load().unwrap();
    assert_eq!(t.colors.accent, "#5B8DEF");
    assert_eq!(t.background.cards, "solid");
    assert!(ask(serde_json::json!({}))
        .unwrap()
        .contains("graphite (current)"));

    let done = ask(serde_json::json!({"panel": "auto", "wallpaper": "fullscreen"})).unwrap();
    assert!(
        done.contains("automatic panels, the wallpaper as a full window"),
        "{done}"
    );
    assert!(done.contains("There is no wallpaper yet"), "{done}");
}

#[test]
fn a_bad_ask_says_what_would_work_and_changes_nothing() {
    let _turn = home();
    let before = Theme::load().unwrap().colors.accent;
    let err = ask(serde_json::json!({"pack": "neon"})).unwrap_err();
    assert!(
        err.contains("no theme `neon`") && err.contains("graphite"),
        "{err}"
    );
    let err = ask(serde_json::json!({"accent": "blue"})).unwrap_err();
    assert!(err.contains("#rgb or #rrggbb"), "{err}");
    let err = ask(serde_json::json!({"cards": "frosted"})).unwrap_err();
    assert!(err.contains("clear, glass or solid"), "{err}");
    let err = ask(serde_json::json!({"pack": "light", "text": "white"})).unwrap_err();
    assert!(err.contains("text `white`"), "{err}");
    assert!(
        !ask(serde_json::json!({}))
            .unwrap()
            .contains("light (current)"),
        "the pack was not applied"
    );
    assert_eq!(Theme::load().unwrap().colors.accent, before);
}

#[test]
fn dither_strength_and_size_are_set_checked_and_kept_by_a_pack() {
    let _turn = home();
    let done = ask(serde_json::json!({"dither": "40%", "dither_size": "3px"})).unwrap();
    assert!(done.contains("dither at 40%, 3px dither dots"), "{done}");
    let t = Theme::load().unwrap();
    assert!((t.background.dither - 0.4).abs() < 1e-9);
    assert_eq!(t.background.dither_size, 3);
    let css = t.to_css_vars();
    assert!(css.contains("--dither:0.088;--dither-size:3px;"), "{css}");

    // Nothing changes when one value is wrong.
    let err = ask(serde_json::json!({"dither": "10", "dither_size": "9"})).unwrap_err();
    assert!(err.contains("1 to 6"), "{err}");
    let err = ask(serde_json::json!({"dither": "150"})).unwrap_err();
    assert!(err.contains("0 to 100"), "{err}");
    assert_eq!(Theme::load().unwrap().background.dither_size, 3);

    ask(serde_json::json!({"pack": "graphite"})).unwrap();
    let t = Theme::load().unwrap();
    assert_eq!(t.background.dither_size, 3, "a pack keeps the dither");
    assert!(ask(serde_json::json!({"dither": "off"}))
        .unwrap()
        .contains("no dither"));
    assert!(Theme::load()
        .unwrap()
        .to_css_vars()
        .contains("--dither:0.000;"));
}
