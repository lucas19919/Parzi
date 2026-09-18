# Parzi Hub — workspaces, projects, roles, and the team daemon

Status: proposal v2, 2026-09-16. Replaces v1 (which wrongly made a project a
repo). Open source, domainless: nothing here depends on a Parzi-owned server.

## 0. In one paragraph

A **workspace** is the set of repos a team works with. A **project** is a piece
of work across those repos: it starts as one markdown file a person can read,
an **orchestration agent** turns it into a plan of lanes and mini-sprints,
**coding agents** check out sprint tasks together with the files they need, and
a cheap **header agent** answers "what is happening?" without burning the
expensive models. You talk to the header to understand, to the orchestrator to
change things, and never to the coder. The **hub** is a headless daemon that
keeps the live state (who holds what, what is running) in sync between
teammates' native Parzi apps; the durable record is files in git.

## 1. The model

```
Workspace  acme            repos: shop-api, shop-web, shop-crm     members, roles
  Project  checkout-flow        PROJECT.md (why/what/constraints/roster)  status, budget
    Plan   PLAN.md              sprints → lanes → tasks [repo] [scope]    drafted by the orchestrator
      Lane api                  one coding session per sprint             holds task + file leases
        Task TSK-7              [repo:shop-api] [scope:src/checkout/**]  claimed, running, done
```

### 1.1 Files are the truth: the workspace repo

Every workspace is a small git repository of its own, cloned on every member's
machine at `~/.parzi/workspaces/<name>/` and on the hub:

```
workspace.toml           name, repos = [{name, remote, default_branch}], members = [{user, role}]
projects/<slug>/
  PROJECT.md             the project, in Parzi grammar (§2)
  PLAN.md                sprints, lanes, tasks (§3); written only by the orchestrator session
  KNOWLEDGE.md           gotchas and decisions, appended by any lane (knowledge.record)
  capsules/TSK-7.json    handoff capsules (§5)
```

This is why the design is domainless: a team with the workspace repo and no
hub still shares projects and plans, asynchronously, through git. The hub adds
the real-time layer — leases, presence, heartbeats, relay — and nothing that
would be lost if it went away. Code repos are never touched by the hub; each
machine maps `remote → local path` for itself.

### 1.2 Roles: chosen when a project is created

The three roles already exist in Parzi as the project roster
(`lanes.rs::ProjectRoster { header, orchestrator, implementation }`); a project
binds a model to each:

| Role | You pick e.g. | Talks to | Does |
|---|---|---|---|
| **Header** ("idea agent") | gemini-flash | you, any time | drafts PROJECT.md with you; answers "what's happening / why / where is X" from the board, journal and capsules — reads only, never plans, never codes |
| **Orchestrator** | fable | you, when you want change | writes and re-writes PLAN.md, dispatches lanes, arbitrates contention, decides re-plans; the only writer of PLAN.md |
| **Coder** | muse-spark | nobody | one session per lane per sprint; checks out a task and its files, works in a worktree, ends with a capsule |

Sessions form a tree the runtime already supports (`session.spawn`,
`lane.dispatch`, `parent_id`): project session (header) → orchestrator session
→ lane sessions. Token spend is shaped by the roster: the header is cheap and
answers most questions; the orchestrator runs only on plan, re-plan and
contention; coders run per task, with a per-project budget (`budget:` in
PROJECT.md; AUDIT R-4 makes the runtime honour it).

## 2. PROJECT.md — Parzi grammar

Small enough to read in a minute and to parse with the existing plan parser
style (`parzi_core::plan`), no YAML engine:

```markdown
# Project: Checkout flow
workspace: acme
repos: shop-api, shop-web
roster: header = google/gemini-2.5-flash, orchestrator = anthropic/claude-fable-5-1, coder = opencode/muse-spark-1.3
budget: 40 USD
status: drafting            # drafting → planned → running → done | parked
critical: shop-api/src/payments/**     # files whose lease transfer needs a human

## Why
Two sentences a new teammate understands.

## What
- [ ] acceptance criterion, testable
- [ ] …

## Constraints
- no schema migrations this sprint
```

The header agent drafts it in conversation with you; you edit it like any file.
`status: planned` is set by the orchestrator when PLAN.md exists.

## 3. PLAN.md — sprints, lanes, tasks

Drafted by the orchestrator from PROJECT.md; the checklist grammar Parzi already
parses, extended by four bracket attributes:

```markdown
## Sprint 1 — API surface (target: 1 day)
### lane api
- [ ] TSK-7  Checkout session endpoint     [repo:shop-api] [scope:src/checkout/**,src/routes.rs] [after:]
- [ ] TSK-8  Payment intent adapter        [repo:shop-api] [scope:src/payments/**] [critical]
### lane web
- [ ] TSK-9  Checkout page skeleton        [repo:shop-web] [scope:src/routes/checkout/**] [after:TSK-7]

## Sprint 2 — wire and test
…
```

A **mini-sprint** is a group of tasks the orchestrator expects to land
together; lanes inside a sprint run in parallel, sprints run in order. A lane
session checks out its sprint's tasks one at a time, in `[after:]` order.

## 4. Checkout, leases, and asking for a file

**Checkout.** When a lane session starts a task it sends `lease.claim {task,
paths}` — the task's scope as the plan wrote it, repo-prefixed. A path holds
itself and everything under it; a glob holds every path it matches and
everything under those; case is ignored where the file system ignores it
(Windows, macOS). The hub grants when no active lease could hold a file the
claim could (a plain path might be a folder, so the check leans to asking),
else answers `Held {by: lane api / TSK-8, since}`. A lease lives while its
heartbeat does (TTL 90 s) and ends with the task.

**Asking.** If lane *web* needs `src/routes.rs` that lane *api* holds:

```
web  → hub → api's session:  lease.request {path, for: TSK-9, reason}
api's session answers (a tool call in its own run, with its own context):
        lease.grant   → path moves to web (out of a claimed folder, only that path); both journals record it
        lease.deny    → web gets the refusal and its reason
        (no answer in 120 s) → treated as deny
```

- If the path is under `critical:` in PROJECT.md, `grant` is not the coder's to
  give: the request becomes an **approval card** for a human — the existing
  `Approver` path, so it shows up exactly like a tool approval, on whichever
  member's app is watching the project (owners and maintainers, §7).
- On `deny`, or when two lanes keep colliding, the **orchestrator convenes**: it
  receives both requests and the plan, and decides — re-order the sprint, split
  the scope, or merge the two tasks into one lane. That decision is a PLAN.md
  edit, so it is visible and reversible. "Convene" is one orchestrator turn, not
  a group chat.

**Enforcement.** Every edit the agent asks about passes Parzi's gate
(`parzi_runtime::toolhost`), which checks the local lease table: a file held
by another lane is refused, naming the holder; a file outside the lane's own
scope is allowed with a `Notice` and a `scope_creep` mark on the card
(`lease_mode = "strict"` in `workspace.toml` makes it a refusal). A shell
command cannot say what it will write, so the worktree is compared before and
after it: a change to another lane's file is refused and put back from a
copy taken before the command. An agent that does not ask before every change
(see the README's agent table) is outside this promise. Humans are
stopped at the commit: `parzi hook install` puts a pre-commit hook in each repo
that refuses a commit touching a held file, naming the holder; `--no-verify`
overrides and the hub logs it.

**Where lanes run.** By default on the machine of whoever started the project;
a teammate can take a lane from the deck (`lane.take`), which moves the lane
session to their machine — the lease follows the lane, not the machine.

## 5. Context: what each role sees

No role ever receives another session's transcript.

- **Coder**, per task: `KNOWLEDGE.md`, the task line and its acceptance lines,
  the file list in scope, public signatures of what the scope imports, and the
  `capsules/` of its `[after:]` tasks (~25 lines each: summary, touched files,
  exported symbols, verification, invariants, gotchas). A task ends with
  `board.handoff {capsule}`, validated against a schema like widgets are.
- **Orchestrator**: PROJECT.md, PLAN.md, all capsules, the live board, and the
  contention it is asked to resolve.
- **Header**: PROJECT.md, the board, the journal tail, capsules — and nothing
  that costs tokens to produce. "What's happening?" is answered from state.

## 6. The hub daemon

`crates/parzi-hub`, one static binary, headless. It holds the live layer and
serves the workspace repo's current state:

- **auth**: per-user bearer tokens (`parzi-hub user add sam` prints one once);
  plain HTTP, TLS by a reverse proxy on a VPS, plaintext on a LAN and documented
  as such;
- **journal** `~/.parzi-hub/<workspace>/journal.jsonl` (append-only, fsync'd)
  + `state.json` snapshot every 500 events; rebuilds from the journal;
- **leases**, **board** (the live overlay on PLAN.md), **presence** (who holds
  what since when — derived from leases), **heartbeats** (5 s per live task:
  turn, tool, files touched, branch, checkpoint), **relay** (watch, §8);
- **workspace repo sync**: the hub pulls the workspace repo on a timer or
  webhook; clients push commits through git as usual. PLAN.md has a single
  writer (the orchestrator session), so merges do not happen.

**Protocol.** One WebSocket per client, JSON text, one object per frame,
requests with an `id` answered by `id`, pushes without. Fourteen types:
`hello/welcome`, `board.create|claim|release|handoff|block|event`,
`lease.request|grant|deny`, `heartbeat`, `presence`, `plan.changed`,
`watch.start|stop|event`, `error`. Reconnect = `hello` again, full snapshot.
No binary framing, no channels, no partial sync — the state is small.

**Storage** is the journal; no database. **Idle** under 20 MB.

## 7. Team and permissions

`workspace.toml` carries members and roles; the hub enforces them:

| Role | can |
|---|---|
| owner | everything; mint tokens; change roles; delete the workspace |
| maintainer | create projects, approve `critical` transfers, integrate, take lanes |
| member | create projects, claim tasks, take lanes, ask the header/orchestrator |
| viewer | read the deck, watch |

Identity is `user` on this hub, nothing else — no accounts service. A fork of
Parzi with its own hub is a full peer. Secrets never cross the hub: provider
keys stay in each machine's keyring; the hub sees plans, leases and capsules.

## 8. What the native app shows

**Sidebar.** Workspaces = Local + each joined hub; under each, its projects
with counts (running lanes · open tasks · people). Threads unchanged.

**Project deck** (the stage view for a project; replaces `ProjectMainPage`,
in line with the 2026-09-16 sidebar simplification):

```
checkout-flow · acme · sprint 1/2 · ● hub 18 ms · Ada · Sam · ⚡ 2 lanes     [ Project ] [ Plan ] [ Activity ]
```

- **Project**: PROJECT.md rendered; the **Ask** box beneath it talks to the
  header ("what's blocking the API lane?"), and a **Direct** toggle switches
  the same box to the orchestrator ("move TSK-9 to sprint 2").
- **Plan**: sprints as rows, lanes as columns, tasks as cards: holder, leased
  files (count, hover for the list), heartbeat when live (tool · turn · branch
  · integrates-cleanly dot from `git merge-tree`), pending lease requests,
  approval cards for `critical` transfers. Actions: dispatch sprint, take lane,
  block, integrate, watch.
- **Activity**: the journal — claims, grants, denials, convenes, handoffs,
  hook refusals, plan changes — filterable by lane, person, task.

**Watch** (on request only): `watch.start` asks the holder's client to relay
that lane's `RunEvent`s while somebody watches; read-only thread view with a
banner; `share_sessions = false` in a member's config declines.

**Lease pills** wherever a file is named: amber "held by api · TSK-8".

## 9. Install and run — no domain required

The hub is one static binary released by **cargo-dist** from the same tag as the
client (installers are generated from the release, on whatever fork):

```
curl -fsSL https://github.com/<you>/Parzi/releases/latest/download/parzi-hub-installer.sh | sh
irm  https://github.com/<you>/Parzi/releases/latest/download/parzi-hub-installer.ps1 | iex

$ parzi-hub init acme --repo git@github.com:acme/workspace.git
parzi-hub 0.2.0 · workspace acme · listening on 0.0.0.0:4040
owner token: 7f3a-…                    (saved once to ~/.parzi-hub/acme/token)
add people:   parzi-hub user add sam --role maintainer
they join:    parzi hub join http://<this-ip>:4040 <token>
```

`parzi-hub service install` for systemd / Windows service / launchd; a
`FROM scratch` Docker image from the same workflow. Client: `parzi hub join …`
or Settings › Team. Solo use needs no hub at all: a local workspace with the
same files, same roles, same deck.

## 10. What comes first (blockers in the audit)

| Item | Why it blocks | Size |
|---|---|---|
| R-5 queued runs lose events | a lane card that says "running" must be right | S–M |
| R-1 kill does not stop the stream | stop from the deck must stop; leases must release | M |
| R-4 no token/cost budget | `budget:` is a promise the runtime must keep | S–M |
| H-5 `session.send_message` injects a `User` turn | orchestrator ↔ lane messages must be typed, untrusted | M |
| S-1 85 IPC commands in the shell | the deck adds a `hub.*` family; the shell must forward, not implement | M |
| B6 art in the public repo's history | installers point at this repo | S, human gate |

## 11. Milestones

| # | Deliverable | Acceptance | Lane-days |
|---|---|---|---|
| H0 | `parzi-core::hub`: PROJECT.md + PLAN.md grammar (parse/serialize), `Lease`, `Capsule`, messages; workspace repo layout | round-trips; a 50-task plan parses < 1 ms; capsule schema validation | 1 |
| H1 | Local-only: project creation (roster pick, header drafts PROJECT.md), orchestrator drafts PLAN.md, lanes check out tasks with local leases, `resolve()` enforcement, capsules in context, `lease.request` between local lanes, `critical` → approval card | one machine, two lanes, a forced collision: request → grant, request → human card, deny → orchestrator re-plans; all as tests with a scripted provider | 3 |
| H2 | `parzi-hub` daemon: auth, journal, leases, board, presence, heartbeat, workspace repo sync, `user add`, first-run UX | two fake clients: overlap blocked; TTL expiry; restart rebuilds; idle < 20 MB | 2 |
| H3 | Client ↔ hub: `parzi-runtime::hub`, remote leases, `lane.take`, pre-commit hook, Settings › Team | two machines: claim on A visible on B < 1 s; commit into a held file refused | 1.5 |
| H4 | Deck: Project/Plan/Activity, Ask/Direct box, lease pills, approval cards for transfers | screenshots per state; a full project run end-to-end on the test workspace | 2.5 |
| H5 | Install: cargo-dist, installers, service install, Docker, docs | clean VPS to running hub in one line; clean laptop joins in one command | 0.5 |
| H6 | Watch relay | streams within one event of live; declining works | 1 |

≈ 11.5 lane-days after §10. Order: H0 → H1 (everything works on one machine
first) → H2 ∥ H3 → H4 → H5 → H6. H1 is the milestone that proves the idea;
nothing in H2+ changes what a project *is*.

## 12. Gates

hub idle memory; claim round-trip on LAN (< 50 ms); client overhead in
`parzi-app` (< 5 MB, `target/measure-app.ps1`); the collision and
request/grant/escalate tests in CI; header-agent cost per "what's happening?"
(< 2k tokens in, measured); a two-machine smoke test before each release.

## 13. Cut, and why

- Streaming every session to the hub — watch is on demand; privacy and bandwidth.
- A "convene" chat between agents — one orchestrator turn with both requests in
  hand decides; the decision is a plan edit you can see and undo.
- Generated stubs for blocked tasks — creates the collision it claims to avoid.
- Speculative rebase machinery — one `merge-tree` boolean on the card.
- The hub as a git remote, a database, a binary protocol, accounts, a domain.
- Talking to the coder — its capsule and its diff are the interface.

## 14. Open questions (three)

1. **Where lanes run** (§4): default to the initiator's machine, movable with
   `lane.take` — or should the orchestrator place lanes on members by role?
2. **`critical` transfers**: any maintainer online may approve, or only the
   project's creator?
3. **Watch by default**: on with a visible banner (proposed), or off?

## 15. Things the model has to answer before H1 (review of v2)

1. **Today's "project" is tomorrow's workspace.** `~/.parzi/projects/<name>`
   (root, lanes, roster) conflates the two. Migration: each existing project
   becomes a local workspace with one repo; its lanes become lane templates.
   Otherwise two things are called project and the sidebar lies.
2. **PLAN.md has one writer, but people edit files.** The orchestrator session
   holds a lease on `PLAN.md` like any file; a human edit while it is held is
   refused by the hook, and a human edit while it is free is re-read by the
   orchestrator before its next write. Two orchestrators for one project cannot
   exist: the lease is the lock.
3. **Whose tokens, whose keys.** A lane runs on a machine with that machine's
   provider keys and subscriptions. The budget in PROJECT.md is a cap on the
   project; spend is attributed per member in the journal. Shared team keys are
   out of scope; the hub never holds a key.
4. **Whose permissions.** A taken lane runs an orchestrator-written task on a
   teammate's machine with shell and file access. Rule: the workspace can only
   *tighten* what a machine allows (lane mode, allowed tools, sandbox roots);
   the hub can never raise a permission. This is AUDIT B2's principle extended
   to the team.
5. **What Done means.** A task ends with tests in the capsule (`verification:`
   is required, not prose), then Verification → Integrate. Per repo,
   `integrate = "pr" | "merge"` in `workspace.toml`: teams with review open a
   PR from the worktree branch; solo work merges. Integration is a maintainer
   action or, with `integrate = "merge"` and green tests, the orchestrator's.
6. **Failure cases with a stated behaviour.** Coder crash → lease expires
   (90 s) → card shows *stale* → orchestrator may reassign after 10 min (a
   member can do it at once). Orchestrator crash mid-plan → PLAN.md is written
   atomically and committed, so the plan is either the old or the new one.
   Laptop asleep → same as crash. Request/deny loops → at most two requests per
   file per task, then convene. Budget hit → lanes pause, header explains,
   orchestrator asks a human for more or re-scopes.
7. **"What's happening?" must not cost tokens to compute.** A deterministic
   `STATUS.md` renderer (board + journal → text, no model) is the header's
   input; the header only phrases it. Testable, and the same text is the
   activity summary in the deck.
8. **Notifications.** An approval card for a `critical` transfer must reach a
   person who is not looking: OS notification from the client (Tauri has it)
   plus a badge on the workspace in the sidebar. No push service.
9. **Onboarding is a wizard, not a doc.** `parzi hub join` → clone the
   workspace repo (URL from the hub) → map each repo remote to a local path
   (offer to clone missing ones) → done. Under a minute or it is not lean.
10. **Grammar versioning.** `parzi: 1` on the first line of PROJECT.md and
    PLAN.md; parsers refuse unknown majors with a message, never silently.
11. **Trust is the team's, not the tool's.** Members run each other's plans on
    their own machines; the workspace repo is private on whatever host; LAN
    traffic is plaintext unless proxied. Say this in the docs in one paragraph
    instead of building an access-control system nobody asked for.
12. **Prove "effective" before shipping it.** One benchmark project (say, the
    checkout flow above) run two ways: today's single-agent thread vs the
    roster flow. Measure wall time, tokens per role, human interventions, and
    files touched outside scope. If the roster flow is not better on at least
    two of the four, H2+ waits.
