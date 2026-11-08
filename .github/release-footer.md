### Download

| Your machine | Pick this file |
| --- | --- |
| Windows 10 / 11, 64-bit | `Parzi-Setup.exe`: one click, no admin needed, opens Parzi when it's done |
| macOS, Apple Silicon | `Parzi_*_aarch64.dmg`: drag Parzi into Applications |
| Command line, not the app | `parzi-cli-windows-x64.exe` or `parzi-cli-macos-arm64`: drives sessions and tabs from scripts |
| Your own Linux server (ParziOS) | `parzi-linux-x64` or `parzi-linux-arm64`: the desktop's *Set up remote* installs it for you |

`Parzi_*_x64-setup.exe` and `Parzi*.app.tar.gz` are the files the in-app updater downloads. You don't need them.

### First launch (once per machine)

- **Windows:** SmartScreen asks once because setup isn't Authenticode-signed yet: *More info*, then *Run anyway*.
- **macOS:** Gatekeeper blocks the first open because the build isn't Apple-notarized: open Parzi once, then *System Settings*, *Privacy & Security*, *Open Anyway*. Afterwards it launches normally.

### Updates

Parzi checks for new versions on launch and offers them in the app, no manual reinstalls. Turn on *Settings, General, Dev builds* to get dev builds too.

> Your threads and settings live in `~/.parzi` and are never touched by updates or reinstalls.
