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

## Open backend — plug other agents into Parzi

Parzi is a harness other harnesses can drive. Three surfaces, same state
under `~/.parzi`:

**MCP server** (richest — 19 tools: sessions, workspaces, deck projects,
knowledge, plans, diagnostics, plus `report_issue` so agents file Parzi bugs
themselves):

```json
{ "mcpServers": { "parzi": { "command": "parzi", "args": ["mcp"] } } }
```

`sessions` run to completion and return the transcript tail plus usage;
`sends` auto-approve (the transport is non-interactive). Destructive tools
(`workspace_delete`, `project_delete`) kill the affected runs first and say
how many sessions went with them.

**CLI** (scripts, pipes, other harnesses):

```powershell
.\target\debug\parzi.exe send new "hello" --model auto --yes
.\target\debug\parzi.exe export <id>          # transcript to stdout
.\target\debug\parzi.exe report-issue "title" "what happened, what you expected"
```

**Files** (no API at all): sessions, transcripts (`session.md`), `PROJECT.md` /
`PLAN.md` grammar, `workspace.toml`, `KNOWLEDGE.md` — all plain text under
`~/.parzi`, readable and writable by anything.

## Standing instructions (Claude-style memory, Parzi-shaped)

Three scopes, auto-loaded into context, capped at 8 KB each. Missing files
are normal — most scopes have none:

| Scope | File | Applies to |
| --- | --- | --- |
| You, everywhere | `~/.parzi/SYSTEM.md` | every chat and role run |
| Workspace | `~/.parzi/workspaces/<name>/SYSTEM.md` | chats in that workspace, its header/orchestrator runs |
| Project / lane (legacy) | `projects/<name>/SYSTEM.md`, `lanes/<l>/SYSTEM.md` | chats via the existing scan pass |

Keep them short (under ~200 lines); longer files cap with a note. Deck
projects carry their goal in `PROJECT.md` instead. Coders stay task-scoped
on purpose — instructions stop at header/orchestrator level.

License: MIT.
