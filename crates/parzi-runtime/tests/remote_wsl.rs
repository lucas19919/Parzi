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
        &["-d", &distro, "-e", "env", &home_env, "sh", "-c"],
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
