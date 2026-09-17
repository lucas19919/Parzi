//! Hub workspaces and deck projects against a hermetic `PARZI_HOME`.
//!
//! These were unit tests inside `parzi-core`. They exercise the real path
//! layer, and the lib test binary has no `PARZI_HOME` of its own — so on any
//! machine that ran `cargo test` without setting one, they created and (on a
//! good day) deleted directories inside the developer's actual `~/.parzi`.
//! One interrupted run was enough to leave `t-map-repo-<pid>` sitting in the
//! sidebar of the real app, with no way to tell it from a real workspace.
//!
//! `PARZI_HOME` is process-global, so a per-test override would race every
//! other test in the same binary (the lesson of the `scoped_files` split).
//! One home per binary, set once, with unique names per test instead.

use std::path::{Path, PathBuf};

use parzi_core::project::{self, Roster, Status};
use parzi_core::workspace::{
    self, add_repos, create_for, dir, import_legacy, load, map_repo, remove, save, Kind, RepoRef,
    Role,
};

/// One temp `PARZI_HOME` for this binary, cleared once inside the lock — a
/// clear per test would delete a parallel test's fixtures.
fn test_home() -> PathBuf {
    static HOME: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        // Distinct prefix: `hub_grammar` already claims `parzi-test-hub-*`,
        // and two binaries sharing a name is one pid collision away from one
        // wiping the other's fixtures mid-run.
        let dir = std::env::temp_dir().join(format!("parzi-test-wshub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
        dir
    })
    .clone()
}

/// Unique per test, so nothing collides in the shared home.
fn scratch(tag: &str) -> String {
    test_home();
    format!("t-{tag}")
}

fn cleanup(name: &str) {
    let _ = std::fs::remove_dir_all(dir(name));
}

#[test]
fn the_creator_is_the_owner_and_an_owner_is_required() {
    let name = scratch("create-for");
    cleanup(&name);
    let ws = create_for(&name, Kind::Team, " ada ").expect("create");
    assert_eq!(ws.kind, Kind::Team);
    assert_eq!(ws.role_of("ada"), Some(Role::Owner));
    assert!(ws.repos.is_empty());
    // Twice is a refusal, not a silent overwrite.
    assert!(create_for(&name, Kind::Solo, "ada").is_err());
    assert!(create_for(&scratch("no-owner"), Kind::Solo, "  ").is_err());
    cleanup(&name);
}

#[test]
fn mapping_records_this_machines_path_and_refuses_an_unknown_repo() {
    let name = scratch("map-repo");
    cleanup(&name);
    create_for(&name, Kind::Solo, "ada").expect("create");
    add_repos(
        &name,
        vec![RepoRef {
            name: "shop-api".into(),
            remote: "https://github.com/org/shop-api.git".into(),
            default_branch: "main".into(),
            local_path: None,
        }],
    )
    .expect("add repos");

    let ws = map_repo(&name, "shop-api", Path::new("D:/code/shop-api")).expect("map");
    let want = Some(PathBuf::from("D:/code/shop-api"));
    assert_eq!(ws.repo("shop-api").and_then(|r| r.local_path.clone()), want);
    // From disk, not just from the value we were handed.
    let reread = load(&name).expect("load");
    assert_eq!(
        reread.repo("shop-api").and_then(|r| r.local_path.clone()),
        want
    );
    // A repo the workspace never named is an error, not a no-op.
    assert!(map_repo(&name, "nope", Path::new("D:/code/nope")).is_err());
    cleanup(&name);
}

#[test]
fn removing_a_workspace_deletes_the_tree_and_refuses_default() {
    let name = scratch("remove");
    cleanup(&name);
    create_for(&name, Kind::Solo, "ada").expect("create");
    assert!(dir(&name).is_dir());
    remove(&name).expect("remove");
    assert!(!dir(&name).exists());
    // Idempotent: deleting twice still succeeds.
    remove(&name).expect("remove again");
    assert!(remove("default").is_err());
    assert!(remove("../evil").is_err());
    assert!(remove("").is_err());
}

/// A workspace becomes a git repo the moment anything syncs it, and git marks
/// every file under `.git/objects` read-only. Deleting one still has to work.
#[test]
fn removing_a_synced_workspace_deletes_its_git_repo_too() {
    let name = scratch("remove-git");
    cleanup(&name);
    let ws = create_for(&name, Kind::Solo, "ada").expect("create");
    workspace::sync::sync_now(&ws, "parzi: seed").expect("sync inits the repo");
    assert!(
        dir(&name).join(".git").is_dir(),
        "the repo is there to resist"
    );

    remove(&name).expect("a synced workspace must still delete");
    assert!(!dir(&name).exists());
}

#[test]
fn a_workspace_may_not_be_called_default() {
    // It would shadow the Inbox, and `remove` refuses that name — so it could
    // be created once and then never deleted again.
    test_home();
    assert!(create_for("default", Kind::Solo, "ada").is_err());
    assert!(!dir("default").join("workspace.toml").exists());
}

#[test]
fn workspace_defaults_round_trip_through_toml() {
    let name = scratch("defaults");
    cleanup(&name);
    create_for(&name, Kind::Solo, "ada").expect("create");
    let mut ws = load(&name).expect("load");
    assert!(ws.defaults.model.is_empty() && ws.defaults.effort.is_empty());
    ws.defaults.model = "claude/opus".into();
    ws.defaults.effort = "high".into();
    save(&ws).expect("save");
    let back = load(&name).expect("reload");
    assert_eq!(back.defaults.model, "claude/opus");
    assert_eq!(back.defaults.effort, "high");
    cleanup(&name);
}

#[test]
fn importing_a_legacy_project_moves_the_directory_keeping_the_key() {
    let name = scratch("import-legacy");
    cleanup(&name);
    // A bare legacy project dir is enough: no lanes, no root.
    std::fs::create_dir_all(parzi_core::paths::projects_dir().expect("dirs").join(&name))
        .expect("legacy dir");
    let ws = import_legacy(&name).expect("import");
    assert_eq!(ws.name, name);
    assert!(dir(&name).join("workspace.toml").is_file());
    assert!(!parzi_core::paths::projects_dir()
        .expect("dirs")
        .join(&name)
        .exists());
    // Twice is a refusal (legacy source is gone), default never imports.
    assert!(import_legacy(&name).is_err());
    assert!(import_legacy("default").is_err());
    cleanup(&name);
}

#[test]
fn a_new_project_is_drafting_and_a_slug_is_taken_only_once() {
    let ws = scratch("project-create");
    cleanup(&ws);
    let p = project::create(
        &ws,
        "  Checkout flow  ",
        vec!["shop-api".to_string()],
        Roster::default(),
        Some(40.0),
    )
    .expect("create");
    assert_eq!(p.slug, "checkout-flow");
    assert_eq!(p.title, "Checkout flow");
    assert_eq!(p.status, Status::Drafting);
    assert!(p.why.is_empty() && p.what.is_empty() && p.constraints.is_empty());
    assert_eq!(
        project::load(&ws, "checkout-flow")
            .expect("load")
            .budget_usd,
        Some(40.0)
    );
    // Same slug from a different title is still the same project.
    assert!(project::create(&ws, "Checkout Flow!", vec![], Roster::default(), None).is_err());
    assert!(project::create(&ws, "   ", vec![], Roster::default(), None).is_err());
    // Retitle changes the title, never the slug; remove deletes the tree.
    let p = project::retitle(&ws, "checkout-flow", "  New title  ").expect("retitle");
    assert_eq!(p.title, "New title");
    assert_eq!(p.slug, "checkout-flow");
    assert_eq!(
        project::load(&ws, "checkout-flow").expect("load").title,
        "New title"
    );
    assert!(project::retitle(&ws, "checkout-flow", "   ").is_err());
    project::remove(&ws, "checkout-flow").expect("remove");
    assert!(project::load(&ws, "checkout-flow").is_err());
    // Idempotent: deleting twice still succeeds.
    project::remove(&ws, "checkout-flow").expect("remove again");
    assert!(project::remove(&ws, "../evil").is_err());
    cleanup(&ws);
}
