use std::path::{Path, PathBuf};

// asInvoker matters: without it Windows sees "setup" in the name and asks for
// admin, and an elevated install lands in the wrong user's folder.
const MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>"#;

fn main() {
    println!("cargo:rerun-if-env-changed=PARZI_INSTALLER");
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("installer.exe");
    // Unset for a plain `cargo check`: that build says so if it is ever run.
    match std::env::var("PARZI_INSTALLER") {
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            std::fs::copy(Path::new(&path), &out)
                .unwrap_or_else(|e| panic!("PARZI_INSTALLER {path}: {e}"));
        }
        Err(_) => std::fs::write(&out, b"").unwrap(),
    }
    #[cfg(windows)]
    {
        let mut res = tauri_winres::WindowsResource::new();
        res.set_icon("../src-tauri/icons/icon.ico")
            .set_manifest(MANIFEST)
            .set("FileDescription", "Parzi Setup")
            .set("ProductName", "Parzi");
        res.compile().expect("setup resources");
    }
    let _ = MANIFEST;
}
