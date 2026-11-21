//! `parzi connector` for agents on a computer without the parzi CLI: the app
//! answers it itself (`parzi-app connector ...`), and keeps a small `parzi`
//! launcher in ~/.parzi/bin, which session shells have on their PATH.

use std::path::{Path, PathBuf};

/// Runs `connector <args>`, prints what it says and exits with its status.
pub fn answer(args: &[String]) -> ! {
    let out = parzi_runtime::connector_cli::run(args, || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text);
        text
    });
    if out.ok {
        println!("{}", out.text);
        std::process::exit(0);
    }
    eprintln!("{}", out.text);
    std::process::exit(1);
}

#[cfg(windows)]
const SHIM: &str = "parzi.cmd";
#[cfg(not(windows))]
const SHIM: &str = "parzi";

/// Writes the launcher when no real `parzi` is on the PATH, and removes it
/// when one is, so it never hides an installed CLI.
pub fn install() {
    let Ok(bin) = parzi_core::paths::parzi_dir().map(|d| d.join("bin")) else {
        return;
    };
    let shim = bin.join(SHIM);
    if real_cli_on_path(&bin) {
        let _ = std::fs::remove_file(&shim);
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let text = launcher(&exe);
    if std::fs::read_to_string(&shim).ok().as_deref() == Some(text.as_str()) {
        return;
    }
    if std::fs::create_dir_all(&bin).is_err() || std::fs::write(&shim, &text).is_err() {
        tracing::debug!("the parzi connector launcher was not written");
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755));
    }
}

/// The launcher: `parzi connector ...` goes to the app; anything else says
/// the CLI is not installed here.
fn launcher(exe: &Path) -> String {
    let exe = exe.display();
    let missing =
        "Only parzi connector works here: the parzi CLI is not installed on this computer.";
    if cfg!(windows) {
        format!(
            "@echo off\r\nif /I \"%~1\"==\"connector\" (\r\n  \"{exe}\" %*\r\n  exit /b %ERRORLEVEL%\r\n)\r\necho {missing} 1>&2\r\nexit /b 2\r\n"
        )
    } else {
        format!(
            "#!/bin/sh\nif [ \"$1\" = connector ]; then exec \"{exe}\" \"$@\"; fi\necho \"{missing}\" >&2\nexit 2\n"
        )
    }
}

/// Whether a `parzi` other than the launcher is on the PATH.
fn real_cli_on_path(bin: &Path) -> bool {
    let names: &[&str] = if cfg!(windows) {
        &["parzi.exe"]
    } else {
        &["parzi"]
    };
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path)
            .filter(|dir| !same_dir(dir, bin))
            .any(|dir| names.iter().any(|n| dir.join(n).is_file()))
    })
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
    canon(a) == canon(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_only_forwards_connector() {
        let text = launcher(Path::new("C:/Apps/Parzi/parzi-app.exe"));
        assert!(text.contains("connector"));
        assert!(text.contains("parzi-app.exe"));
        assert!(text.contains("not installed"));
    }
}
