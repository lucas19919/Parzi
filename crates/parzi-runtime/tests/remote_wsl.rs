//! ParziOS end to end, with WSL standing in for the SSH server:
//!
//! PARZI_WSL_DISTRO=Ubuntu-24.04 PARZI_LINUX_BIN=C:\path\to\linux\parzi \
//!   cargo test -p parzi-runtime --test remote_wsl -- --ignored
//!
//! The remote side runs with HOME set to a fresh folder under /tmp, so the
//! distro's own home is never touched.

use std::sync::Mutex;
use std::time::Duration;

use parzi_runtime::remote::{self, Progress, SetupOptions, Target, Transport};
use parzi_runtime::sync;
use serde_json::json;

#[tokio::test]
#[ignore = "needs WSL and a Linux build of parzi"]
async fn setup_link_and_answer_through_wsl() {
    let distro = std::env::var("PARZI_WSL_DISTRO").expect("PARZI_WSL_DISTRO");
    let bin = std::env::var("PARZI_LINUX_BIN").expect("PARZI_LINUX_BIN");

    let local = std::env::temp_dir().join(format!("parzi-wsl-desk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&local);
    std::fs::create_dir_all(local.join("brain").join("projects")).unwrap();
    std::env::set_var("PARZI_HOME", &local);
    std::fs::write(
        local.join("brain").join("hello.md"),
        "# Hello\nfrom the desk\n",
    )
    .unwrap();
    std::fs::write(
        local.join("brain").join("projects").join("app.md"),
        "---\nfolder: C:\\src\\app\n---\n# App\n",
    )
    .unwrap();

    let home = format!("/tmp/parzi-wsl-e2e-{}", std::process::id());
    let home_env = format!("HOME={home}");
    let transport = Transport::custom(
        "wsl.exe",
        &[
            "-d",
            &distro,
            "-e",
            "env",
            &home_env,
            "PARZI_DEVICE=wsl-e2e",
            "sh",
            "-c",
        ],
    );
    let seen: Mutex<Vec<Progress>> = Mutex::default();
    let opts = SetupOptions {
        binary: Some(bin.into()),
        transport: Some(transport.clone()),
        ..Default::default()
    };
    let result = remote::setup(Target::parse("e2e", "wsl").unwrap(), opts, &|p| {
        seen.lock().unwrap().push(p)
    })
    .await;
    let steps = seen.lock().unwrap().clone();
    for p in &steps {
        eprintln!("{:<8} {:<5} {}", p.step, p.state, p.detail);
    }
    let (_, link) = result.expect("setup finishes");
    assert!(steps.iter().all(|p| p.state != "fail"));
    for step in [
        "connect", "probe", "install", "spec", "brain", "engine", "link",
    ] {
        assert!(
            steps.iter().any(|p| p.step == step && p.state == "ok"),
            "no ok for {step}"
        );
    }

    let health = link
        .call("health", json!({}), Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(health["version"], json!(env!("CARGO_PKG_VERSION")));
    let list = link
        .call("session.list", json!({}), Duration::from_secs(10))
        .await
        .unwrap();
    assert!(list["sessions"].is_array());
    let agents = link
        .call("providers", json!({}), Duration::from_secs(60))
        .await
        .unwrap();
    assert!(agents["providers"].is_array());
    let bad = link
        .call("no.such.op", json!({}), Duration::from_secs(10))
        .await
        .unwrap_err();
    assert!(bad.contains("unknown op"), "{bad}");

    let notes = transport
        .script("ls \"$HOME/.parzi/brain\"\n", Duration::from_secs(20))
        .await
        .unwrap();
    assert!(notes.stdout.contains("hello.md"), "{}", notes.stdout);
    assert!(
        !notes.stdout.contains("projects"),
        "project notes stay home"
    );
    let leftover = transport
        .script(
            "ls \"$HOME/.parzi/setup-spec.json\" 2>&1 || true\n",
            Duration::from_secs(20),
        )
        .await
        .unwrap();
    assert!(
        leftover.stdout.contains("No such file"),
        "the spec is deleted after setup"
    );

    // Live stream: a run on an agent that is not installed there still
    // starts and streams its end. Nothing reaches a vendor, so no quota.
    let missing = agents["providers"]
        .as_array()
        .and_then(|all| {
            all.iter()
                .find(|a| a["state"] == json!("not_installed"))
                .and_then(|a| a["provider"].as_str())
        })
        .expect("an agent that is not installed in the test home")
        .to_string();
    let mut events = link.subscribe().await.expect("subscribe");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let sent = link
        .call(
            "session.send",
            json!({ "target": "new", "message": "hello", "model": missing, "lane": "build", "mode": "supervised" }),
            Duration::from_secs(30),
        )
        .await
        .expect("the run starts");
    let sid = sent["id"].as_str().unwrap().to_string();
    let end = tokio::time::timeout(Duration::from_secs(60), async {
        while let Some(ev) = events.recv().await {
            if ev["session"] == json!(sid) && matches!(ev["kind"].as_str(), Some("error" | "done"))
            {
                return Some(ev);
            }
        }
        None
    })
    .await
    .expect("an end event within a minute")
    .expect("the stream stays open");
    eprintln!("streamed: {end}");
    let thread = link
        .call(
            "session.events",
            json!({ "id": sid }),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
    assert!(thread["events"].as_array().is_some_and(|e| !e.is_empty()));

    // Sync: notes both ways, a project folder per device, a conflict, a
    // delete, settings, and this device's sessions in the server's mirror.
    let wait = Duration::from_secs(20);
    let server = "e2e@wsl";
    let store = parzi_core::store::SessionStore::open().unwrap();
    let desk_sid = store
        .create("desk session", "default", "build", "claude")
        .unwrap()
        .id;
    store
        .append(
            &desk_sid,
            &parzi_core::store::Event::User {
                text: "from the desk".into(),
            },
        )
        .unwrap();
    std::fs::write(local.join("brain").join("shared.md"), "v1\n").unwrap();
    let first = sync::everything(&link, &store, server).await;
    eprintln!("first sync: {}", first.summary());
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    assert!(
        matches!(first.settings, sync::SettingsOutcome::Pushed),
        "an empty server takes the desk's settings"
    );
    let there = manifest_there(&link).await;
    assert!(
        there.contains_key("shared.md") && there.contains_key("projects/app.md"),
        "{there:?}"
    );
    let app_here =
        std::fs::read_to_string(local.join("brain").join("projects").join("app.md")).unwrap();
    assert!(
        app_here.contains("folder@") && !app_here.contains("\nfolder:"),
        "claimed: {app_here}"
    );
    let projects = link.call("project.list", json!({}), wait).await.unwrap();
    let app = projects["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["slug"] == json!("app"))
        .expect("the server sees the project")
        .clone();
    assert_eq!(app["folder"], json!(""), "no folder on the server yet");
    link.call(
        "project.folder",
        json!({ "slug": "app", "title": "App", "folder": "/srv/app" }),
        wait,
    )
    .await
    .unwrap();
    link.call(
        "brain.apply",
        json!({ "put": [
            { "path": "server.md", "text": "made there\n" },
            { "path": "shared.md", "text": "v2 there\n" },
        ] }),
        wait,
    )
    .await
    .unwrap();
    std::fs::write(local.join("brain").join("shared.md"), "v2 here\n").unwrap();
    std::fs::remove_file(local.join("brain").join("hello.md")).unwrap();
    let second = sync::everything(&link, &store, server).await;
    eprintln!("second sync: {}", second.summary());
    assert!(second.errors.is_empty(), "{:?}", second.errors);
    assert_eq!(second.brain.conflicts, 1);
    let brain = local.join("brain");
    assert_eq!(
        std::fs::read_to_string(brain.join("shared.md")).unwrap(),
        "v2 there\n",
        "the server's text keeps the name"
    );
    let aside = std::fs::read_dir(&brain)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.starts_with("shared.conflict-"))
        .expect("this device's text is kept beside it");
    assert_eq!(
        std::fs::read_to_string(brain.join(&aside)).unwrap(),
        "v2 here\n"
    );
    assert!(brain.join("server.md").exists());
    let there = manifest_there(&link).await;
    assert!(there.contains_key(&aside), "the copy reached the server");
    assert!(
        !there.contains_key("hello.md"),
        "the delete reached the server"
    );
    let app_here = std::fs::read_to_string(brain.join("projects").join("app.md")).unwrap();
    assert!(
        app_here.contains("folder@wsl-e2e"),
        "both folders: {app_here}"
    );
    let mirrors = link.call("mirror.list", json!({}), wait).await.unwrap();
    let card = mirrors["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["session"]["id"] == json!(desk_sid))
        .expect("this device's session is mirrored")
        .clone();
    let device = card["device"].as_str().unwrap().to_string();
    let read = link
        .call(
            "mirror.events",
            json!({ "device": device, "id": desk_sid }),
            wait,
        )
        .await
        .unwrap();
    assert_eq!(read["total"], json!(1));
    let adopted = link
        .call(
            "mirror.adopt",
            json!({ "device": device, "id": desk_sid }),
            wait,
        )
        .await
        .unwrap();
    let on_server = link
        .call("session.events", json!({ "id": adopted["id"] }), wait)
        .await
        .unwrap();
    assert_eq!(on_server["total"], json!(1), "the history came along");
    let third = sync::everything(&link, &store, server).await;
    assert!(third.errors.is_empty(), "{:?}", third.errors);
    assert_eq!(third.summary(), "", "nothing left to move");

    // A second link while the first is open: both answer.
    let second = remote::Link::open(&transport).await.unwrap();
    assert!(second
        .call("health", json!({}), Duration::from_secs(10))
        .await
        .is_ok());

    let _ = link
        .call("shutdown", json!({}), Duration::from_secs(10))
        .await;
    drop(link);
    drop(second);
    tokio::time::sleep(Duration::from_millis(500)).await;
    // The engine may still write its log while it stops: retry.
    let _ = transport
        .script(
            "for i in 1 2 3 4 5; do rm -rf \"$HOME\"; [ -e \"$HOME\" ] || exit 0; sleep 1; done\n",
            Duration::from_secs(20),
        )
        .await;
    let _ = std::fs::remove_dir_all(&local);
}

async fn manifest_there(link: &remote::Link) -> std::collections::BTreeMap<String, String> {
    let v = link
        .call("brain.manifest", json!({}), Duration::from_secs(20))
        .await
        .unwrap();
    serde_json::from_value(v["notes"].clone()).unwrap()
}
