# Project → Session → Plan

Agreed direction, 2026-11-16. Replaces the project-wide plan (`plan.md`,
plan items, phases, dependencies) and folds Steps and Suggested sessions
into it. All four steps are built (2026-11-16); notes under Build order.

## The model

```
Project        a folder. Its sessions are its overview.
└─ Session     one piece of work, with one agent at a time (switching keeps it)
   └─ Plan     the session's checklist: written once, ticked off as it works
```

- **One list per session.** The plan *is* the checklist. Today's Steps
  (`todo.write`) become the plan, made durable: it lives on the session,
  not in the last turn's widget, so it survives restarts and agent switches.
- **Work not started yet is a draft session.** An agent that suggests work
  creates a draft in the project: a title, a short brief and a plan, no run.
  You start it, or delete it. This replaces Suggested session cards and
  "Not started" plan items.
- **Helpers are sessions too.** A helper (`session.spawn`) or a side session
  is a child session with its own small plan. One word for all of them:
  Running.
- **No project-wide plan.** Ordering between pieces of work, when it
  matters, is written in a draft's brief ("after the login fix lands").

## What each screen shows

| Where | Shows |
|---|---|
| Thread | The plan as one card, near the top of the agent's latest reply, folded to "Plan · 3 of 7" once work starts. Proposing a plan for approval is the same card with Use this plan / Change it. |
| Side column | No plan card (the thread has it). Running lists child sessions with their "n of m". |
| Session manager | Needs you, Working, Drafts, then recent by day. Each row: agent, title, project, "3 of 7". A draft row opens the draft: its brief and plan, with Start. |
| Composer | Unchanged. Quote chips are gone: they only referenced project plan items. Old messages still show theirs. |

## Agent tools

| Tool | Does |
|---|---|
| `plan.write` | Sets this session's plan: `items: [{title, state: pending/active/done}]`. Replaces `todo.write` (kept as an alias for a release, since agents are trained on it). |
| `plan.read` | Reads this session's plan; with `session`, another session's in the same project. |
| `session.draft` | Creates a draft in the current project: `title`, `brief`, `plan`. Replaces `ui.propose_task`. |
| `session.list` | Gains each session's plan progress and `draft` status, so an agent sees what is planned. |

The plan approval flow (`USE_PLAN`, `plan_review`) stays, aimed at the
session's plan instead of `plan.md`.

## Storage

- `SessionMeta` gains `plan: Option<SessionPlan>` (title, items with
  state, updated). Written by `plan.write`, read by every surface. The
  transcript still records each change as a widget event, so history shows
  how the plan moved.
- `SessionStatus` gains `Draft`: never run, not counted as idle, and not
  given a run slot until started. Starting a draft sends its brief as the
  first message, with the plan already in place.
- Hand edits: the plan belongs to one session, so a person's edit and an
  agent's write only meet in that session. Last write wins, and the agent
  is told about a person's edit in its next turn (fixes the "hand edits
  erased" bug by removing the shared file it came from).

## What goes

- `crates/parzi-core/src/plan.rs` project plans (`plan.md` parsing, phases,
  `needs`), `plan_item` on sessions, `brief_for` plan injection, PR matching
  by item branch (`plan_prs`), `src-tauri/src/plans.rs` quote resolution by
  item id.
- UI: `PlanInline`, `planStore`, most of `planModel`, `TaskCard`,
  `taskCards` (localStorage), the side-column Plan card, `TodoList` (becomes
  the plan card).
- Roughly 4,000 lines, with their tests (`tests/plans.rs`,
  `tests/plan_review.rs` rewritten for session plans).

## Existing `plan.md` files

On first start after the update, each project's `plan.md` is read once:
items not done become **draft sessions** (title, detail as brief, an empty
plan), done and dropped items are left out. The file stays where it is,
renamed `plan.archived.md`, so nothing is lost. Sessions already linked to
an item keep working; the link is dropped.

## Build order

1. Session plan storage + `plan.write` / `plan.read` on the session
   (`todo.write` as alias); the thread's plan card reads it.
   **Built 2026-11-16**, with one change: `plan.write` / `plan.read` still
   write the project's `plan.md` until step 3 removes it, so for now
   `todo.write` (and an agent's own TodoWrite / update_plan) writes the
   session plan (`SessionMeta.plan`, `SessionStore::set_plan`). The names
   swap in step 3. An agent that starts on the thread fresh gets the plan
   as `<session-plan>`; the manager shows "n of m".
2. Drafts: `SessionStatus::Draft`, `session.draft`, manager Drafts group,
   Start.
   **Built 2026-11-16.** `SessionMeta.brief`; `SessionStore::create_draft` /
   `take_brief`; `session.draft` needs no approval (it only writes a card)
   and drafts in the caller's project, folder and model. The first send to
   a draft starts it, and its brief always leads that message, even when the
   person types their own. `session.list` shows drafts, briefs and "n of m".
   UI: `DraftCard` (Start / Delete) at the top of a draft's thread, Drafts
   group in the manager. `ui.propose_task` and `TaskCard` stay until step 3.
3. Remove project plans, `plan_item`, `PlanInline`, `TaskCard`; migrate
   `plan.md` to drafts.
   **Built 2026-11-16.** `parzi-core/src/plan.rs` is now only a reader and
   `to_drafts`, run once at startup (`src-tauri/src/main.rs`); it archives
   each `plan.md` as `plan.archived.md`. `plan.read` / `plan.write` act on
   the session plan (`todo.write` stays as an alias). Gone: `plan_item`,
   `ui.propose_task` and `widgets::task`, the plan brief and its
   `brief_hash`, plan-file locks, `src-tauri/src/plans.rs` (the PR lookup
   moved to `prs.rs`, command `branch_prs`), quote chips everywhere (UI,
   held queue, drafts, `send_message`), `PlanInline`, `planStore`,
   `planModel`, `PlanMark`, `TaskCard`, `taskCards`, the side-column Plan
   card. Old `<plan-item>` blocks in past messages still show as chips
   (`oldQuotes.ts`); old suggestion cards are hidden.
4. Approval flow on session plans; `session.list` shows progress.
   **Built 2026-11-16.** `plan.write` with `propose: true` asks Use this
   plan / Not now and saves on yes; plain writes save at once; with nobody
   to ask it saves. Tests: `tests/plan_review.rs`.

Each step lands on its own and leaves the app working.

## Open questions

- Should a draft's plan be written by the suggesting agent, or left for the
  agent that starts it? (Leaning: suggesting agent writes a rough one; the
  starting agent may rewrite it.)
- Do drafts expire? (Leaning: no; the manager shows their age.)
