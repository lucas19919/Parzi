//! H0 gates (hub PLAN §11): the grammars round-trip, the capsule schema
//! bites, the lease table is right about collisions and time, and STATUS.md
//! is the same bytes on any machine.
//!
//! Hermetic: every test that touches disk calls `test_home()` first, so
//! nothing ever lands in the real `~/.parzi`.

use std::collections::BTreeSet;

use parzi_core::capsule::{self, HandoffCapsule};
use parzi_core::journal::{self, JournalLine, Kind};
use parzi_core::lease::{Answer, Claim, Holder, LeaseTable, RequestState, REQUEST_TIMEOUT_SECS};
use parzi_core::plan::{parse_plan_v1, render_plan, LanePlan, Plan, Sprint, Task, TaskId};
use parzi_core::project::{self, Criterion, Project, Roster, Status};
use parzi_core::status::render_status;
use parzi_core::workspace::{self, Kind as WsKind, Member, RepoRef, Role, Workspace};

/// One temp `PARZI_HOME` for this binary, set before any path is read.
static TEST_HOME_INIT: std::sync::Once = std::sync::Once::new();

fn test_home() {
    TEST_HOME_INIT.call_once(|| {
        let dir = std::env::temp_dir().join(format!("parzi-test-hub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PARZI_HOME", &dir);
    });
}

fn holder(user: &str, lane: &str) -> Holder {
    Holder {
        user: user.into(),
        machine: "laptop".into(),
        run: Some(format!("run-{lane}")),
        lane: lane.into(),
    }
}

fn task_id(id: &str) -> TaskId {
    TaskId(id.to_string())
}

/// A project with every field of the §2 grammar filled in.
fn full_project() -> Project {
    Project {
        slug: "checkout-flow".into(),
        title: "Checkout flow".into(),
        workspace: "acme".into(),
        repos: vec!["shop-api".into(), "shop-web".into()],
        roster: Roster {
            header: "google/gemini-2.5-flash".into(),
            orchestrator: "anthropic/claude-fable-5-1".into(),
            coder: "opencode/muse-spark-1.3".into(),
        },
        budget_usd: Some(40.0),
        status: Status::Running,
        critical: vec![
            "shop-api/src/payments/**".into(),
            "shop-web/src/routes.rs".into(),
        ],
        why: "Two sentences a new teammate understands.\n\nThe second paragraph survives.".into(),
        what: vec![
            Criterion {
                text: "a session endpoint returns 201".into(),
                done: true,
            },
            Criterion {
                text: "the page posts to it".into(),
                done: false,
            },
        ],
        constraints: vec![
            "no schema migrations this sprint".into(),
            "keep the public API stable".into(),
        ],
    }
}

/// 3 sprints x 4 lanes x 50 tasks, with every bracket attribute in play.
fn big_plan() -> Plan {
    let lanes = ["api", "web", "core", "docs"];
    let mut sprints = vec![];
    let mut n = 0;
    for s in 0..3 {
        let mut sprint = Sprint {
            title: format!("Sprint {} — slice {s}", s + 1),
            target: if s == 0 {
                "1 day".into()
            } else {
                String::new()
            },
            lanes: vec![],
        };
        for (li, lane) in lanes.iter().enumerate() {
            let mut plan_lane = LanePlan {
                name: (*lane).into(),
                tasks: vec![],
            };
            // 50 tasks over the whole plan: 4 per lane, 5 in the last sprint's
            // two back lanes.
            let count = if s == 2 && li >= 2 { 5 } else { 4 };
            for _ in 0..count {
                n += 1;
                plan_lane.tasks.push(Task {
                    id: task_id(&format!("TSK-{n}")),
                    title: format!("Task number {n} that does a thing"),
                    repo: format!("shop-{lane}"),
                    scope: vec![format!("src/{lane}/**"), "src/routes.rs".into()],
                    after: if n > 1 {
                        vec![task_id(&format!("TSK-{}", n - 1))]
                    } else {
                        vec![]
                    },
                    critical: n % 7 == 0,
                    done: n % 5 == 0,
                    acceptance: if n % 3 == 0 {
                        vec![format!("cargo test -p shop-{lane} passes")]
                    } else {
                        vec![]
                    },
                });
            }
            sprint.lanes.push(plan_lane);
        }
        sprints.push(sprint);
    }
    assert_eq!(n, 50, "the gate is a 50-task plan");
    Plan { sprints }
}

#[test]
fn project_md_round_trips_with_every_field() {
    let p = full_project();
    let back = project::parse(&project::render(&p)).expect("renders valid PROJECT.md");
    assert_eq!(back, p);
    assert!(project::render(&p).starts_with("parzi: 1\n# Project: Checkout flow\n"));
}

#[test]
fn project_md_is_tolerant_of_order_blank_lines_and_comments() {
    let raw = "parzi: 1\n\nstatus: planned    # set by the orchestrator\n\n\
# Project: Checkout flow\n\nrepos:  shop-api ,  shop-web\n\
roster: coder = a/b , header = c/d\nbudget: $40.50\nworkspace: acme\n\n\
## Constraints\n- no migrations\n\n## What\n- [x] one\n- [ ] two\n\n## Why\nBecause.\n";
    let p = project::parse(raw).expect("tolerant parse");
    assert_eq!(p.status, Status::Planned);
    assert_eq!(p.repos, vec!["shop-api", "shop-web"]);
    assert_eq!(p.roster.header, "c/d");
    assert_eq!(p.roster.coder, "a/b");
    assert_eq!(p.budget_usd, Some(40.5));
    assert_eq!(p.slug, "checkout-flow", "the slug falls back to the title");
    assert_eq!(p.what.len(), 2);
    assert!(p.what[0].done && !p.what[1].done);
    assert_eq!(p.constraints, vec!["no migrations"]);
    assert_eq!(p.why, "Because.");
}

#[test]
fn unknown_grammar_major_is_refused_not_guessed() {
    let err = project::parse("parzi: 2\n# Project: X\n").expect_err("major 2 is refused");
    assert!(err.to_string().contains("unsupported"), "{err}");
    assert!(parse_plan_v1("parzi: 9\n## Sprint 1\n").is_err());
    // A file written before the version line still reads as v1.
    assert!(project::parse("# Project: X\n").is_ok());
}

#[test]
fn plan_md_round_trips_50_tasks_across_3_sprints_and_4_lanes() {
    let plan = big_plan();
    let text = render_plan(&plan);
    let back = parse_plan_v1(&text).expect("renders valid PLAN.md");
    assert_eq!(back, plan);
    assert_eq!(back.tasks().count(), 50);
    assert_eq!(back.lane_of(&task_id("TSK-3")), Some("api"));
    assert_eq!(back.lane_of(&task_id("TSK-5")), Some("web"));
    // Re-rendering is stable, which is what makes the git diff readable.
    assert_eq!(render_plan(&back), text);
}

#[test]
fn a_50_task_plan_parses_in_under_a_millisecond() {
    let text = render_plan(&big_plan());
    // The gate is the parser, not whatever else the machine is running: warm
    // up, then take the fastest of 500 parses. A debug build does this in
    // ~100 µs; only a parser that got slow by an order of magnitude fails,
    // however loaded the box is.
    for _ in 0..10 {
        parse_plan_v1(&text).expect("warm-up parse");
    }
    let mut best = std::time::Duration::from_secs(1);
    for _ in 0..500 {
        let t0 = std::time::Instant::now();
        parse_plan_v1(&text).expect("parse");
        best = best.min(t0.elapsed());
    }
    assert!(best.as_micros() < 1_000, "50-task plan parsed in {best:?}");
}

#[test]
fn plan_tags_parse_in_any_order() {
    let raw = "parzi: 1\n## Sprint 1 — API surface (target: 1 day)\n### lane api\n\
- [ ] TSK-7 Checkout session endpoint [scope:src/checkout/**,src/routes.rs] [repo:shop-api] [after:]\n\
- [x] TSK-8 Payment intent adapter [critical] [repo:shop-api]\n  - acceptance: cargo test passes\n\
### lane web\n\
- [ ] TSK-9 Page skeleton [after:TSK-7] [repo:shop-web]\n";
    let plan = parse_plan_v1(raw).expect("tags in any order");
    assert_eq!(plan.sprints.len(), 1);
    assert_eq!(plan.sprints[0].target, "1 day");
    assert_eq!(plan.sprints[0].title, "Sprint 1 — API surface");
    let t7 = plan.task(&task_id("TSK-7")).expect("TSK-7");
    assert_eq!(t7.repo, "shop-api");
    assert_eq!(t7.scope, vec!["src/checkout/**", "src/routes.rs"]);
    assert!(t7.after.is_empty(), "an empty [after:] is no dependency");
    let t8 = plan.task(&task_id("TSK-8")).expect("TSK-8");
    assert!(t8.critical && t8.done);
    assert_eq!(t8.acceptance, vec!["cargo test passes"]);
    assert_eq!(
        plan.task(&task_id("TSK-9")).expect("TSK-9").after,
        vec![task_id("TSK-7")]
    );
}

#[test]
fn the_legacy_plan_grammar_still_parses() {
    let p = parzi_core::plan::parse_plan("## Alpha\n\n- [ ] build core [lane:core]\n");
    assert_eq!(p.milestones.len(), 2, "one header plus one task");
}

#[test]
fn critical_globs_match_stars_double_stars_and_prefixes() {
    let p = full_project();
    assert!(project::is_critical(&p, "shop-api/src/payments/intent.rs"));
    assert!(project::is_critical(&p, "shop-api/src/payments/a/b/c.rs"));
    assert!(project::is_critical(&p, "shop-web/src/routes.rs"));
    assert!(!project::is_critical(&p, "shop-api/src/checkout/mod.rs"));
    assert!(!project::is_critical(&p, "shop-web/src/routes/mod.rs"));
    // The matcher itself, spelled out.
    assert!(project::glob_match("src/**", "src/a/b.rs"));
    assert!(project::glob_match("src/**", "src"));
    assert!(project::glob_match("src/*.rs", "src/main.rs"));
    assert!(!project::glob_match("src/*.rs", "src/a/main.rs"));
    assert!(project::glob_match("src/?.rs", "src/a.rs"));
    assert!(project::glob_match("src/payments", "src/payments/x.rs"));
    // Windows separators never change the answer.
    assert!(project::glob_match("src/**", "src\\a\\b.rs"));
}

#[test]
fn capsule_schema_requires_a_task_a_summary_and_a_verification() {
    let ok = serde_json::json!({
        "task": "TSK-7",
        "summary": "endpoint added",
        "touched_files": ["src/checkout/mod.rs"],
        "exported_symbols": ["create_session"],
        "verification": "cargo test -p shop-api (12 passed)",
        "invariants": ["session ids are uuids"],
        "gotchas": []
    });
    let c = capsule::validate(&ok).expect("valid capsule");
    assert_eq!(c.task, task_id("TSK-7"));
    assert!(c.render().contains("verified: cargo test -p shop-api"));

    let mut missing = ok.clone();
    missing["verification"] = serde_json::json!("   ");
    let err = capsule::validate(&missing).expect_err("verification is required");
    assert!(err.to_string().contains("verification"), "{err}");

    let mut blank = ok.clone();
    blank["summary"] = serde_json::json!("");
    assert!(capsule::validate(&blank).is_err());
    assert!(capsule::validate(&serde_json::json!({ "task": "TSK-7" })).is_err());

    let mut huge = ok;
    huge["summary"] = serde_json::json!("x".repeat(2_001));
    assert!(capsule::validate(&huge).is_err());
}

#[test]
fn capsules_round_trip_on_disk_and_refuse_a_path_escape() {
    test_home();
    let ws = "caps-ws";
    workspace::create(Workspace::solo(ws)).expect("workspace");
    let c = HandoffCapsule {
        task: task_id("TSK-11"),
        summary: "adapter".into(),
        touched_files: vec!["src/payments/mod.rs".into()],
        exported_symbols: vec![],
        verification: "cargo test".into(),
        invariants: vec![],
        gotchas: vec!["the sandbox rejects absolute paths".into()],
    };
    capsule::save(ws, "p", &c).expect("save");
    assert_eq!(capsule::load(ws, "p", &c.task).expect("load"), c);
    assert_eq!(
        capsule::for_after(ws, "p", &[c.task.clone(), task_id("TSK-404")]).len(),
        1,
        "a missing predecessor capsule is skipped, not fatal"
    );
    assert!(capsule::path(ws, "p", &task_id("../../etc")).is_none());
}

#[test]
fn leases_collide_on_overlapping_paths_and_free_up_on_release() {
    let mut t = LeaseTable::new();
    let api = holder("ada", "api");
    let web = holder("sam", "web");
    assert_eq!(
        t.claim(
            task_id("TSK-8"),
            api.clone(),
            ["shop-api/src/payments/", "shop-api/src/routes.rs"].map(String::from)
        ),
        Claim::Granted
    );
    // A file inside a held directory is held.
    let Claim::Held { by } = t.claim(
        task_id("TSK-9"),
        web.clone(),
        ["shop-api/src/payments/intent.rs".to_string()],
    ) else {
        panic!("an overlapping claim must be refused");
    };
    assert_eq!(by.holder, api);
    assert_eq!(by.task, task_id("TSK-8"));
    // A disjoint claim is granted.
    assert_eq!(
        t.claim(
            task_id("TSK-9"),
            web.clone(),
            ["shop-web/src/page.svelte".to_string()]
        ),
        Claim::Granted
    );
    assert_eq!(
        t.holder_of("shop-api/src/routes.rs").map(|l| &l.holder),
        Some(&api)
    );
    assert!(t.holder_of("shop-api/src/checkout/mod.rs").is_none());
    let freed = t.release(&task_id("TSK-8")).expect("held a lease");
    assert_eq!(freed.paths.len(), 2);
    assert_eq!(
        t.claim(
            task_id("TSK-10"),
            web,
            ["shop-api/src/routes.rs".to_string()]
        ),
        Claim::Granted
    );
}

#[test]
fn a_lease_expires_when_its_heartbeat_stops() {
    let mut t = LeaseTable::new();
    t.claim(
        task_id("TSK-1"),
        holder("ada", "api"),
        ["r/a.rs".to_string()],
    );
    let now = chrono::Utc::now();
    assert!(t.expire(now).leases.is_empty(), "a fresh lease survives");
    assert!(t.heartbeat(&task_id("TSK-1")));
    assert!(!t.heartbeat(&task_id("TSK-404")));
    let later = now + chrono::Duration::seconds(91);
    let gone = t.expire(later);
    assert_eq!(gone.leases.len(), 1);
    assert_eq!(gone.leases[0].task, task_id("TSK-1"));
    assert!(t.is_empty());
    assert!(t.expire(later).leases.is_empty(), "expire is idempotent");
}

#[test]
fn a_request_moves_the_path_on_grant_and_times_out_into_a_deny() {
    let mut t = LeaseTable::new();
    t.claim(
        task_id("TSK-8"),
        holder("ada", "api"),
        ["shop-api/src/routes.rs".to_string()],
    );
    let id = t.request(
        "shop-api/src/routes.rs",
        holder("sam", "web"),
        task_id("TSK-9"),
    );
    assert_eq!(t.pending().count(), 1);
    let answered = t.answer(id, Answer::Grant).expect("grant");
    assert!(matches!(answered.state, RequestState::Granted));
    assert_eq!(
        t.holder_of("shop-api/src/routes.rs")
            .map(|l| l.task.clone()),
        Some(task_id("TSK-9")),
        "the path moved to the requester"
    );
    assert!(t
        .lease_of(&task_id("TSK-8"))
        .expect("still a lease")
        .paths
        .is_empty());
    assert!(
        t.answer(id, Answer::Grant).is_err(),
        "answered once, only once"
    );

    // Deny with a reason, then a request nobody answers: silence is a deny.
    let denied = t.request(
        "shop-api/src/routes.rs",
        holder("ada", "api"),
        task_id("TSK-8"),
    );
    t.answer(
        denied,
        Answer::Deny {
            reason: "mid-refactor".into(),
        },
    )
    .expect("deny");
    let silent = t.request(
        "shop-api/src/routes.rs",
        holder("ada", "api"),
        task_id("TSK-8"),
    );
    let late = chrono::Utc::now() + chrono::Duration::seconds(REQUEST_TIMEOUT_SECS + 1);
    let expired = t.expire(late);
    assert_eq!(expired.denied.len(), 1);
    assert_eq!(expired.denied[0].id, silent);
    assert!(matches!(
        t.request_get(silent).map(|r| r.state.clone()),
        Some(RequestState::Denied { .. })
    ));
    // Two asks per file per task is the cap; a third convenes (§15.6).
    assert!(t.should_convene("shop-api/src/routes.rs", &task_id("TSK-8")));
    assert!(!t.should_convene("shop-api/src/routes.rs", &task_id("TSK-9")));
}

#[test]
fn status_md_is_deterministic() {
    let project = full_project();
    let plan = parse_plan_v1(
        "parzi: 1\n## Sprint 1 — API surface (target: 1 day)\n### lane api\n\
- [x] TSK-7 Endpoint [repo:shop-api]\n- [ ] TSK-8 Adapter [repo:shop-api] [critical]\n\
### lane web\n- [ ] TSK-9 Page [repo:shop-web]\n\n## Sprint 2 — wire and test\n\
### lane api\n- [ ] TSK-10 Tests [repo:shop-api]\n",
    )
    .expect("plan");
    let mut leases = LeaseTable::new();
    leases.claim(
        task_id("TSK-8"),
        holder("ada", "api"),
        [
            "shop-api/src/payments/intent.rs".to_string(),
            "shop-api/src/routes.rs".to_string(),
        ],
    );
    leases.claim(
        task_id("TSK-9"),
        holder("sam", "web"),
        ["shop-web/src/page.svelte".to_string()],
    );
    leases.request(
        "shop-api/src/routes.rs",
        holder("sam", "web"),
        task_id("TSK-9"),
    );
    let at = chrono::DateTime::parse_from_rfc3339("2026-09-16T09:00:00Z")
        .expect("fixed stamp")
        .with_timezone(&chrono::Utc);
    let journal: Vec<JournalLine> = [
        (Kind::Claim, "ada", Some("TSK-8"), "claimed 2 files"),
        (
            Kind::Request,
            "sam",
            Some("TSK-9"),
            "wants shop-api/src/routes.rs",
        ),
        (Kind::Note, "ada", None, "sprint 1 dispatched"),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (kind, who, task, text))| JournalLine {
        at: at + chrono::Duration::seconds(i as i64 * 30),
        who: who.into(),
        kind,
        task: task.map(task_id),
        text: text.into(),
    })
    .collect();

    let rendered = scrub_stamps(&render_status(&project, &plan, &leases, &journal));
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("status.md");
    if std::env::var_os("PARZI_GOLDEN_UPDATE").is_some() {
        std::fs::write(&golden_path, rendered.as_bytes()).expect("write golden");
    }
    let golden = std::fs::read_to_string(&golden_path).expect("golden STATUS.md");
    assert_eq!(rendered, golden.replace("\r\n", "\n"));
    // Same state in, same bytes out.
    assert_eq!(
        scrub_stamps(&render_status(&project, &plan, &leases, &journal)),
        rendered
    );
}

/// Wall-clock stamps (a lease's `last seen`, a request's `asked`) are the only
/// part of STATUS.md a test cannot pin from outside the table, so the golden
/// compares everything else byte for byte.
fn scrub_stamps(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let end = line.trim_end_matches('\n');
        let scrubbed: Vec<&str> = end
            .split(' ')
            .map(|tok| {
                if tok.len() == 10 && tok.starts_with("20") && tok.as_bytes()[4] == b'-' {
                    "<date>"
                } else if tok.len() == 9 && tok.ends_with('Z') && tok.contains(':') {
                    "<time>"
                } else {
                    tok
                }
            })
            .collect();
        out.push_str(&scrubbed.join(" "));
        out.push_str(&line[end.len()..]);
    }
    out
}

#[test]
fn workspaces_are_created_loaded_listed_and_re_mapped() {
    test_home();
    let ws = Workspace {
        name: "acme".into(),
        kind: WsKind::Team,
        repos: vec![RepoRef {
            name: "shop-api".into(),
            remote: "git@github.com:acme/shop-api.git".into(),
            default_branch: "main".into(),
            local_path: None,
        }],
        members: vec![
            Member {
                user: "ada".into(),
                role: Role::Owner,
            },
            Member {
                user: "sam".into(),
                role: Role::Maintainer,
            },
        ],
        defaults: Default::default(),
    };
    let made = workspace::create(ws.clone()).expect("create");
    assert_eq!(made, ws);
    assert!(workspace::dir("acme").join("projects").is_dir());
    assert_eq!(workspace::load("acme").expect("load"), ws);
    assert!(workspace::list().contains(&"acme".to_string()));
    assert!(
        workspace::create(Workspace::solo("acme")).is_err(),
        "creating over an existing workspace is refused"
    );
    // Re-running the wizard re-maps the local path instead of duplicating.
    let re = workspace::add_repos(
        "acme",
        vec![RepoRef {
            name: "shop-api".into(),
            remote: "git@github.com:acme/shop-api.git".into(),
            default_branch: "main".into(),
            local_path: Some(std::path::PathBuf::from("C:/code/shop-api")),
        }],
    )
    .expect("add_repos");
    assert_eq!(re.repos.len(), 1);
    assert!(re.repos[0].local_path.is_some());
    assert_eq!(re.role_of("sam"), Some(Role::Maintainer));
    assert!(Role::Maintainer.may_approve_critical() && !Role::Member.may_approve_critical());
    assert!(workspace::load("../escape").is_err());
}

#[test]
fn a_project_is_saved_under_its_workspace_and_listed() {
    test_home();
    workspace::create(Workspace::solo("proj-ws")).expect("workspace");
    let mut p = full_project();
    p.workspace = "proj-ws".into();
    project::save(&p).expect("save");
    assert_eq!(project::load("proj-ws", "checkout-flow").expect("load"), p);
    assert_eq!(project::list("proj-ws"), vec!["checkout-flow".to_string()]);
    assert_eq!(project::slug_of("Checkout Flow!! "), "checkout-flow");
    assert_eq!(project::slug_of("///"), "project");
}

#[test]
fn the_journal_appends_whole_lines_and_tails() {
    test_home();
    workspace::create(Workspace::solo("jrnl-ws")).expect("workspace");
    for i in 0..12 {
        journal::append(
            "jrnl-ws",
            "p",
            &JournalLine::now(
                "ada",
                Kind::Claim,
                Some(task_id(&format!("TSK-{i}"))),
                format!("line {i}"),
            ),
        )
        .expect("append");
    }
    let all = journal::read("jrnl-ws", "p");
    assert_eq!(all.len(), 12);
    assert_eq!(all[0].text, "line 0");
    let tail = journal::tail("jrnl-ws", "p", 10);
    assert_eq!(tail.len(), 10);
    assert_eq!(tail[0].text, "line 2");
    // A torn or foreign line is skipped, never fatal.
    let path = journal::path("jrnl-ws", "p");
    let raw = std::fs::read_to_string(&path).expect("journal on disk");
    std::fs::write(&path, format!("{raw}{{not json\n")).expect("append a bad line");
    assert_eq!(journal::read("jrnl-ws", "p").len(), 12);
    assert_eq!(Kind::PlanChanged.as_str(), "plan_changed");
}

#[test]
fn a_legacy_project_reads_as_a_solo_workspace_with_one_repo() {
    test_home();
    let legacy = parzi_core::paths::projects_dir()
        .expect("projects dir")
        .join("oldproj");
    std::fs::create_dir_all(legacy.join("lanes").join("core")).expect("lane core");
    std::fs::create_dir_all(legacy.join("lanes").join("ui")).expect("lane ui");
    let root = std::env::temp_dir()
        .join("parzi-legacy-root")
        .join("shop-api");
    std::fs::create_dir_all(&root).expect("legacy root");
    std::fs::write(
        legacy.join("parzi.toml"),
        format!("root = \"{}\"\n", root.to_string_lossy().replace('\\', "/")),
    )
    .expect("parzi.toml");
    std::fs::write(legacy.join("lanes").join("core").join("SYSTEM.md"), "core").expect("core");
    std::fs::write(legacy.join("lanes").join("ui").join("SYSTEM.md"), "ui").expect("ui");

    let ws = workspace::from_legacy_project("oldproj").expect("legacy read");
    assert_eq!(ws.name, "oldproj");
    assert_eq!(ws.kind, WsKind::Solo);
    assert_eq!(ws.repos.len(), 1);
    assert_eq!(ws.repos[0].name, "shop-api");
    assert_eq!(ws.repos[0].default_branch, "main");
    assert!(
        ws.repos[0].remote.is_empty(),
        "the wizard fills the remote in"
    );
    assert_eq!(
        workspace::legacy_lane_names("oldproj"),
        vec!["core".to_string(), "ui".to_string()]
    );
    // Nothing is written or moved: migration stays a human act (§15.1).
    assert!(!workspace::dir("oldproj").exists());
    assert!(legacy.join("parzi.toml").is_file());
    assert!(workspace::from_legacy_project("no-such-project").is_err());
}

#[test]
fn a_lease_path_set_is_normalised_once() {
    let mut t = LeaseTable::new();
    t.claim(
        task_id("TSK-1"),
        holder("ada", "api"),
        [
            "./shop-api/src/a.rs".to_string(),
            "shop-api\\src\\a.rs".to_string(),
            "  ".to_string(),
        ],
    );
    let held: &BTreeSet<String> = &t.lease_of(&task_id("TSK-1")).expect("a lease").paths;
    assert_eq!(held.len(), 1, "one file, however it was spelled: {held:?}");
    assert!(held.contains("shop-api/src/a.rs"));
}
