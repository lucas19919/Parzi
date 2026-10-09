# Modes Plan — Search / Build / Work

Status: Build + Work lanes, switch UI, dock, shell ownership shipped
(2026-10-07). Remaining: decompose proposal, track staffing, CLI catalog.

## The three modes (locked)

- **Search** (blue, globe): the omnibar is an address bar. `chess.com`
  opens chess.com; anything else becomes a search-engine results page
  via `desk::normalize_url` (DuckDuckGo today). No model, no session.
- **Build** (green, bot icon): full agent, project folder optional. A
  picked folder adds cwd + brain context; no folder means scratch, like
  today. Never blocks on project picking.
- **Work** (amber, brain icon): everything that isn't building software
  — studying, writing, email, admin, onboarding tools. Scholar identity
  for math/science, assistant identity for the rest. Never shows an
  approval card (no shell, so nothing gated), runs the Settings
  quick-model, low effort default. May write notes/docs, generate
  images, staff read/write subsessions, and create real projects
  (a paper is a project). Model and effort fully pickable; quick/low
  are defaults, never locks.

Work lives in two doors, one lane: the segmented control and a docked
side panel for asking mid-build without losing the thread.

## Shipped (was P1–P4)

- Switch UI: segmented Search/Build/Work, Tab cycle, Ctrl+1/2/3,
  slash commands, per-mode model/effort memory, session lane lock.
- Lanes: per-lane allowlists, session lane inheritance, quick-model
  default on Work, legacy `code`/`research` ids normalize.
- Work uni kit: connected tabs, artifacts, KaTeX, save-to-brain,
  citation convention in the brief.
- Dock: Agents/Projects/Tasks tabs, resizable, session-scoped.
- Shell ownership (see SHELL.md); Work has no shell by design.

## Remaining

- `task.decompose` proposal flow, track staffing with status sync.
- CLI catalog (verdict 2026-10-06: feasible, suggest-don't-autoinstall).

- PATH scan exists (`process::resolve`/`find_on_path`); MCP manager
  spawns stdio servers from `mcp.servers` config with per-tool modes.
- A curated manifest maps known CLIs (gh, kubectl, docker, …) to MCP
  server packages. Onboarding/Settings proposes matches; the user
  toggles. Never auto-enable: context bloat + arbitrary-code risk.
- Non-MCP CLIs need a wrapper entry in the manifest or stay out.
