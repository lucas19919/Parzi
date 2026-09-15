# Packaging & distribution — Phase 5 research (cargo-packager migration path)

Status: research + disabled draft config only. The live pipeline
(`.github/workflows/release.yml` → tauri-action, NSIS per-user `.exe` + WiX
MSI + unsigned universal DMG + minisign-signed `latest.json` feed) ships every
release and is NOT touched by this work.

Research assets:
- `crates/parzi-desk/packaging/NOTES.md` — exact asset map, NSIS/WiX
  settings, updater design
- `crates/parzi-desk/packaging/packager.toml` — commented draft (all keys
  UNCERTAIN until checked against the packager reference)
- `.github/workflows/desk-release.yml` — DISABLED (`if: false` on every
  job); enables only at Phase 6 cutover

Ground truth (from `src-tauri/tauri.conf.json`): productName `Parzi`,
identifier `com.parzi.app`, version `0.1.11` (workspace), publisher `Parzi`,
feed `https://github.com/lucas19919/Parzi/releases/latest/download/latest.json`.

## 1. macOS coexistence plan — do not break the DMG lane

A macOS lane just shipped a DMG workflow (unsigned universal
`Parzi_*_universal.dmg`, Apple Silicon + Intel via `--target
universal-apple-darwin`, `minimumSystemVersion 11.0`, DMG window 660×400,
Gatekeeper right-click-open note in the release body). That workflow stays
LIVE and authoritative for macOS until the native DMG is verified:

1. Until cutover, macOS ships ONLY from `release.yml` (Tauri). The disabled
   `desk-release.yml` macOS job must not be enabled early for any reason.
2. When the native DMG exists, verify side-by-side before switching: installs
   to `/Applications`, launches on both Apple Silicon and Intel (or the
   universal binary's `lipo -info` shows both archs), icon (`icon.icns`)
   renders, Gatekeeper behaviour matches today's note, and the app's update
   check hits the same `latest.json`.
3. Only then propose flipping the macOS job at cutover review — never
   unilaterally. If the native DMG fails verification, Windows may still cut
   over while macOS stays on Tauri (mixed ship is acceptable; the feed serves
   per-platform entries).

## 2. Minisign key-compat test procedure (defer execution to Lucas)

Question (PLAN §0, §7 updater-key row): does the existing Tauri minisign key
verify unchanged under `cargo-packager-updater`, or is a new keypair needed?

- Pubkey (from `tauri.conf.json`): `dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDIwMUY2NjI5RERGRDQ4RTYKUldUbVNQM2RLV1lmSUlTZndQeE8ybmZVT0FkS2VuZjRyZlFacmtIa3cxdy9uSTFzVDBLRzhCT3gK`
- Private-key material: `TAURI_SIGNING_PRIVATE_KEY` secret (CI) — the test
  needs nothing else.
- Procedure (one line + one check, run by Lucas, NOT in CI yet):
  1. Sign one throwaway artifact with the existing private key.
  2. Verify it with the new stack (`minisign-verify = "0.2"` is already a
     `parzi-desk` dependency for exactly this; `cargo packager signer
     generate` exists only to mint a REPLACEMENT if this fails).
  3. Pass = same keypair carries over, no reinstall. Fail = mint a new
     keypair and every existing user reinstalls once (accepted fallback per
     PLAN §7: "the current feed is dead anyway per AUDIT S-7").
- Ownership: another lane owns the automated test; this lane only defines the
  procedure. The RESULT blocks cutover approval (§3 item 3) — do not approve
  Phase 6 without it.

## 3. Cutover checklist — Phase 6 approval (all must be true)

1. [ ] Every `UNCERTAIN` key in `packager.toml` confirmed against the
       cargo-packager configuration reference and the marker removed.
2. [ ] WiX UpgradeCode carry-over resolved (native MSI upgrades existing
       installs; `NOTES.md` §4).
3. [ ] Minisign key-compat test executed by Lucas with a recorded pass/fail
       (§2); if fail, new keypair minted and the reinstall-once user note drafted.
4. [ ] M6 acceptance rerun green on the native artifacts: silent
       install/uninstall on a clean user account (`/S` NSIS, `/quiet` MSI)
       + update check succeeds against a TEST feed (never the live
       `latest.json` until approval).
5. [ ] Native DMG verified side-by-side with the Tauri DMG (§1), or an
       explicit mixed-ship decision (Windows cuts over, macOS stays).
6. [ ] `desk-release.yml` reviewed job-by-job, `if: false` guards lifted ONLY
       by cutover approval, secrets (`*_SIGNING_PRIVATE_KEY`) named and set.
7. [ ] Rollback named: if the native release misbehaves, the next tag ships
       from `release.yml` (Tauri) unchanged — keep it green and unmodified
       until at least one native release is confirmed in the wild.
8. [ ] M7 deletion (`ui/`, `src-tauri/`) happens only AFTER a confirmed
       native release — never in the same change as cutover.

## 4. Open follow-ups (not this lane)

- Installer art copies staged beside `packager.toml` (blocked on nothing,
  but binary art + five parallel lanes = do it in Phase 6, not now).
- Store-tile PNGs / `android/` / `ios/` icon dirs: drop decision at cutover.
- `format` semantics in the updater endpoint JSON (Phase 6 reference check).
- Version-placeholder mechanism (auto-read vs explicit vs env var) + extend
  the `ci.yml` versions-agree hygiene job to cover `packager.toml`.
