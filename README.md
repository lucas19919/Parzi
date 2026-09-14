# Parzi — lean agent harness (Rust + Tauri)

Sidebar, stage, glass omni-bar. Projects > lanes > threads. Five providers, one bar:
Claude, Codex, Antigravity, OpenCode, Grok — subscriptions first, keys only when you say so.

- `PLAN.md` — frozen architecture
- `LOOP.md` — how it gets built (gates, phases)
- `PROGRESS.md` — build evidence per phase

Quick start:

```powershell
cargo build -p parzi-cli
.\target\debug\parzi.exe init
.\target\debug\parzi.exe doctor
.\target\debug\parzi.exe send new "hello" --model auto --yes
```

GUI: `cd ui; npm install; npm run build`, then `cargo tauri dev` in `src-tauri`
(requires `cargo install tauri-cli --locked`).

Default background: Asuka (`assets/backgrounds/asuka.png`), seeded to
`~/.parzi/backgrounds/` on first run. Replace the file to make it yours.

License: MIT.
