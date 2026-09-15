//! The Tauri updater key verifies with `minisign-verify`.
//!
//! Manual run (downloads ~15 MB from the live v0.1.11 release, hence ignored):
//!
//! ```powershell
//! cargo test -p parzi-desk --test minisign_compat -- --ignored --nocapture
//! ```

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use minisign_verify::{PublicKey, Signature};

/// Second line of the decoded `plugins.updater.pubkey` in
/// `src-tauri/tauri.conf.json` (the first line is the untrusted comment).
const PUBKEY_B64: &str = "RWTmSP3dKWYfIISfwPxO2nfUOAdKenf4rfQZrkHkw1w/nI1sT0KG8BOx";
const VERSION: &str = "0.1.11";
const RELEASE_BASE: &str = "https://github.com/lucas19919/Parzi/releases/download";

fn asset_urls() -> (String, String) {
    let exe = format!("{RELEASE_BASE}/v{VERSION}/Parzi_{VERSION}_x64-setup.exe");
    let sig = format!("{exe}.sig");
    (exe, sig)
}

/// Minimal base64 decoder (test-only; avoids a new dependency for one call).
/// Input must be pre-trimmed; returns None on non-alphabet bytes.
fn base64_decode(s: &str) -> Option<String> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let s = s.strip_suffix("==").unwrap_or(s);
    let s = s.strip_suffix('=').unwrap_or(s);
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out: Vec<u8> = Vec::with_capacity(s.len() * 3 / 4);
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let chunk = &b[i..usize::min(i + 4, b.len())];
        let mut n = 0u32;
        for &c in chunk {
            n = (n << 6) | val(c)?;
        }
        n <<= 6 * (4 - chunk.len());
        // A 2-char tail carries 1 byte, a 3-char tail 2 bytes, else 3.
        let take = if chunk.len() == 4 { 3 } else { chunk.len() - 1 };
        for k in 0..take {
            out.push((n >> (16 - 8 * k)) as u8);
        }
        i += 4;
    }
    String::from_utf8(out).ok()
}

fn download(url: &str, dest: &Path) -> io::Result<()> {
    if dest.is_file() && dest.metadata().map(|m| m.len()).unwrap_or(0) > 0 {
        return Ok(());
    }
    let dest_str = dest.to_string_lossy().into_owned();
    // curl ships with Windows 10+; `-f` fails on HTTP errors.
    let curl = Command::new("curl")
        .args(["-fsSL", "-o", &dest_str, url])
        .status();
    match curl {
        Ok(status) if status.success() => return Ok(()),
        _ => {}
    }
    // Fallback when curl is missing.
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("Invoke-WebRequest -Uri '{url}' -OutFile '{dest_str}'"),
        ])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("download failed: {url}"),
        ))
    }
}

#[test]
#[ignore]
fn tauri_release_signature_verifies() {
    let (exe_url, sig_url) = asset_urls();
    let dir = std::env::temp_dir().join(format!("parzi-minisign-{VERSION}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let exe_path: PathBuf = dir.join(format!("Parzi_{VERSION}_x64-setup.exe"));
    let sig_path: PathBuf = dir.join(format!("Parzi_{VERSION}_x64-setup.exe.sig"));

    download(&exe_url, &exe_path).expect("download exe");
    download(&sig_url, &sig_path).expect("download sig");

    let bytes = std::fs::read(&exe_path).expect("read exe");
    assert!(bytes.len() > 1_000_000, "exe suspiciously small");
    let sig_text = std::fs::read_to_string(&sig_path).expect("read sig");
    let public_key = PublicKey::from_base64(PUBKEY_B64).expect("decode pubkey");

    // Tauri v2 ships the minisign file base64-encoded inside the `.sig`
    // asset (single line, no newlines). Accept that transport encoding,
    // falling back to a raw minisign file if the format ever changes.
    let decoded_transport = base64_decode(sig_text.trim());
    let signature = decoded_transport
        .as_deref()
        .and_then(|t| Signature::decode(t).ok())
        .or_else(|| Signature::decode(sig_text.trim()).ok())
        .expect("decode sig");

    // Tauri signs pre-hashed; retry legacy only if the mode mismatches.
    public_key
        .verify(&bytes, &signature, false)
        .or_else(|e| match e {
            minisign_verify::Error::UnexpectedAlgorithm => {
                public_key.verify(&bytes, &signature, true)
            }
            other => Err(other),
        })
        .expect("signature must verify");
    println!("trusted comment: {}", signature.trusted_comment());
}
