#[test]
fn budget_round_trips_through_toml() {
    let mut cfg = parzi_core::config::ParziConfig::default();
    cfg.budget.max_cost_usd = Some(2.5);
    cfg.budget.max_tokens = Some(1000);
    let text = toml::to_string(&cfg).expect("serialize");
    let back: parzi_core::config::ParziConfig = toml::from_str(&text).expect("parse");
    assert_eq!(back.budget.max_cost_usd, Some(2.5));
    assert_eq!(back.budget.max_tokens, Some(1000));
}
