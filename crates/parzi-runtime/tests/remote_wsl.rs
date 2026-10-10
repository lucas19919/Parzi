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
    let _ = transport
        .script("rm -rf \"$HOME\"\n", Duration::from_secs(20))
        .await;
    let _ = std::fs::remove_dir_all(&local);
}
