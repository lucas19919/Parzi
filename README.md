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
| macOS, Apple Silicon | `Parzi_*_aarch64.dmg` — drag Parzi into Applications |
| Your own Linux server | nothing to download: *Set up remote* installs `parzi-linux-x64` / `-arm64` there ([ParziOS](#parzios-your-own-server)) |

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

UI without the app: `npm run dev` in `ui`, then open
`http://localhost:1420/mock.html` (a fake backend; `?view=home|session|brain|history|settings|setup`,
`&turns=300` for a long thread, `&live=1` for agents at work, `&plan=none` for a project without a plan). With that running, `npm run lint:ui` checks every view and its
menus against `ui/ui-policy.json` with [lintapple](https://github.com/gholtzap/lintapple): the type
scale, contrast, control size and spacing, labels and tooltips. Exceptions in the policy carry
their reason.

## Using it

- **Tabs** hold a session, a web page, the Brain or History, and come back
  after a restart. Drag a tab to reorder it (`Ctrl+Shift+←/→` on a focused
  tab). `Ctrl+T` new tab (Home, cursor in the composer), `Ctrl+W` close,
  `Ctrl+Shift+T` reopen the last closed page, `Ctrl+Tab` cycle, `Ctrl+H`
  history and bookmarks, `Ctrl+P` search sessions, tabs and actions, `Ctrl+,`
  settings. These work while a page has focus too.
- **Home** is where a new tab lands: just the composer. The project tab on
  its top edge picks the folder a session works in; with a folder the agent
  can build there, without one it is a plain assistant. **Agent** and **Web**
  under the box choose who answers: the agent, or the web (a search, or an
  address to open).
- **The session column** (`Ctrl+J`) sits beside a session: where it runs,
  how full its context is (with Compact), what changed in its folder and its
  PR, and the helpers it started.
- **Plan**: agents write a project's plan (ask one to "plan the next
  release") and keep it current. A session working on it shows the plan as a
  checklist at the end of the thread: each item reads as working, needing
  you, in review, started or not started, and done items fold away. Click an
  item to quote it into the composer, then say what you want ("start this",
  "split this"). The plan is a note in the Brain (`projects/<slug>/plan.md`),
  so it syncs like the rest. Every session keeps how its latest run went: what
  it changed, the checks it ran, what it cost.
- **Providers** (Settings) lists every agent with what it needs next: Install
  or Sign in right on its row, checked again on its own.
- **Connectors** (Settings, Connections) give your agents more tools. Click
  one (GitHub, Higgsfield, Notion, Linear, Sentry, Stripe, Hugging Face) and
  sign in to the service in your browser; GitHub takes a personal access
  token instead. **Other** adds any hosted MCP server by its address, or a
  local one by its command. Hosted ones run through
  [mcp-remote](https://github.com/geelen/mcp-remote), so they need Node.js.
- **Set up Parzi** (first launch, or the menu) signs you in to each agent with
  its own login, imports Brave/Chrome/Edge bookmarks and history (never
  passwords or cookies), and turns your other agents' instructions, memories
  and skills into notes and the folders you worked in into projects.
- **Brain** (`Ctrl+B`) manages what context your agents start with. Notes are
  plain markdown in `~/.parzi/brain` (open it in Obsidian or any editor; edits
  show up when you come back to Parzi). A note goes to **All sessions** or to
  any number of projects (frontmatter `projects: [all, parzi]`, or a link to
  the project note); a project is a note with a folder (`folder@<device>:`,
  one per machine; see [ParziOS](#parzios-your-own-server)). Pinned notes
  (`pinned: true`) are sent in full; every other note is listed with a
  one-line summary (`description:` or its first line), and the agent reads it
  with `brain_read` when the task needs it. Full text is capped at about 12k
  characters. In the Brain tab these scopes are folders: drag a note onto one
  to move it, Ctrl+drag to copy it, and click the pin to send a note in full.
  There is no embedding index: search is plain text, and links are the graph.
- **Pages** keep their state when you switch tabs, and tabs restored at
  startup load in the background. The address bar (and the composer in Web
  mode) completes sites you visited or bookmarked as you type, and shows a
  list like Chrome's: the search itself, DuckDuckGo suggestions (never for
  text that looks like an address), then matching history and bookmarks;
  arrows to move, Enter to go, Esc to close. `Ctrl+L`
  address bar, `Alt+←/→` back and forward, `Ctrl+R` reload, `F11` full
  screen. Links that open a new window open a new tab (behind, for an
  agent's page); sign-in pop-ups open as windows, and pop-ups nobody clicked
  for are blocked. Links for other apps (`mailto:`, Zoom, VS Code) ask
  first. `Ctrl+B`, `Ctrl+H` and `Ctrl+K` go to the page first and reach
  Parzi only when the page doesn't use them; `Ctrl+P` prints. Hidden tabs
  give memory back after a while, and `Ctrl+Shift+T` reopens closed pages,
  also after a restart.
- **Downloads** go to your Downloads folder, with progress in a toast and a
  list under History › Downloads. A site asking for the camera, microphone,
  location and the like asks in a bar above the page; Settings › General
  lists and forgets those choices, turns on developer tools for pages, and
  decides whether pages an agent opens share your sign-ins (off: they get
  their own profile).
- **Passwords** come from Bitwarden; Parzi stores none. Install the CLI
  (`winget install Bitwarden.CLI`), then Settings › General › Passwords signs
  in or unlocks in Bitwarden's own prompt, so Parzi never sees the master
  password. The key in the page toolbar fills a login for the site you are
  on, only on https and only when you click it.
- **Ad blocker**: Brave's engine with EasyList and EasyPrivacy, downloaded to
  `~/.parzi/cache/adblock` and refreshed every four days; it blocks ad and
  tracker requests and hides the lists' ad boxes on the page. The shield in the
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

Set up Parzi installs each agent with its vendor's official installer, in a
terminal you can watch, and signs you in with the agent's own login; Parzi
picks it up when it's done. It never stores keys or tokens.

| Agent | Program | Parzi talks to it over | Asks Parzi before every change |
| --- | --- | --- | --- |
| Claude Code | `claude` | the Agent SDK's stdio control protocol | yes |
| Codex | `codex` | `codex app-server` (JSON-RPC) | yes |
| OpenCode | `opencode` | ACP (`opencode acp`) | yes |
| Grok | `grok` | ACP (`grok agent stdio`) | no |
| Antigravity | `agy_acp_server` (Google's build, kept in `~/.parzi/agents`) | ACP | no |
| Cursor | `cursor-agent` | ACP (`cursor-agent acp`) | no |

Antigravity signs in with Google in your browser. Its first start takes about a
minute, so Parzi keeps it running for 15 minutes after a turn.

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
quota. New threads use the default model picked in Settings › Providers; a
started thread stays with its agent. A turn that fails says so in the thread,
in the vendor's own words.

An agent may read anywhere except Parzi's and the agents' own token files
(see CLI below), but a write outside the session's folder is never
approved on your behalf: you are asked, and a run with nobody to ask is
refused. The desktop app logs to `~/.parzi/logs/parzi-<date>.log`.

## CLI

When the window is open the CLI drives it: it finds the window through
`~/.parzi/gui.json` and checks that the window itself answers before it
sends the token (a file left by a crash is removed). Otherwise session
commands go to `parzi serve` if it runs, or work on the files directly.

```powershell
parzi session list
parzi session send new "hello" --model claude --cwd C:\src\app   # streams the reply; Ctrl-C stops the run
parzi session send <id> "and now the tests" --yes                # approves every tool call
parzi session export <id>                                        # transcript to stdout
parzi approval list                                              # tool calls waiting for a yes
parzi approval allow 1a2b                                        # a key prefix is enough
parzi watch                                                      # runs, approvals, tabs as they happen
parzi state                                                      # tabs, folder, running sessions
parzi open session <id>                                          # also: settings providers, brain <note>, history, plan, home
parzi notify "deploy is green"
parzi screenshot                                                 # the window, to ~/.parzi/shots/cli/
parzi providers --json
parzi doctor                                                     # also checks the window and parzi serve
```

Page tabs, from the command line. Each works on the tab in front unless
`--tab <id>` names one (`parzi tab list` shows ids):

```powershell
parzi tab open github.com                       # --background keeps your current tab
parzi tab read --offset 4000                    # text in pages, plus the controls
parzi tab click "Sign in"                       # or --selector "#login"
parzi tab type "rust tauri" --field Search --submit
parzi tab press Escape                          # Enter, Tab, ArrowDown, ctrl+a ...
parzi tab select Blue --field Color
parzi tab scroll bottom
parzi tab wait --text "Results" --timeout 20
parzi tab shot --out page.jpg
```

Some commands wait for you in the window, whoever holds the token:
`--yes` (a run that approves its own shell commands and file writes),
`session delete`, and reading or driving a page tab that no agent opened
(you can allow that for 10 minutes at a time). The CLI says when it is
waiting. Agents cannot open `gui.json`, `serve.json`, `config.toml`, the
server's SSH key or the agents' own sign-in files with their file tools,
and shell commands that name them are refused.

Sessions are plain files under `~/.parzi/sessions/<id>/` (`meta.json`,
append-only `events.jsonl`, rendered `session.md`), readable by anything.

## ParziOS: your own server

ParziOS is Parzi's engine running on a Linux machine you own (a VPS, a
home server, a GPU box). It becomes the home of your brain, settings and
sessions: every device you link to it (this PC, a laptop) stays in step
with it, and sessions there keep going while your machines sleep. It opens
no port: each device reaches it with your own `ssh`, and nothing else.

**Set up remote** (the last step of *Set up Parzi*) asks for `user`,
`host` (`host:port` works) and, only if the server takes no key, a
password. Parzi then:

1. connects with your SSH keys, or with the password once. With a
   password it adds its own key (`~/.parzi/ssh/id_ed25519`) to that
   account so it can reconnect without one; the password is never stored;
2. checks the machine (Linux x64 or arm64, curl or wget, systemd);
3. puts the matching `parzi` in `~/.local/bin`, downloaded from this
   repo's release and checked against its `.sha256`;
4. runs `parzi setup --enable` there: a systemd user service
   (`parzi-os.service`), or a detached engine when the machine has no
   user systemd. The service stops at logout unless lingering is on
   (`sudo loginctl enable-linger $USER`, setup says so);
5. links back, remembers the remote in `~/.parzi/remote.json`, and syncs.

Set up a second device against the same server and it joins: the server's
settings win, and the notes merge.

**What syncs** (on link, every two minutes, and a few seconds after any
change here):

- **Brain notes**, both ways. A note changed on one side moves to the
  other; one deleted on one side and untouched on the other is deleted.
  A note changed on two sides at once keeps the server's text under its
  name and this device's beside it as `<name>.conflict-<device>.md`.
- **Projects**: the notes are shared; the folder is per device. A project
  note holds one `folder@<device>` line per machine, so the PC, the
  laptop and the server each run it in their own folder. A project from
  another device lists here with no folder until you pick one.
- **Settings**: the newest change wins. Connectors and agent program
  paths stay per machine.
- **Sessions**: sessions on the server are the same on every device.
  Sessions run on a PC are mirrored to the server, so every device sees
  them (named with the device they ran on); sending to one of those
  continues it on the server.

The composer has a **Remote** switch (the server icon next to the
permissions), on by default once a server is linked. With it on, a new
session runs on the server, in a project folder there (the tab above the
box picks one) or its scratch folder; turn it off to work in a folder on
this PC. Everything else is the harness you know: the thread streams
live, approvals and questions appear as usual, Stop, rename, fork and
delete work. Attachments are read on this PC and sent along. The model
list shows the server's agents; install and sign them in from
Settings › Connections, in a terminal you watch
(`ssh -t … parzi agent install claude`). Browser tools need a window, so
sessions on a server have none.

When the desktop updates, the server follows on the next connect: Parzi
installs the matching build there and restarts its engine, keeping the
server's own settings.

How the link works: the desktop runs `ssh … ~/.local/bin/parzi rpc`. That
process lives on the server, reads the engine's token file there (0600)
and relays newline JSON to `parzi serve` on 127.0.0.1. SSH is the
authentication; the token never leaves the server.

Without the desktop:

```bash
parzi remote setup you@box          # key login only; syncs when linked
parzi remote sync
parzi remote status
parzi remote call session.list
parzi remote forget
```

On the server itself, `parzi approval list` and `parzi approval allow <key>`
answer parked approvals; an approval nobody answers is denied after six
hours. `parzi serve` and the desktop window never own the same `~/.parzi`
at once.

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
