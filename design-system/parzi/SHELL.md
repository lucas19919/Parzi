# SHELL — harness-owned shell (plan, not built)

Goal: the harness owns shell execution. Vendor-native shell tools are
refused at the gate; agents get Parzi shell tools backed by our own
process manager. Fixes shell amnesia, invisible background work, and
uninspectable command approvals in one move.

## What everyone else does (researched 2026-10-07)

- **Claude Code**: owns the shell. `Bash` runs each command in a
  separate process with default 2-min foreground timeout (env-tunable
  ceiling 10 min). `run_in_background: true` starts background tasks
  listed/stopped via `/tasks`; a foreground command hitting timeout is
  auto-moved to background (output to file, task id returned).
  Permission rules match on command text incl. `Bash(run_in_background:…)`;
  read-only commands auto-pass. OS sandbox (Seatbelt/bubblewrap, no
  native Windows) can auto-approve contained commands. Model-side API
  contract: app keeps one bash process alive across calls, sentinel
  line marks command end, timeout kills the process *group* and
  restarts the session.
- **OpenCode**: owns the shell. `bash`/`shell` tool with `timeout` and
  `background` params; background jobs go in a `BackgroundJob`
  registry, output spills to a managed file past size limits, TUI badge
  + cancel dialog, `bash_kill` by id. Permissions match parsed command
  text (`git status *`) plus `external_directory` guard. Known sharp
  edge, directly relevant to us: completion must key on process `exit`,
  never stdio `close` (background children inherit pipes and `close`
  never fires) — plus Windows job-object handling.
- **Codex**: shell via sandbox with network/filesystem policy; model
  retries unsandboxed only through an explicit escape hatch.

Common shape: **foreground call with timeout + cap, background spawn
returning an id, poll/kill by id, output spillover to file, text-matched
permissions.** We copy that shape; we skip Claude's live-watch
interjection (no event bus into turns yet) and OS sandboxing (no native
Windows story — our fence stays permission-text + cwd confinement).

## Design

Four tools, one registry (`shell.rs`, new file in parzi-runtime):

| Tool | Args | Returns |
|---|---|---|
| `shell.exec` | `cmd`, `timeout_ms?` (default 120s, cap 600s), `workdir?` | exit, output (capped 12k chars), `truncated` + spill path when cut |
| `shell.start` | `cmd`, `title?` | `shell_id`, "use shell.logs to poll" |
| `shell.logs` | `id`, `tail?` (default 4k chars) | new output since last read, `running` bool, exit if done |
| `shell.kill` | `id` | killed/confirmed-dead |

Semantics:

- Shell binary: `powershell -NoProfile -NonInteractive -Command` on
  Windows (pwsh if present), `sh -c` elsewhere. No `cd` in commands —
  `workdir` arg, defaults to session cwd, confined to the folder fence
  (reuse `worktree_relative`; outside → deny before spawn).
- Foreground: wait with deadline; on timeout, kill tree and return
  `timed out after Nms — restart it with shell.start` (Claude's
  auto-background is deliberately NOT copied: explicit is predictable).
- Background: spawn detached-ish (piped stdio into ring buffer +
  spill file), return immediately. Registry keyed by id, per-run cap
  (default 8 concurrent shells per session; 9th refuses with "kill one
  first"). Completion is detected on process `exit`, never pipe close
  (the OpenCode lesson). Kill = `kill_tree` (taskkill /T /F on
  Windows, process group on unix) + job-object adopt at spawn so
  orphans die with Parzi.
- Output: ring buffer 64KB in memory + append to
  `sessions/<sid>/shell/<id>.log`. `logs` returns only-unread tail
  (per-reader cursor per tool-call? No — per-shell global cursor would
  race two readers; keep it simple: `tail` last N chars + byte offset
  the caller passes back. `offset?` arg, response includes `next_offset`).
- Env: inherit filtered parent env (reuse `child_env` allowlist) plus
  per-call `env` map. No interactive input, ever: stdin is null/devnull
  so a prompting command fails fast instead of hanging; document it.
- Timeouts: `timeout_ms` clamp [1000, 600000]. Background shells get a
  max lifetime (default 30 min, then kill + note) so dev servers can't
  squat forever across restarts. Registry does NOT survive restart
  (running PIDs die with the job object anyway); on boot, stale
  `shell/*.log` files remain readable, shells report `restarted`.
- Research lane: no shell tools offered, gate denies vendor shell as
  today. Unchanged.

## Gate changes (exact)

1. `category()` in toolhost.rs: add `"Bash" | "BashOutput" | "KillShell"`
   (already there) — plus vendor spellings seen in the wild:
   `"shell" | "execute" | "run_command" | "bash"`. Vendor shell calls
   on lanes offering `shell.*` get
   `Deny("use shell.exec — the Parzi shell keeps history, timeouts, and background jobs")`.
   Destination-included: the deny message names the replacement so the
   model reroutes first try.
   Gotcha this plan already hit once: `"shell.exec"` is both our tool
   name and a vendor category key, so the pre-existing `lists_kinds`
   check misfires on our own allowlist entry and instantly denies all
   vendor file tools. `is_parzi_tool()` excludes our own tools from
   that check; `approval_gate` caught it.
2. `lane_policy_for`: build/research-unchanged except add
   `"shell.exec" | "shell.start" | "shell.logs" | "shell.kill"` to the
   always-offered list; research explicitly excluded (assert in test).
3. `research_allows`: no shell entries (unchanged — deny path stands).
4. `approval_override`/mode flow untouched: shell tools go through
   `approved()` like any Parzi tool, so Full runs, Supervised asks,
   MCP `tool_modes` still apply per `server.tool`.

## Brief changes (exact strings go in handler.rs)

- PARZI_BRIEF, page paragraph: append
  "The Parzi shell is the only shell: shell.exec for commands,
  shell.start for servers and watchers (poll with shell.logs, stop with
  shell.kill). Vendor Bash is refused — never retry a refused command
  natively, reroute it to shell.exec. Never pass `cd` inside commands;
  use workdir. Stdin is closed: non-interactive flags only."
- TEAMWORK_BRIEF: append
  "Long work goes to shell.start (one server per need, kill it when
  done); foreground builds get explicit timeouts."

## Files (exact)

1. `crates/parzi-runtime/src/shell.rs` (new, ~450 lines): `ShellRegistry`
   (`Arc<Mutex<HashMap<ShellId, LiveShell>>>`), spawn/wait/kill/log-tail,
   spill files under `sessions/<sid>/shell/`, `shell_for_tests` ctor.
2. `crates/parzi-runtime/src/tools.rs`: 4 defs + `is_shell_tool` +
   humanize labels ("Running `x`", "Starting bg `x`", "Reading logs",
   "Killing shell"); defs filtered by allowlist as usual.
3. `crates/parzi-runtime/src/toolhost.rs`: `ToolHostParts.shell:
   Arc<ShellRegistry>` plumbed from `launch_inner` (one registry per
   run — shell history is per-session-run, matching cwd lifetime);
   `execute_shell` branch; gate vendor-shell deny (item 1 above).
4. `crates/parzi-runtime/src/orchestrator/launch.rs`: allowlist edit +
   registry construction.
5. `crates/parzi-runtime/src/lib.rs`: `pub mod shell`.
6. Tests: `tests/shell_tools.rs` (new) — foreground echo/true/false
   exit codes; timeout kills a sleeper (short sleeps only);
   start→logs→kill lifecycle; output cap + spill file exists;
   research lane denies vendor Bash and offers no shell.*; allowlist
   membership assertions in the style of the existing teamwork tests.
7. No UI changes. No protocol changes. Follow-up (not this build):
   Tasks dock tab reads live shells (registry is per-run today; needs
   promoting to per-session to survive across turns in one session —
   note this explicitly: per-run registry means a *new turn* gets a new
   registry and loses handles to the previous turn's background shells.
   Fix options: (a) per-session registry stored on Pump keyed by sid,
   (b) persist shell table in sidecar. (a) is correct; do it in this
   build, not later.)

Correction to (a): construct the registry per-session, not per-run —
store `Arc<Mutex<HashMap<SessionId, Arc<ShellRegistry>>>>` on Pump
(`shells` field), `launch_inner` clones the session's registry
(creating on first use). Stale entries reaped when the session is
deleted/purged. This is what makes shells persist across turns and
what the Tasks tab will read.

## Out of scope (explicit)

- Persistent interactive shell sessions (Claude API-style sentinel
  sessions with cd-persistence). Each call is one process; state via
  cwd + files. Rationale: sentinel sessions hang on prompts and need a
  restart dance; one-shot + background covers real needs with far less
  failure surface. Revisit only with demand.
- OS sandboxing (no native Windows mechanism; fence + approvals stand).
- Live output streaming into a running turn (turn loop is
  request/response; poll covers it).
- Cross-restart shell resurrection (PIDs die with us by design).

## Order

1. `shell.rs` + unit tests (registry, cap, spill, kill).
2. Tool defs + toolhost branch + gate deny + allowlist + briefs.
3. Integration tests (shell_tools.rs) + full suite.
4. Commit, build. Tasks-tab live-shell follow-up after.
