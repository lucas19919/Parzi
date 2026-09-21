use parzi_core::context_store::{self, Tier};
use std::path::PathBuf;

fn home() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp home");
    let root = tmp.path().to_path_buf();
    (tmp, root)
}

#[test]
fn empty_workspace_lists_nothing() {
    let (_tmp, home) = home();
    let docs = context_store::list(&home, "acme", None).expect("list");
    assert!(docs.is_empty());
}

#[test]
fn system_md_seeds_pinned() {
    let (_tmp, home) = home();
    let ws = home.join("workspaces").join("acme");
    std::fs::create_dir_all(&ws).expect("ws dir");
    std::fs::write(ws.join("SYSTEM.md"), "# Acme rules\n").expect("seed file");
    let docs = context_store::list(&home, "acme", None).expect("list");
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].name, "system.md");
    assert_eq!(docs[0].tier, Tier::Pinned);
}

#[test]
fn add_pin_remove_round_trip() {
    let (_tmp, home) = home();
    let doc = context_store::add(
        &home,
        "acme",
        None,
        "goals",
        "Ship it.",
        Tier::Curated,
        "test",
    )
    .expect("add");
    assert_eq!(doc.name, "goals.md");
    context_store::set_pinned(&home, "acme", None, "goals.md", true).expect("pin");
    let docs = context_store::list(&home, "acme", None).expect("list");
    assert!(docs
        .iter()
        .any(|d| d.name == "goals.md" && d.tier == Tier::Pinned));
    context_store::remove(&home, "acme", None, "goals.md").expect("remove");
    let docs = context_store::list(&home, "acme", None).expect("list");
    assert!(docs.is_empty());
}

#[test]
fn project_inherits_workspace() {
    let (_tmp, home) = home();
    context_store::add(
        &home,
        "acme",
        None,
        "shared",
        "shared.",
        Tier::Pinned,
        "test",
    )
    .expect("ws add");
    context_store::add(
        &home,
        "acme",
        Some("shop"),
        "plan",
        "sell.",
        Tier::Curated,
        "test",
    )
    .expect("proj add");
    let docs = context_store::list(&home, "acme", Some("shop")).expect("list");
    let names: Vec<&str> = docs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"shared.md"));
    assert!(names.contains(&"plan.md"));
    assert_eq!(docs[0].tier, Tier::Pinned);
}

#[test]
fn identical_add_is_rejected_and_names_are_safe() {
    let (_tmp, home) = home();
    context_store::add(&home, "acme", None, "a", "same", Tier::Curated, "test").expect("first");
    assert!(context_store::add(&home, "acme", None, "a", "same", Tier::Curated, "test").is_err());
    assert!(context_store::add(&home, "acme", None, "..", "x", Tier::Curated, "test").is_err());
    assert!(
        context_store::add(&home, "acme", None, "has space", "x", Tier::Curated, "test").is_err()
    );
    assert!(
        context_store::add(&home, "acme", None, "a", "different", Tier::Curated, "test").is_ok()
    );
}

#[test]
fn injection_orders_pinned_first_within_budget() {
    let (_tmp, home) = home();
    context_store::add(
        &home,
        "acme",
        None,
        "zzz-cur",
        "curated body",
        Tier::Curated,
        "test",
    )
    .expect("cur");
    context_store::add(
        &home,
        "acme",
        None,
        "aaa-pin",
        "pinned body",
        Tier::Pinned,
        "test",
    )
    .expect("pin");
    let text = context_store::injection_text(&home, "acme", None, 10_000);
    let pin = text.find("aaa-pin").expect("pinned present");
    let cur = text.find("zzz-cur").expect("curated present");
    assert!(pin < cur);
    let tiny = context_store::injection_text(&home, "acme", None, 10);
    assert!(tiny.is_empty() || !tiny.contains("pinned body"));
}

#[test]
fn refs_pin_live_plans_and_block_overlap() {
    let (_tmp, home) = home();
    let ws = home.join("workspaces").join("acme");
    std::fs::create_dir_all(ws.join("projects").join("shop")).expect("proj dir");
    std::fs::write(ws.join("projects").join("shop").join("PLAN.md"), "# Shop plan").expect("plan");
    context_store::add_ref(&home, "acme", Some("shop"), "Shop plan", "projects/shop/PLAN.md", "plan").expect("add ref");
    let refs = context_store::list_refs(&home, "acme", Some("shop"));
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].kind, "plan");
    assert!(context_store::add_ref(&home, "acme", Some("shop"), "dup", "projects/shop/PLAN.md", "plan").is_err());
    assert!(context_store::add_ref(&home, "acme", None, "evil", "../x.md", "plan").is_err());
    assert!(context_store::add_ref(&home, "acme", None, "missing", "nope.md", "plan").is_err());
    assert!(context_store::add(&home, "acme", Some("shop"), "copy", "# Shop plan", context_store::Tier::Curated, "test").is_err());
    let text = context_store::injection_text(&home, "acme", Some("shop"), 10_000);
    let plan = text.find("Shop plan").expect("plan injected");
    assert!(text[..plan].contains("# Plan"));
    context_store::remove_ref(&home, "acme", Some("shop"), "projects/shop/PLAN.md").expect("remove");
    assert!(context_store::list_refs(&home, "acme", Some("shop")).is_empty());
}
