//! The round-1 syncer (hub PLAN.md §1.1): two machines share a workspace
//! through a git remote, and a plan conflict is reported, never merged.
//!
//! The tests drive the directory-level API against temp clones of a bare temp
//! repo, so nothing here touches the network; the one test of the
//! workspace-level API points `$PARZI_HOME` at a temp dir.

use std::path::{Path, PathBuf};

use parzi_core::workspace::sync::{self, SyncReport};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("has a parent")).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

fn workspace_toml(name: &str) -> String {
    format!("name = \"{name}\"\nkind = \"team\"\n")
}

/// A bare "remote", machine A with the workspace pushed, and machine B cloned
/// from it. Returned tempdir keeps them alive.
fn two_machines() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(tmp.path(), &["init", "--bare", &bare.to_string_lossy()]);

    let a = tmp.path().join("a");
    write(&a, "workspace.toml", &workspace_toml("acme"));
    sync::init_at(&a, Some(&bare.to_string_lossy())).unwrap();
    let pushed = sync::commit_and_push_at(&a, "parzi: workspace created").unwrap();
    assert!(pushed.pushed, "the first push seeds the remote");
    // A hosted remote points its HEAD at the default branch; a bare `git init`
    // leaves it on whatever this git's `init.defaultBranch` says, which would
    // make the clone below check out nothing.
    git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let b = tmp.path().join("b");
    git(
        tmp.path(),
        &["clone", &bare.to_string_lossy(), &b.to_string_lossy()],
    );
    (tmp, a, b)
}

#[test]
fn init_creates_the_repo_and_commits_the_workspace_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("ws");
    write(&dir, "workspace.toml", &workspace_toml("solo"));

    let report = sync::init_at(&dir, None).unwrap();
    assert_eq!(
        report,
        SyncReport {
            committed: true,
            ..SyncReport::default()
        }
    );
    assert!(dir.join(".git").exists());
    assert_eq!(git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert!(git(&dir, &["show", "--name-only", "--format=", "HEAD"]).contains("workspace.toml"));

    // Idempotent: a second init keeps the history and commits nothing new.
    let head = git(&dir, &["rev-parse", "HEAD"]);
    let again = sync::init_at(&dir, None).unwrap();
    assert!(!again.committed);
    assert_eq!(git(&dir, &["rev-parse", "HEAD"]), head);
}

#[test]
fn a_solo_workspace_commits_locally_and_never_pushes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("ws");
    write(&dir, "workspace.toml", &workspace_toml("solo"));
    sync::init_at(&dir, None).unwrap();

    write(
        &dir,
        "projects/checkout/PROJECT.md",
        "parzi: 1\n# Project\n",
    );
    let report = sync::sync_now_at(&dir, "parzi: project created").unwrap();
    assert_eq!(
        report,
        SyncReport {
            committed: true,
            pushed: false,
            pulled: false,
            conflicts: vec![],
        },
        "no remote is not an error"
    );
    assert_eq!(git(&dir, &["status", "--porcelain=v1"]), "");

    // Nothing changed since: nothing to commit, still no error.
    assert_eq!(
        sync::sync_now_at(&dir, "parzi: sync").unwrap(),
        SyncReport::default()
    );
}

#[test]
fn a_plan_written_on_one_machine_arrives_on_the_other() {
    let (_tmp, a, b) = two_machines();

    write(
        &a,
        "projects/checkout/PLAN.md",
        "parzi: 1\n## Sprint 1 — API surface\n### lane api\n- [ ] TSK-7 endpoint\n",
    );
    let sent = sync::commit_and_push_at(&a, "parzi: plan drafted").unwrap();
    assert!(sent.committed && sent.pushed && sent.conflicts.is_empty());

    let got = sync::pull_at(&b).unwrap();
    assert!(got.pulled, "B pulled A's commit");
    assert!(got.conflicts.is_empty());
    assert!(read(&b, "projects/checkout/PLAN.md").contains("TSK-7"));

    // Pulling again is a no-op, not a merge commit.
    let head = git(&b, &["rev-parse", "HEAD"]);
    assert_eq!(sync::pull_at(&b).unwrap(), SyncReport::default());
    assert_eq!(git(&b, &["rev-parse", "HEAD"]), head);
}

#[test]
fn concurrent_project_edits_are_reported_not_merged() {
    let (_tmp, a, b) = two_machines();
    let rel = "projects/checkout/PROJECT.md";

    write(&a, rel, "parzi: 1\n# Project: Checkout\nstatus: drafting\n");
    sync::commit_and_push_at(&a, "parzi: project created").unwrap();
    sync::pull_at(&b).unwrap();

    // Both edit the same line while neither has the other's commit.
    write(&a, rel, "parzi: 1\n# Project: Checkout\nstatus: planned\n");
    sync::commit_and_push_at(&a, "parzi: planned").unwrap();
    write(&b, rel, "parzi: 1\n# Project: Checkout\nstatus: parked\n");

    let report = sync::commit_and_push_at(&b, "parzi: parked").unwrap();
    assert_eq!(
        report.conflicts,
        vec![rel.to_string()],
        "the conflict is named, not swallowed"
    );
    assert!(!report.pushed, "nothing half-merged reaches the remote");

    let text = read(&b, rel);
    assert!(
        text.contains("<<<<<<<") && text.contains("planned") && text.contains("parked"),
        "both versions are left in the file for a human: {text}"
    );

    // While the markers are there, B commits nothing at all.
    let head = git(&b, &["rev-parse", "HEAD"]);
    let blocked = sync::commit_and_push_at(&b, "parzi: try again").unwrap();
    assert_eq!(blocked.conflicts, vec![rel.to_string()]);
    assert!(!blocked.committed && !blocked.pushed);
    assert_eq!(git(&b, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        sync::conflicts_at(&b).unwrap(),
        vec![rel.to_string()],
        "the deck can ask for the same list"
    );

    // A is untouched by B's trouble.
    sync::pull_at(&a).unwrap();
    assert!(read(&a, rel).contains("status: planned"));
    assert!(!read(&a, rel).contains("<<<<<<<"));
}

#[test]
fn a_resolved_conflict_syncs_again() {
    let (_tmp, a, b) = two_machines();
    let rel = "projects/checkout/PROJECT.md";

    write(&a, rel, "parzi: 1\nstatus: drafting\n");
    sync::commit_and_push_at(&a, "parzi: project created").unwrap();
    sync::pull_at(&b).unwrap();
    write(&a, rel, "parzi: 1\nstatus: planned\n");
    sync::commit_and_push_at(&a, "parzi: planned").unwrap();
    write(&b, rel, "parzi: 1\nstatus: parked\n");
    assert!(!sync::commit_and_push_at(&b, "parzi: parked")
        .unwrap()
        .conflicts
        .is_empty());

    // A human picks a side and removes the markers.
    write(&b, rel, "parzi: 1\nstatus: planned\n");
    let done = sync::commit_and_push_at(&b, "parzi: conflict resolved").unwrap();
    assert!(done.committed && done.pushed && done.conflicts.is_empty());

    assert!(sync::pull_at(&a).unwrap().pulled);
    assert!(read(&a, rel).contains("status: planned"));
    assert_eq!(sync::conflicts_at(&a).unwrap(), Vec::<String>::new());
}

/// The same resolution through `sync_now`, which pulls before it commits: an
/// open merge must not start a second one.
#[test]
fn a_full_sync_closes_an_open_merge_before_it_pulls_again() {
    let (_tmp, a, b) = two_machines();
    let rel = "projects/checkout/PLAN.md";

    write(&a, rel, "parzi: 1\n- [ ] TSK-7\n");
    sync::commit_and_push_at(&a, "parzi: plan drafted").unwrap();
    sync::pull_at(&b).unwrap();
    write(&a, rel, "parzi: 1\n- [x] TSK-7\n");
    sync::commit_and_push_at(&a, "parzi: TSK-7 done").unwrap();
    write(&b, rel, "parzi: 1\n- [ ] TSK-7 renamed\n");
    assert!(!sync::sync_now_at(&b, "parzi: renamed")
        .unwrap()
        .conflicts
        .is_empty());

    write(&b, rel, "parzi: 1\n- [x] TSK-7 renamed\n");
    let done = sync::sync_now_at(&b, "parzi: conflict resolved").unwrap();
    assert!(done.committed && done.pushed, "{done:?}");
    assert_eq!(git(&b, &["status", "--porcelain=v1"]), "");

    assert!(sync::pull_at(&a).unwrap().pulled);
    assert!(read(&a, rel).contains("TSK-7 renamed"));
}

#[test]
fn markers_written_by_hand_block_the_commit_too() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("ws");
    write(&dir, "workspace.toml", &workspace_toml("solo"));
    sync::init_at(&dir, None).unwrap();
    let head = git(&dir, &["rev-parse", "HEAD"]);

    write(
        &dir,
        "projects/checkout/PLAN.md",
        "parzi: 1\n<<<<<<< ours\n- [ ] TSK-7\n=======\n- [ ] TSK-8\n>>>>>>> theirs\n",
    );
    let report = sync::commit_and_push_at(&dir, "parzi: plan").unwrap();
    assert_eq!(report.conflicts, vec!["projects/checkout/PLAN.md"]);
    assert!(!report.committed);
    assert_eq!(git(&dir, &["rev-parse", "HEAD"]), head);
}

/// The workspace-level entry points take a `Workspace`, so they need
/// `$PARZI_HOME`; no other test in this file reads it.
#[test]
fn syncing_a_workspace_that_was_never_inited_inits_it() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("PARZI_HOME", home.path());

    let ws = parzi_core::workspace::create(parzi_core::workspace::Workspace::solo("acme"))
        .expect("workspace created");
    let dir = parzi_core::workspace::dir(&ws.name);
    assert!(!dir.join(".git").exists(), "create() leaves git to sync");

    let first = sync::sync_now(&ws, "parzi: workspace created").unwrap();
    assert!(first.committed && !first.pushed && first.conflicts.is_empty());
    assert!(sync::is_repo(&dir));
    assert!(git(&dir, &["show", "--name-only", "--format=", "HEAD"]).contains("workspace.toml"));

    // Second round: a project file is committed, still nothing to push.
    write(
        &dir,
        "projects/checkout/PROJECT.md",
        "parzi: 1\n# Project\n",
    );
    let second = sync::sync_now(&ws, "parzi: project created").unwrap();
    assert!(second.committed && !second.pushed);
    assert_eq!(git(&dir, &["status", "--porcelain=v1"]), "");
}

#[test]
fn sync_on_a_plain_directory_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let err = sync::pull_at(tmp.path()).unwrap_err().to_string();
    assert!(err.contains("not a git repository"), "{err}");
    assert!(!sync::is_repo(tmp.path()));
    assert!(sync::remote_url(tmp.path()).is_none());
}
