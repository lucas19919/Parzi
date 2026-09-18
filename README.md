# Parzi — lean agent harness (Rust + Tauri)

Sidebar, stage, glass omni-bar. Projects > lanes > threads. Six agents, one bar:
Claude Code, Codex, OpenCode, Grok, Antigravity and Cursor. Parzi drives each
vendor's own agent on your own sign-in; Parzi's tools reach the agent over MCP,
and every action the agent takes passes Parzi's approval gate.

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

## Agents

Install an agent and sign in with its own program; Parzi picks it up. It
never stores keys or tokens.

| Agent | Program | Parzi talks to it over |
| --- | --- | --- |
| Claude Code | `claude` | the Agent SDK's stdio control protocol |
| Codex | `codex` | `codex app-server` (JSON-RPC) |
| OpenCode | `opencode` | ACP (`opencode acp`) |
| Grok | `grok` | ACP (`grok agent stdio`) |
| Antigravity | `agy_acp_server` (T3 Code installs it) | ACP |
| Cursor | `cursor-agent` | ACP (`cursor-agent acp`) |

`parzi providers` (or Settings › Providers) asks each program where it
stands: installed, signed in, plan usage, models and their effort levels. It
spends no quota. Smart Auto starts a new thread on the first ready agent in
your order; a started thread stays with its agent. A turn that fails says so
in the thread, in the vendor's own words.

An agent may read anywhere, but a write outside the thread's folder, or to a
file it does not name, is never approved on your behalf: you are asked, and a
run with nobody to ask is refused. The desktop app logs to
`~/.parzi/logs/parzi-<date>.log`.

Default background: none (a solid stage). Drop any image into
`~/.parzi/backgrounds/` and pick it under Settings › Appearance.

## Open backend — plug other agents into Parzi

Parzi is a harness other harnesses can drive. Three surfaces, same state
under `~/.parzi`:

**MCP server** (richest — 22 tools: sessions, workspaces, deck projects,
knowledge, plans, diagnostics, workspace sync/remote/clone, plus
`report_issue` so agents file Parzi bugs themselves):

```json
{ "mcpServers": { "parzi": { "command": "parzi", "args": ["mcp"] } } }
```

`sessions` run to completion and return the transcript tail plus usage;
`sends` auto-approve (the transport is non-interactive). Destructive tools
(`workspace_delete`, `project_delete`) kill the affected runs first and say
how many sessions went with them.

Remote VMs need no new protocol: `ssh user@vm parzi mcp` *is* an MCP
server entry, and the workspace's git remote moves the work:

```powershell
parzi workspace remote acme git@github.com:you/acme-workspace.git
parzi workspace sync acme          # here
ssh vm parzi workspace clone git@github.com:you/acme-workspace.git acme
ssh vm parzi workspace sync acme   # there, from now on
```

See `docs/remote-control.md`.

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

## Path-scoped rules

Domain knowledge that loads only when relevant: `<workspace>/rules/*.md`
(and `projects/<name>/rules/*.md` for legacy projects). A `paths:` frontmatter
picks the files it governs; attaching one pulls the body into that run:

```markdown
---
paths:
  - "apps/web/**"
  - "*.tsx"
---
Use the shared Button; never raw <button>.
```

A pattern without a `/` also matches the bare file name; no `paths:` means
always applies. Bodies cap at 8 KB, 32 files per directory, sorted by name.

## Workspace composer defaults

A workspace can set the composer's starting pick for new drafts in
`workspace.toml`:

```toml
[defaults]
model = "claude/opus"   # agent/model, or "auto"
effort = "high"          # low | medium | high | extra | ultra
```

Empty (or absent) means no opinion. Switching into the workspace adopts
them, but only where the composer is still on app defaults (`auto` /
`medium`) — an explicit pick is never clobbered. Permission modes stay
global for now.

## Workspace policy: enforced permissions

`workspace.toml` can floor what runs are allowed to do:

```toml
[policy]
mode = "ask"   # ask | auto | deny; unset = the lane policy decides
```

`deny` is absolute lockdown (nothing lifts it, not even Full access);
`ask` floors `auto`. The composer's permission pill
(Supervised/Edits → ask, Auto/Full → auto, Edits pre-approves file writes)
tightens, never lifts. Applies to chats, MCP sends, and deck role runs alike.

## Workspace hooks: user scripts watching tools

`~/.parzi/hooks.toml` (global) and `workspaces/<name>/hooks.toml`:

```toml
[[pre_tool]]
match = "shell.exec"          # exact, family prefix "fs.*", or "*"
command = "python .parzi/guard.py"
timeout_secs = 5              # default 5, max 120
```

`pre_tool` hooks run before the approval gate with the event JSON on stdin:
exit 0 allows, exit 2 denies (stderr is the reason), anything else allows
with a warning. `post_tool` hooks are notify-only. Children get a scrubbed
environment (no provider keys) plus `PARZI_SESSION/PROJECT/TOOL/EVENT`.

License: MIT.
