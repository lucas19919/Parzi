# Parzi — lean agent harness (Rust + Tauri)

Tabs, a composer, and six agents behind one bar: Claude Code, Codex, OpenCode,
Grok, Antigravity and Cursor. Parzi drives each vendor's own agent on your own
sign-in; Parzi's tools reach the agent over MCP, and every action the agent
asks about passes Parzi's approval gate. Which agents ask about every change is
in the table under [Agents](#agents).

## Install

Grab the latest release from
[GitHub Releases](https://github.com/lucas19919/Parzi/releases):

| Your machine | Pick this file |
| --- | --- |
| Windows 10 / 11, 64-bit (recommended) | `Parzi_*_x64-setup.exe` — per-user install, no admin needed |
| Windows, system-wide | `Parzi_*_x64_en-US.msi` — needs admin |
| macOS, Apple Silicon or Intel | `Parzi_*_universal.dmg` — drag Parzi into Applications |

First launch asks once: on Windows SmartScreen wants
*More info → Run anyway*. On macOS (builds are not Apple-notarized), run
`xattr -d com.apple.quarantine /Applications/Parzi.app` once, or use
*System Settings › Privacy & Security › Open Anyway*. Afterwards it updates
itself in-app. Threads and settings live in `~/.parzi` and survive updates.

From source:

```powershell
cargo build -p parzi-cli
.\target\debug\parzi.exe init
.\target\debug\parzi.exe doctor
.\target\debug\parzi.exe session send new "hello" --model auto --yes
```

GUI: `cd ui; npm install; npm run build`, then `cargo tauri dev` in `src-tauri`
(requires `cargo install tauri-cli --locked`).

## Using it

- **Tabs** hold a session or a web page, and come back after a restart. Drag a
  tab to reorder it (`Ctrl+Shift+←/→` on a focused tab). `Ctrl+T` new tab
  (Home, cursor in the composer), `Ctrl+W` close, `Ctrl+Shift+T` reopen the
  last closed page, `Ctrl+Tab` cycle, `Ctrl+H` history and bookmarks, `Ctrl+P`
  search sessions, tabs and actions, `Ctrl+,` settings. These work while a page
  has focus too.
- **Home** is where a new tab lands: the composer, pinned sites (pin any page
  from its toolbar; until you do, your most visited sites show), and recent
  sessions.
- **Set up Parzi** (first launch, or the menu) signs you in to each agent with
  its own login, imports Brave/Chrome/Edge bookmarks and history (never
  passwords or cookies), and turns your other agents' instructions, memories
  and skills into notes and the folders you worked in into projects.
- **Brain** (`Ctrl+B`) manages what context your agents start with. Notes are
  plain markdown in `~/.parzi/brain` (open it in Obsidian or any editor; edits
  show up when you come back to Parzi). A note goes to **All sessions** or to
  any number of projects (frontmatter `projects: [all, parzi]`, or a link to
  the project note); a project is a note with a `folder:`. Pinned notes
  (`pinned: true`) are sent in full; every other note is listed with a
  one-line summary (`description:` or its first line), and the agent reads it
  with `brain_read` when the task needs it. Full text is capped at about 12k
  characters. In the Brain tab these scopes are folders: drag a note onto one
  to move it, Ctrl+drag to copy it. Clicking a folder shows its token cost and
  exactly what the agent gets.
  There is no embedding index: search is plain text, and links are the graph.
- **Pages** keep their state when you switch tabs, and tabs restored at
  startup load in the background. The address bar (and the composer in Web
  mode) completes sites you visited or bookmarked as you type, and shows a
  list like Chrome's: the search itself, DuckDuckGo suggestions (never for
  text that looks like an address), then matching history and bookmarks;
  arrows to move, Enter to go, Esc to close. `Ctrl+L`
  address bar, `Alt+←/→` back and forward, `Ctrl+R` reload, `F11` full
  screen. Links that open a new window open a new tab.
- **Passwords** come from Bitwarden; Parzi stores none. Install the CLI
  (`winget install Bitwarden.CLI`), then Settings › General › Passwords signs
  in or unlocks in Bitwarden's own prompt, so Parzi never sees the master
  password. The key in the page toolbar fills a login for the site you are
  on, only on https and only when you click it.
- **Ad blocker**: Brave's engine with EasyList and EasyPrivacy, downloaded to
  `~/.parzi/cache/adblock` and refreshed every four days. The shield in the
  page toolbar shows how many requests it blocked; click it to allow ads on
  that site. Settings › General turns it off.
- **Composer**: `Tab` switches between Agent and Web. In Agent mode pick the
  model, effort, permissions and the project the agent works on (pick one,
  start a new one from a folder, or turn the current folder into one); `@`
  lists files in the project's folder, `/` lists commands (`/new`, `/fork`,
  `/compact`, `/stop`, `/model`, `/effort`, `/page`, `/settings`). `Esc` stops
  a run.
- A session without a project runs in its own empty scratch folder.
- Drop any image into `~/.parzi/backgrounds/` and pick it under
  Settings › Appearance.

## Agents

Install an agent and sign in with its own program; Parzi picks it up. It never
stores keys or tokens.

| Agent | Program | Parzi talks to it over | Asks Parzi before every change |
| --- | --- | --- | --- |
| Claude Code | `claude` | the Agent SDK's stdio control protocol | yes |
| Codex | `codex` | `codex app-server` (JSON-RPC) | yes |
| OpenCode | `opencode` | ACP (`opencode acp`) | yes |
| Grok | `grok` | ACP (`grok agent stdio`) | no |
| Antigravity | `agy_acp_server` (T3 Code installs it) | ACP | no |
| Cursor | `cursor-agent` | ACP (`cursor-agent acp`) | no |

Parzi starts every agent in its most-asking mode and answers for the session
itself: Supervised, Auto-accept edits, Auto or Full access. Claude Code runs
without user or project settings, so no permission rule or hook in them answers
before Parzi; Parzi hands it the repo's `CLAUDE.md` files itself. Codex runs
with approval on every action and a read-only sandbox. OpenCode asks on every
edit, command and fetch and ignores the repo's `opencode.json`; it still reads
`AGENTS.md`. An agent marked *no* can change files without asking: Settings and
`parzi providers` say so, and its thread says so once.

`parzi providers` (or Settings › Providers) asks each program where it stands:
installed, signed in, plan usage, models and their effort levels. It spends no
quota. Smart Auto starts a new thread on the first ready agent in your order; a
started thread stays with its agent. A turn that fails says so in the thread,
in the vendor's own words.

An agent may read anywhere, but a write outside the session's folder is never
approved on your behalf: you are asked, and a run with nobody to ask is
refused. The desktop app logs to `~/.parzi/logs/parzi-<date>.log`.

## CLI

When the window is open the CLI drives it (it finds the window through
`~/.parzi/gui.json`); otherwise session commands work on the files directly.

```powershell
parzi session list
parzi session send new "hello" --model claude --cwd C:\src\app   # asks before tools
parzi session send <id> "and now the tests" --yes                # approves every tool call
parzi session export <id>                                        # transcript to stdout
parzi tab open github.com                                        # opens a page tab
parzi providers --json
```

Sessions are plain files under `~/.parzi/sessions/<id>/` (`meta.json`,
append-only `events.jsonl`, rendered `session.md`), readable by anything.

## Standing instructions and hooks

`~/.parzi/SYSTEM.md` is loaded into every chat (capped at 8 KB; a missing file
is normal).

`~/.parzi/hooks.toml` runs your scripts around tool calls:

```toml
[[pre_tool]]
match = "shell.exec"          # exact, family prefix "fs.*", or "*"
command = "python guard.py"
timeout_secs = 5              # default 5, max 120
```

`pre_tool` hooks run before the approval gate with the event JSON on stdin:
exit 0 allows, exit 2 denies (stderr is the reason), anything else allows with
a warning. `post_tool` hooks are notify-only. Children get a scrubbed
environment (no provider keys) plus `PARZI_SESSION/TOOL/EVENT`.

License: MIT.
