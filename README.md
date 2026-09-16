# Parzi — lean agent harness (Rust + Tauri)

Sidebar, stage, glass omni-bar. Projects > lanes > threads. Five providers, one bar:
Claude, Codex, Antigravity, OpenCode, Grok — subscriptions first, keys only when you say so.

- `PLAN.md` — frozen architecture
- `LOOP.md` — how it gets built (gates, phases)
- `PROGRESS.md` — build evidence per phase

## Install

Grab the latest release from
[GitHub Releases](https://github.com/lucas19919/Parzi/releases):

| Your machine | Pick this file |
| --- | --- |
| Windows 10 / 11, 64-bit (recommended) | `Parzi_*_x64-setup.exe` — per-user install, no admin needed |
| Windows, system-wide | `Parzi_*_x64_en-US.msi` — needs admin |
| macOS, Apple Silicon or Intel | `Parzi_*_universal.dmg` — drag Parzi into Applications |

First launch asks once: on Windows SmartScreen wants
*More info → Run anyway*. On macOS (since builds are not Apple-notarized), Gatekeeper
asks once: open Terminal and run `xattr -d com.apple.quarantine /Applications/Parzi.app`
(or go to *System Settings › Privacy & Security › Open Anyway*). Afterwards it
launches normally and updates itself in-app. Your threads and settings live in
`~/.parzi` and are never touched by updates or reinstalls.

Quick start (from source):

```powershell
cargo build -p parzi-cli
.\target\debug\parzi.exe init
.\target\debug\parzi.exe doctor
.\target\debug\parzi.exe send new "hello" --model auto --yes
```

GUI: `cd ui; npm install; npm run build`, then `cargo tauri dev` in `src-tauri`
(requires `cargo install tauri-cli --locked`).

Default background: none (a solid stage). Drop any image into
`~/.parzi/backgrounds/` and pick it under Settings › Appearance.

License: MIT.
