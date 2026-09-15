# Tools & connectors — mess inventory + rework plan

Status: proposal, 2026-09-15. Plan first, build after (same deal as `docs/projects-vision.md`).
Verdict: four permission layers with no single matrix, tool routing spread over six
must-touch-together lists, a hand-rolled STDIO-only MCP client, three overlapping
extension systems, and one genuine env-leak hole. Details below, then the plan.

## 1. Inventory: what's actually wrong

### 1A. Permissions: four layers, no single truth

1. **Lane allowlist** — global `lanes.default_allowed_tools` → project `parzi.toml` →
   lane `parzi.toml`, empty-means-inherit (`orchestrator.rs:581-608`). Deny by default.
2. **MCP exposure** — per-server `enabled` + `allow`/`deny` lists (`config.rs:95-143`).
3. **Per-tool approval override** — `tool_modes` auto/ask/deny/inherit. Only applies to
   MCP tools: `approval_override()` returns `None` for every builtin
   (`tools.rs:79-90`), which is correct but nowhere stated.
4. **Handler gate** (`handler.rs:523-547`) — lane Deny absolute → allowlist →
   per-tool override → lane mode → human approver. Except `ui.*` tools skip the gate
   entirely (`handler.rs:493-495`) and session/lane-spawn tools take a separate
   branch (`handler.rs:496-512`).

No single place shows the effective policy for "can agent X use `github.create_issue`
right now". The answer lives in the intersection of four checks across three crates.
The only documentation is a hint paragraph at the bottom of the Connectors page.
UI naming triples the confusion: **Turbo/Lockdown** (Agent tools page) vs
**auto/deny** (config + lane files) vs **Ask** (both). `patternAllows()` is
duplicated verbatim in two Svelte files.

### 1B. The allowlist lies to the model

`defs()` advertises **all** local/ui/session tools unconditionally; only MCP tools
are filtered by the allowlist (`tools.rs:92-122`). Enforcement happens later in
`execute()`. So a blocked `fs.write` is still offered to the model, which calls it,
burns a turn, and gets "not allowed in this lane" back in its transcript. Blocked
tools must be hidden from the advertised surface, not rejected after the call.

### 1C. `ui.show_artifact` falls between three stools

- Advertised in `defs()` (via `ui_defs`), toggleable in the catalog.
- **Not** in the orchestrator's force-allow list — only markdown/widget/diagram are
  (`orchestrator.rs:609-614`).
- **Not** in the UI's `ALWAYS_ON` set either (`AgentToolsSection.svelte:23-36`),
  while the Display group blurb still claims "Always on."

So the artifact tool — the one agents use for real code output — is the only display
tool that can silently stop working depending on which of three lists you believe.

### 1D. Adding one tool touches six places

`is_local` / `is_ui_tool` / `is_session_tool` / `is_plan_tool` / `is_lane_tool`
predicates + `defs()` + `builtin_tools()` catalog + orchestrator force-add +
`ALWAYS_ON` + `GROUP_ORDER`. The existing test (`tools.rs:463-481`) pins
defs↔catalog parity but none of the other four. `session_defs()` is a grab-bag —
it contains `plan.*`, `lane.dispatch` and `knowledge.*` alongside `session.*`
(`tools.rs:286-399`). The dotted-name dispatch in `execute()` tries local-first
then MCP inside a redundant `split_once('.').filter(|_| contains('.'))`
(`tools.rs:134`), so every future dotted builtin is one missed predicate away from
being misrouted to an MCP server lookup.

### 1E. Fresh installs ship with file access off (verify before build)

`default_allowed_tools` defaults to empty (`config.rs:206`), and the force-add list
covers only the 12 teamwork/plan/knowledge/display tools — not `fs.*` or
`shell.exec`. Unless a project/lane file says otherwise, a brand-new agent cannot
read a file until the user discovers the Agent tools page. Secure default, terrible
first run. P1 must decide: sensible-default bundle vs explicit opt-in (see §4 Q1).

### 1F. MCP transport: hand-rolled STDIO only

- `mcp.rs` is a bespoke newline-delimited JSON-RPC client. No Streamable HTTP /
  SSE (PLAN.md §7 promised it; the code comment claims deliberate deviation).
- `request()` reads **one line** — any server that interleaves log lines on stdout
  corrupts the stream. `stderr` is `null`, so diagnostics are discarded instead.
- `tools/list` cached forever per process; no invalidation except restart
  (`set_configs` only drops removed/disabled servers).
- `defs_with_mcp()` loops servers **sequentially** with a 12s timeout each
  (`tools.rs:103-121`) — run start can block N×12s with several servers. Doctor
  uses 15s per server (`doctor.rs:145`) with a **fresh** `McpManager`, so probes
  neither warm the live cache nor share it — double spawns.
- No roots, sampling, resources, or prompts. Three different timeouts (12s defs /
  15s doctor / per-server `timeout_ms`) with no stated relationship.

### 1G. MCP children inherit the full parent env (the real hole)

`shell.exec` scrubs the environment down to an allowlist and documents that
children never inherit provider keys (`tools.rs:719-735`). `McpManager::ensure_live`
does **not** scrub — `Command` inherits the whole parent env plus cfg env
(`mcp.rs:158-165`). Every MCP server process sees every provider key in Parzi's
env. Either scrub both or justify the difference; current state is inconsistent,
and the scrubbed one proves the intent was to scrub.

### 1H. Extension sprawl: three systems, one dead kind

Plugins (`commands`/`theme`/`mcp-pack`) + Skills (paste SKILL.md / git → commands
packs) + MCP presets (15 hardcoded `npx` entries in UI). `mcp-pack` is accepted by
the manifest validator and **never loaded anywhere** (confirmed by the 09-14
audits: `audit/2026-09-14/runtime.md` P2-9, `security.md` §3, `tests-conformance.md`).
Settings taxonomy mirrors the sprawl: **Connectors** vs **Agent tools** vs
**Skills**, with permission explanations split across two pages.

### 1I. Scoping gaps

- Connectors are global-only. Per-project/per-lane connector policy exists on disk
  (`parzi.toml` allowlists) but has **no UI** — hand-edit only.
- No per-role tool policy (Header vs Builder share one surface).
- MCP `env` secrets live in plaintext `config.toml`; the keyring integration built
  for providers was never extended to connector secrets.

## 2. Target design

**One registry.** A single `ToolRegistry` owns every tool: name, group, description,
schema, kind (builtin-fs / builtin-teamwork / builtin-display / connector), and
which policy inputs apply. The six lists (§1D) become views over the registry.
One test asserts registry↔advertised↔catalog↔UI-constants parity — the current
catalog test grows teeth instead of adding a seventh list.

**One permission pipeline, evaluated in one function, explainable in one UI row:**

```
scope allowlist (global → workspace → project → lane, nearest non-empty wins)
  → exposure (connector allow/deny, enabled)
  → mode (lane auto/ask/deny; connector per-tool override for connector tools only)
  → human approval (Ask mode / approval card)
```

`evaluate(tool, lane) -> Allow | Ask | Deny + reason`. The Connectors/Agent-tools
rows render the verdict **and the reason** ("allowed via `github.*`", "blocked by
lane Lockdown", "needs approval"). The advertised tool list is the output of this
pipeline — never advertise what you will reject (§1B). Lane Deny stays absolute
(keep R-8). Resolve the Turbo-punches-through-Lockdown asymmetry explicitly:
either per-tool Auto beats lane Deny (documented, loud) or it doesn't (current
code says it doesn't — then the UI hint claiming otherwise gets fixed).

**Transport abstraction.** `McpTransport` trait with `StdioTransport` (current
client, hardened: stderr capture, notification-skipping reads, list invalidation)
and `HttpTransport` (Streamable HTTP) behind it. One timeout policy
(per-server `timeout_ms`, one code path). Doctor probes through the **live**
manager so probing warms instead of duplicating.

**One extension story.** Skills, presets, and `mcp-pack` converge: an installable
**pack** = slash commands + connector definitions + optional theme, one manifest,
one installer (paste / git / one-click), one list UI. Either implement `mcp-pack`
loading or delete the kind — no third option. Settings collapses to
**Capabilities** (what agents can do: built-ins + connector tools, with the
verdict UI) and **Packs** (how new capabilities arrive).

**Secrets.** Connector env secrets move to the keyring with presence-only display,
same pattern as provider keys. Plus: scrub MCP child env to match `shell.exec`
(§1G) — do this first, it's a one-spot fix.

## 3. Phased plan

- **P0 — safety (hours, no behaviour change).** Scrub MCP child env (§1G).
  Fresh-manager doctor stays, but document why. Acceptance: `shell.exec` and MCP
  spawns have identical secret posture; test asserts it.
- **P1 — honesty (no new features).** Advertise only allowed tools (§1B); fix the
  `ui.show_artifact` triple inconsistency (§1C) one way or the other; unify
  Turbo/Lockdown/auto/deny naming in UI; dedupe `patternAllows`. Decide the
  fresh-install default (§1E). Acceptance: blocked tools never appear in defs;
  catalog test covers all six lists; first-run agent can read files (or an
  explicit, visible choice says why not).
- **P2 — one pipeline.** Implement `evaluate()` (§2), render verdict+reason rows
  in a merged Capabilities UI, collapse the duplicated permission prose into the
  rows themselves. `session_defs` split into honest groups. Acceptance: every
  tool row explains its own state; handler/executor call the same function the
  UI renders.
- **P3 — transport.** Trait + hardened STDIO (stderr capture, resync reads, list
  invalidation, single timeout path); doctor via live manager; sequential defs
  fan-out with a global budget. HTTP transport behind the trait (or a dated,
  explicit deferral). Acceptance: chaos test (chatty server, slow server, dead
  server) green; N-server startup bounded.
- **P4 — packs + scoping.** `mcp-pack` implemented or deleted; Skills/presets/
  plugins converge on one installer + one list; per-project connector scoping UI;
  connector secrets to keyring. Acceptance: install anything three ways, see it
  once; scope a connector to one project from the UI.

Each phase: `cargo test`, `cargo clippy -- -D warnings`, UI smoke of the touched
surfaces. No phase starts until the prior acceptance passes (LOOP.md rules).

## 4. Open questions for Lucas

1. **Fresh-install default:** agents-can-read-files out of the box (sensible
   bundle on, user locks down), or default-deny (current, user opts in)?
2. **Unify extensions** (one Packs concept for skills + presets + plugins), or
   keep Skills and Connectors separate and just kill `mcp-pack`?
3. **HTTP MCP now or defer?** Is any must-have connector HTTP-only for you, or
   can P3 ship hardened-STDIO first with HTTP as a dated follow-up?
