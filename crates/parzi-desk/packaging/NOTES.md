# parzi-desk packaging research notes (Phase 5 — research only, nothing wired up)

Ground truth sources (read, not modified):
- `src-tauri/tauri.conf.json` (productName `Parzi`, version `0.1.11`,
  identifier `com.parzi.app`)
- `src-tauri/installer/hooks.nsh`, `src-tauri/installer/English.nsh`
- `.github/workflows/release.yml` (live Tauri pipeline — DO NOT TOUCH)
- `docs/native-ui/PLAN.md` §0 packaging paragraph, M6 row, §7 updater-key row

Application metadata (verbatim from `tauri.conf.json`, to mirror in `packager.toml`):
- productName `Parzi` · identifier `com.parzi.app` · publisher `Parzi`
- copyright `Copyright © 2026 Parzi` · homepage `https://github.com/lucas19919/Parzi`
- licenseFile `../LICENSE` (relative to `src-tauri/` = repo-root `LICENSE`)
- category `DeveloperTool`
- shortDescription `Lean multi-model agent harness`
- longDescription `Parzi routes Claude, Codex, Antigravity, OpenCode and Grok through one elegant harness.`
- version: workspace `0.1.11` (`Cargo.toml` `[workspace.package]`;
  `parzi-desk` uses `version.workspace = true`). The packager draft must
  track the workspace version, not a hardcoded copy (see `packager.toml`).

## 1. Exact asset map — `src-tauri/installer/` (do not modify originals)

| File | Size (bytes) | What it is / used for |
|---|---|---|
| `installer/banner.bmp` | 114430 | WiX top banner (`bundle.windows.wix.bannerPath`). Shown across the top of MSI wizard pages. |
| `installer/dialog.bmp` | 615318 | WiX dialog background (`bundle.windows.wix.dialogImagePath`). Left-side image on MSI welcome/completion pages. |
| `installer/header.bmp` | 34254 | NSIS wizard header (`nsis.headerImage`) AND uninstaller header (`nsis.uninstallerHeaderImage`). Top-right strip on every NSIS page. |
| `installer/sidebar.bmp` | 206038 | NSIS welcome/finish sidebar (`nsis.sidebarImage`). Tall left image on first/last NSIS pages. |
| `installer/hooks.nsh` | 3011 | NSIS template hooks (`nsis.installerHooks`). Included BEFORE any `MUI_PAGE_*` macro: dark palette (`MUI_BGCOLOR` / `MUI_HEADERIMAGE_BGCOLOR` = `0B0B10`, text `0xEDEDF2`), dark title bar via `DwmSetWindowAttribute` (attribute 20, Win10 1809+), `ParziDarkShow` as `MUI_PAGE_CUSTOMFUNCTION_SHOW` for every installer page, plus all brand-voice strings (welcome / MIT license top+bottom / directory / start-menu / instfiles finish header / finish + `Launch Parzi now` run text / uninstall-confirm top / abort warning). Port all of this, not just the images. |
| `installer/English.nsh` | 1018 | NSIS string overrides (`nsis.customLanguageFiles.English`). Included AFTER stock `English.nsh`; 8 `LangString`s: `alreadyInstalledLong`, `appRunning`, `appRunningOkKill`, `createDesktop` ("Put Parzi on my desktop"), `deleteAppData` ("Also delete my threads, settings and local data"), `failedToKillApp`, `unableToUninstall`, `uninstallApp`, `uninstallBeforeInstalling`. Keep every `$var` (`${PRODUCTNAME}`, `${VERSION}`, `$\n`) intact when porting. |

Porting rule: the native packager config must reference COPIES of the `.bmp`
files staged for `parzi-desk` (e.g. under `crates/parzi-desk/packaging/` or a
shared assets dir) — never point at `src-tauri/installer/` (that tree is
deleted at M7 cutover, PLAN M7). Copies are not made yet (Phase 5 is
research + config draft; another lane may own the binary art).

## 2. Icon set — `src-tauri/icons/` (do not modify originals)

`tauri.conf.json` `bundle.icon` uses exactly these five (verified list):
- `icons/32x32.png` (993 B), `icons/128x128.png` (3892 B),
  `icons/128x128@2x.png` (7998 B) — Linux/macOS PNG set
  (`64x64.png` exists in the dir but is NOT in the bundle list)
- `icons/icon.icns` (93928 B) — macOS bundle/DMG icon
- `icons/icon.ico` (19579 B) — Windows exe + NSIS `installerIcon` /
  `uninstallerIcon` (also embedded at compile time via `winres` in
  `crates/parzi-desk/Cargo.toml` `[build-dependencies]`)

Also present but NOT referenced by the bundle (kept for completeness):
`icon.png` (15691 B), `icon.svg`, `StoreLogo.png`, `Square*.png` (MS Store
tile logos), `android/`, `ios/` dirs. The native config needs at minimum the
`.ico` (Windows) and `.icns` (macOS coexistence, see
`docs/native-ui/packaging.md`); decide in Phase 6 whether the Store tiles
and mobile dirs carry over (expected: no — desktop-only ship).

## 3. NSIS per-user settings to replicate (verbatim from `tauri.conf.json`)

- `installMode: "currentUser"` — per-user install, no admin (release body:
  "`Parzi_*_x64-setup.exe` — per-user install, no admin needed").
- `languages: ["English"]` + `customLanguageFiles: { English:
  "installer/English.nsh" }`.
- `installerHooks: "installer/hooks.nsh"` (dark theme + brand voice, §1).
- `headerImage` + `uninstallerHeaderImage`: `header.bmp`;
  `sidebarImage`: `sidebar.bmp`.
- `installerIcon` + `uninstallerIcon`: `icons/icon.ico`.
- `startMenuFolder: "Parzi"`.
- PLAN §0 adds the cargo-packager NSIS surface to verify against real docs
  in Phase 6: `installMode: currentUser`, `headerImage`, `sidebarImage`,
  custom `template`, `preinstallSection` — confirm each key name in the
  cargo-packager reference before relying on `packager.toml` (§5 UNCERTAIN
  list).

Release artifacts today (Windows): `Parzi_*_x64-setup.exe` (NSIS, recommended)
+ `Parzi_*_x64_en-US.msi` (system-wide, needs admin). Replicate both targets.

## 4. WiX settings (verbatim from `tauri.conf.json`)

- `language: "en-US"` (matches the `_en-US.msi` artifact suffix).
- `bannerPath: "installer/banner.bmp"`, `dialogImagePath:
  "installer/dialog.bmp"`.
- No other WiX keys are set (no `upgradeCode`, no custom `template`/`fragment`
  paths) — Tauri generates defaults. Phase 6 must decide how the MSI
  UpgradeCode is carried over so the native MSI upgrades (not side-by-sides)
  existing installs; record the code from a built MSI or Tauri internals
  before cutover. No silent-install flags are configured in-repo; M6
  acceptance ("silent install/uninstall on a clean user account") is tested
  with stock `/S` (NSIS) / `/quiet` (MSI) at packaging time.

## 5. Updater design (live design today → native mapping)

Live design today:
- `bundle.createUpdaterArtifacts: true`; `plugins.updater`: `active: true`,
  `dialog: false` (in-app offer, no Tauri dialog), single endpoint
  `https://github.com/lucas19919/Parzi/releases/latest/download/latest.json`.
- Pubkey (minisign, in `tauri.conf.json` `plugins.updater.pubkey`):
  `dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDIwMUY2NjI5RERGRDQ4RTYKUldUbVNQM2RLV1lmSUlTZndQeE8ybmZVT0FkS2VuZjRyZlFacmtIa3cxdy9uSTFzVDBLRzhCT3gK`
- Signing: `TAURI_SIGNING_PRIVATE_KEY` secret → tauri-action signs updater
  artifacts → DRAFT release (`releaseDraft: true`, `prerelease: false`,
  tag `v*` + `workflow_dispatch`) carrying `latest.json`.
- Client: `tauri-plugin-updater` (`src-tauri/Cargo.toml` = `"2"`,
  capability `updater:default` in `capabilities/default.json`, registered in
  `src/main.rs`); data dir `~/.parzi` untouched by updates (release body).

Native mapping (PLAN §0: `cargo-packager-updater` verifies minisign via
`minisign-verify`; keys via `cargo packager signer generate`; endpoint JSON
`{version, url|platforms, signature, format}` — same scheme family as Tauri):
- `latest.json` stays the feed URL (same endpoint; feeds are compatible as
  long as the JSON shape matches what the client parses).
- Tauri's per-artifact `.sig` files map to the `signature` field(s) of the
  updater endpoint JSON consumed by `cargo-packager-updater` (per-platform
  entries under `platforms` where the new client needs per-OS URLs; single
  `url` where it does not — confirm the exact shape in the updater reference
  in Phase 6).
- `format` field: confirm semantics (expected: archive format of the update
  payload) in Phase 6; do not guess.
- Verification dependency is already in-tree: `crates/parzi-desk/Cargo.toml`
  has `minisign-verify = "0.2"` explicitly for the Phase 0 compat test.
- PLAN §7 row (updater key): signal = M6 test; fallback = new keypair, users
  reinstall once ("the current feed is dead anyway per AUDIT S-7"). The
  one-line test (does the existing Tauri key verify unchanged under
  `cargo-packager-updater`?) is REQUIRED before cutover — procedure in
  `docs/native-ui/packaging.md`; execution is deferred to Lucas, another lane
  owns the automated test.

## 6. What Phase 6 must still verify (handover)

1. Every `UNCERTAIN` key in `packager.toml` against the cargo-packager
   configuration reference (`https://docs.crabnebula.dev/packager/configuration/`
   per PLAN sources) — schema names first, NSIS/WiX tables second.
2. WiX UpgradeCode carry-over (§4).
3. Minisign key-compat test result (blocks cutover approval).
4. macOS DMG parity (window 660×400 per `bundle.macOS.dmg.windowSize`,
   `minimumSystemVersion 11.0`, unsigned universal dmg) — coexistence plan in
   `docs/native-ui/packaging.md`.
5. M6 acceptance rerun: silent install/uninstall on a clean user account +
   update check against a test feed.
