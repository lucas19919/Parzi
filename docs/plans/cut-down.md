# Parzi, cut down

An audit on 2026-11-21. Parzi is about 90k lines of Rust and 25k of Svelte,
built in 694 commits. The UX is right. The sprawl sits in three places, and
most of the small bugs grow out of them.

## The shape (decided 2026-11-21)

**Window**
- Top tabs stay. Open tabs work like an inbox you clear.
- Settings stays at the top left, beside the session manager.
- The notifications bell goes. A tab shows its session's state as an icon.
- Glass and theme packs stay.

**Home** is global and nearly empty: the glass composer over the wallpaper,
the tabs, settings and the session manager.

**Session manager** (top left, today's history view): every session, past
and present. It stays as it is for now, and gets a review later.

**Inside a session**
- The side column has four cards:
  - **Environment:** where it runs, the project and the branch. All three
    can be changed while the session is open. This is a technical change,
    and an important one.
  - **Context:** how full the session is.
  - **Usage:** the limits of the agent in use: a rolling window (such as 5
    hours), weekly or monthly, whichever that agent has.
  - **Running:** an audit log of what the agent is doing (tool steps,
    background tasks). The name isn't final.
- Files, Hosts and Changes go.
- **Pins:** links the session keeps coming back to (its localhost, its plan)
  sit as pills above the composer. They belong to the session. A pin made
  through a project-wide tool, such as a project plan, belongs to the
  project instead.

**Opening things**
- A link or a pin opens full screen over the current tab, and the composer
  stays floating over it so you can keep prompting.
- Settings opens the same way, without the composer.
- Most of the full browser goes.

**Projects** are the home of context, meaning the codebase. They show in
three places: the line above the composer on Home ("Let's build in X"), the
Environment card, and the session manager.

**Usage** also shows in Settings, Providers, for each agent.

**Tools for agents** get simplified, and further than the first pass if it
holds up: one tool per job.

**Plans**
- **Session plans stay as the core:** a checklist the agent keeps while it
  works, which survives restarts and agent switches. Agents get one plan
  tool, and their own todo tools map onto it. A proposal ends the agent's
  turn instead of holding a tool call open. The full plan document is kept
  with the session, and the plan pin opens it.
- **Drafts stay:** sessions that haven't started yet.
- **Project plans** are a markdown file with a short goal and an ordered list
  of targets. They are pinned at project scope.
  - Each target is tied to the drafts and sessions working on it, so it
    can't drift away from the real work.
  - The plan also shows inline above the composer, next to the pins, in a
    simplified form: just the targets, each with an indicator when a
    session is working on it.
  - Agents can add, reorder and finish targets, and edit the goal.

## Where the sprawl is

**1. The agent contract.** Every turn sends `parzi.md` (about 2.5k tokens), 50
Parzi tools (about 7k) and every connector tool (186 tools, about 35k). There
are four ways to write a plan (`todo_write`, `plan_write`, the agent's own
TodoWrite, `update_plan`), and the two Parzi ones follow different gate rules
(`toolhost.rs:130` vs `:337`). Three ways to show a file. `desk_open` and
`desk_theme` duplicate `app_do`. Three ways to GitHub. Three ways to ask
(`ask_user`, `ui_offer_replies`, `plan_write propose`). Prompts name tools that
aren't listed (`todo.write` in `run.rs:450`, `brain_list` in `brain.rs:727`,
`ui_show_file` in `parzi.md:15`, which is filtered out of `tools/list`).

**2. The engine.** There is one `Provider` trait, but there are three full
drivers (`claude.rs`, `codex.rs`, `acp.rs`). Each one has its own process
lifetime, cancel, approvals, usage accounting and turn-end detection. Where
they disagree, the gaps are patched with text matching: `"exited before it
answered"`, a reply of 100 characters or fewer containing "cancelled" is
deleted (`run.rs:101`), and OpenCode failures are scraped from its log file.
`claude.rs` has its own copy of the JSON-RPC peer.

**3. The window.** About 60% of `ui/src` plus `src-tauri/src` is outside
"chat with coding agents":

| Area | Lines |
| --- | --- |
| Full browser (adblock, downloads, Bitwarden, site prefs, bookmark import) | 6.3k |
| Theming, wallpaper, dither, theme packs | 2.6k |
| Onboarding and import | 2.5k |
| Agent drives the UI (`app_*`, `desk_*`) | 2k |
| Viz and artifacts | 2.6k (keep) |

`App.svelte` (2738 lines) holds the send and queue state in eight overlapping
structures (`running`, `inflight`, `holdLive`, `released`, `queuePaused`,
`drainTimers`, `sendingTabs`, `pendingSend`) with polling loops and grace
timers. There are no component tests for any of it.

## Bugs with evidence

From the logs (`~/.parzi/logs`, 2026-11-15 to 2026-11-21) and the code:

1. **Antigravity turns hang for up to 20 minutes.** Antigravity reports its
   `view_file` and `create_file` calls finished only when the whole prompt
   ends (session history shows the results arriving in one burst, median 8
   minutes late). The quiet-settle path only armed when no tool was open
   (`acp.rs:1568`), so a wedged turn waited for the idle cap. This accounts
   for most of the warnings in the logs. "Run create_file?" is that tool's
   title, not a pending approval. *Fixed in step 1.*
2. ~~ACP Allow can send a rejection.~~ Deliberate: an allow is one action,
   never a standing grant (test `an_allow_is_one_action_and_never_a_standing_grant`).
3. ~~Claude can hang to the idle cap.~~ Both spots are deliberate and tested
   (a resume replays an empty turn first; background agents hold the turn).
   No Claude hangs in the logs.
4. ~~Stopping a turn leaves approvals behind.~~ The runtime's approval wait
   already watches the run's cancel (`toolhost.rs:410`), for every agent.
5. **Switching model duplicates the message.** It resends the last message as
   a new User event and dropped its attachments (`App.svelte:1514`; the
   attachments are fixed in step 1, the duplicate waits for step 4). Going from
   agent A to B and back to A loses A's native session, because only one
   `VendorSession` is stored per session.
6. **A late session load writes into the wrong tab.** `showSession` writes
   `tab.composer.model` after an await, but `navSeq` isn't bumped when you
   switch to a page tab (`App.svelte:568,584,638`).
7. **The app often doesn't shut down cleanly.** 7 of the last 17 runs logged
   "ended without closing", and the browser process died and restarted 3
   times.
8. **The Bitwarden button sticks on "Installing…"** after a successful install
   (`GeneralSection.svelte:46-59`).
9. **CI never runs the desktop crate's tests.** The 74 tests in `src-tauri`
   aren't run in CI: it is outside the workspace and CI only lints it.
10. **Dead settings and copy.** The Cards setting (Clear, Glass, Solid) is
    accepted end to end and ignored (`theme.ts:56-60`), there is a listener
    for `parzi:default-mode` that nothing dispatches, and the Unreleased
    CHANGELOG still describes the Formatting agent it also says was removed.

The October review in `.claude/cleanup/REVIEW_*.md` still lists most of its
duplicate helpers. Its spawn bug (a failed launch reported as "queued") is
fixed: `harness.rs` now returns `Dispatch::Failed`.

## Against the goals

| Goal | Today | Gap |
| --- | --- | --- |
| Test on laptop, iPhone, Android without asking | Hosts card shows loopback URLs only. `shell.rs:1029-1112` throws away Vite's `Network:` line on purpose. Remote sessions show no hosts. | No LAN URL, no QR, no tunnel, no device sizes in the pane, no port forward from the server. |
| Context across many projects | A project is a brain note with a folder per device. | Two sources of truth: `SessionMeta.project` vs the cwd match each turn (`handler.rs:230`). Global instructions live in two places (SYSTEM.md, pinned `all` notes), project instructions in three (project note, `projects/<slug>/`, CLAUDE.md). |
| Visuals when needed | `viz` blocks and diagonaldiagrams work. | Fine. Keep them. |
| Ask the right questions | `ask_user` choice cards are good. | Three tools for one job. |
| Spin out agents on any ACP | `session_spawn` with `models_list`. | `wait` polls the store for up to 180 s inside the parent's tool call. ACP has no `session/load`, so a follow-up turn in one run starts empty. A non-gated child refuses to start in read-only mode. |
| Switch agents easily | Switch model keeps the thread. | The handoff is lossy (12k characters, tool output cut to 500), A→B→A loses A, and the message is duplicated. |

## Status (2026-11-22, branch `cut-down`)

| Step | State | Left |
| --- | --- | --- |
| 1. Stop the bleeding | Done | Watch the logs for false "turn is quiet" warnings |
| 2. Cut | Done | |
| 3. One tool per job | Done, first pass | Merge `ask_user` with `ui_offer_replies`, and the session tools, if it holds up |
| 4. One turn engine | First pass | One shared turn engine behind the three drivers; one meaning for usage; no text matching in `run.rs`; a richer handover than 12k characters. Needs live agents to verify |
| 5. One send state | Done | |
| 6. The session layout | Done | Agents' own browser pages are still tabs (the engine tracks them through the tab list) |
| 7. Plans | Done | |
| 8. Changeable environment | Project and folder done | Moving a session between this computer and the server |
| 9. Test on any device | Phone emulator done (Windows) | Dev servers on the server don't show here (SSH port forwarding) |
| 10. Usage for every agent | Claude and Codex | OpenCode, Grok, Antigravity and Cursor report no limits |

Not verified yet: a run of the real app (only the mock window was checked),
live agents, and the macOS build (edited, not compiled).

## Build order

The pieces depend on each other in two tracks. The **engine** track lives in
`crates/` and the **window** track in `ui/` and `src-tauri/`. Each track runs
in order, and the two tracks can run side by side in separate worktrees.
Steps 7 and 9 are the only places where the window waits on the engine.

| # | Step | Track | Needs | Size |
| --- | --- | --- | --- | --- |
| 1 | Stop the bleeding | both | nothing | S |
| 2 | Cut | window | 1 | M |
| 3 | One tool per job | engine | 1 | M |
| 4 | One turn engine | engine | 3 | L |
| 5 | One send state | window | 2 | M |
| 6 | The session layout | window | 5 | L |
| 7 | Plans | both | 3, 6 | M |
| 8 | Changeable environment | engine, then window | 4 | L |
| 9 | Test on any device | both | 6 | M |
| 10 | Usage for every agent | both | 4 | M |

### 1. Stop the bleeding

Each fix starts with a test that reproduces the bug.

- **Engine:** a tool call started before the latest reply text no longer
  blocks the settle, so Antigravity's late-closing reads can't hold a
  written reply open (`acp.rs`, `Seen::tools_running`).
- **Window**
  - Retrying with another model keeps the message's attachments. Not
    duplicating the message needs a real retry in the engine (step 4).
  - The `showSession` race: a late load checks its tab is still in front.
- **CI** runs the `src-tauri` tests.

Done when a week of logs shows no "turn is quiet" warnings that weren't real
work. The Bitwarden bug is skipped, because step 2 removes Bitwarden.

### 2. Cut

Delete, don't rewrite.

- The browser extras: adblock, Bitwarden, downloads, site prefs, bookmark
  and history import (including that onboarding step).
- The notifications bell and card, plus their settings. Tabs get state
  icons: working, needs you, done unread, failed.
- The Files, Hosts and Changes cards.
- The dead code from the audit: the Cards setting, orphan listeners and
  stale CHANGELOG lines.

Page tabs stay until step 6 replaces them. Done when the app builds, every
remaining test passes, and the line count drops by about 5k.

### 3. One tool per job

- One tool each for `plan`, `ask`, `show`, `app` and `session`. Vendor todo
  tools map onto `plan`. A proposal ends the turn instead of holding a tool
  call open.
- Connectors load on demand.
- `parzi.md` under 1k tokens.
- The repo's `AGENTS.md` or `CLAUDE.md` goes to every agent, not just Claude.

Done when the Context card shows Parzi's own share under about 3k tokens,
and every agent passes the existing teamwork tests.

### 4. One turn engine

- One place owns process lifetime, cancel, idle and settle, approvals and
  usage. Each agent is a thin adapter.
- Usage means the same thing everywhere.
- One native session is stored per agent, so switching A→B→A resumes A.
- ACP gains `session/load`.
- A better conversation handover. Steps 8 and 10, and switching agents,
  all build on it.
- No more text matching in `run.rs` or `App.svelte`.

Done when all six agents pass one shared turn test suite.

### 5. One send state

The eight overlapping structures in `App.svelte` become one per-session
state machine in a `.ts` file with unit tests.

Done when sending, queueing, stopping and switching are covered by tests and
`App.svelte` loses its polling loops.

### 6. The session layout

- **Side column:** Environment, Context, Usage (Claude and Codex first),
  Running.
- **Pins** above the composer.
- **A link or pin opens full screen** over the tab, with the composer
  floating. Settings opens the same way, without the composer.
- **Page tabs go.**

Done when a session with a dev server and a plan works end to end without
leaving its tab.

### 7. Plans

- The session plan document is kept with the session and opened by its pin.
- Project plans: a markdown file with a goal and ordered targets, each tied
  to its drafts and sessions. The targets row sits above the composer with
  "being worked on" indicators. Every agent edit to the plan appears in
  Running.

### 8. Changeable environment

- `SessionMeta.project` becomes the only record of a session's project.
- Then, in order: switch branch, switch project, move between this
  computer and the server. Each one uses step 4's handover.

### 9. Test on any device

- Laptop, iPhone and Android in the full-screen page view, emulated for
  real through WebView2's device mode (screen, pixel density, user agent,
  touch). A QR code was tried and dropped in favour of the emulator.
- Forward a server session's dev servers over SSH.

### 10. Usage for every agent

Find limits for the ACP agents where each one exposes them, and fall back to
tokens and cost where none does. Show it in the Usage card and in
Settings, Providers.

### Later

- A review of the session manager.
- Global instructions in one place: SYSTEM.md, or pinned notes.
- The phone app (`docs/plans/mobile.md`).
