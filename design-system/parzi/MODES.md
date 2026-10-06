# Modes Plan — Search / Build / Research

Status: Research lane backend + mode switch UI built (2026-10-06).
Remaining here: side panel, history chips, LaTeX, CLI catalog.

## The three modes (locked)

- **Search** (blue, globe): the omnibar is an address bar. `chess.com`
  opens chess.com; anything else becomes a search-engine results page
  via `desk::normalize_url` (DuckDuckGo today). No model, no session.
- **Build** (green, bot icon): full agent, project folder optional. A
  picked folder adds cwd + brain context; no folder means scratch, like
  today. Never blocks on project picking.
- **Research** (amber, brain icon): read-only Q&A agent on the Settings
  quick-model. Never shows an approval card by construction: the lane
  allowlist excludes writes/shell, and vendor-native write tools get a
  silent `Deny("read-only mode")` with a lane brief telling the model
  to answer directly. Throwaway answers; Keep promotes to a session.

Research lives in two doors, one lane: the segmented control (dedicated
Q&A) and a docked side panel (`Ctrl+J`) for asking mid-build without
losing the thread. Panel answers have Keep / Copy / save-to-brain.

## P1 — Mode switch UI

- 3-way segmented control in the omnibar bar (replaces agent/web
  toggle), new `bot` icon (revisit: brackets glyph) in `icons.ts`.
- Composer tint per mode (subtle `color-mix` wash, no glow), colored
  mode icon, per-mode placeholder.
- Tab cycles Search→Build→Research; Ctrl+1/2/3 jump; `/search /build
  /research` slash commands; mode persists; per-mode model/effort
  memory for the window.
- Mode-mismatch hint: composer mode ≠ open session lane → one-line
  "Search — this won't reply in the session".
- Tests: mode-cycle unit test, svelte-check.

## P2 — Lanes

- `lane_policy_for(cfg, lane)` with per-lane allowlists; `search` lane
  is UI-only (no turn); `research` lane excludes `fs.write`,
  `shell.*`, session/lane tools; silent-deny path in `ToolHost::gate`
  for vendor write tools on that lane.
- Lane briefs in `system_parts`: Research citation + direct-answer
  brief; Build keeps today's brief.
- Plumb lane from composer (`sessions.rs` instead of hardcoded
  `"default"`); follow-ups keep the session's stored lane. Existing
  `"default"` sessions read as Build.
- Quick-model picker in Settings › Providers; Research + Search-fast
  default to it.
- History filter chips All / Build / Research (Sessions carry lane);
  Pages stay ungrouped; panel throwaways hidden unless Kept.
- MASTER.md documents modes.

## P3 — Research for uni (connected tabs, artifacts, LaTeX)

Research gets the same connected tabs Build has (`browser.open`,
`browser.tabs`, `browser.read` with text + controls) so it can open
sources itself and quote them. Click/type stay Build-only at first;
promote later if Research needs forms.

- **Artifacts**: Research answers over ~15 lines go out as
  `ui.show_artifact` documents (essays, comparison tables, timelines);
  add a "Research summary" shape if the generic document feels wrong.
- **Save to brain**: small button under Research/panel answers calls
  `brain.write` into a research notes path; never automatic.
- **LaTeX**: `md.ts` has no math support today. Add math rendering
  (markdown-it math plugin + KaTeX, DOMPurify allowlist for the math
  spans) so `$…$` / `$$…$$` render equations in answers and artifacts.
  Check bundle size (KaTeX fonts) before committing to client-side vs
  pre-rendered HTML.

## P4 — Research side panel

- `AskPanel.svelte` docked right, `Ctrl+J`, independent input, streams
  on the quick model with capped steps, ephemeral answers.
- Keep → creates a Research session with the transcript; Copy;
  save-to-brain. Esc/close discards. No session persisted on ask
  (regression test).

## Later — CLI catalog (see verdict 2026-10-06: feasible, suggest-don't-autoinstall)

- PATH scan exists (`process::resolve`/`find_on_path`); MCP manager
  spawns stdio servers from `mcp.servers` config with per-tool modes.
- A curated manifest maps known CLIs (gh, kubectl, docker, …) to MCP
  server packages. Onboarding/Settings proposes matches; the user
  toggles. Never auto-enable: context bloat + arbitrary-code risk.
- Non-MCP CLIs need a wrapper entry in the manifest or stay out.
