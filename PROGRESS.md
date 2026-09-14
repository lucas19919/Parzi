# Parzi — build progress (LOOP.md execution log)

## P0 — accepted (2026-09-09, attempts: 1)
- workspace + 3 lib crates + CLI + detached src-tauri + ui scaffold
- `cargo check --workspace` green; Asuka seeded as default background
- evidence: `assets/backgrounds/asuka.png` (609KB), `examples/backgrounds/asuka.png`

## P1 — accepted (2026-09-09, attempts: 2)
- core: config v1, theme (CSS vars + asuka default), store (meta/events.jsonl/session.md),
  lanes scan, context builder, widget/diagram validators
- fix loop: missing `atomic_write` re-export + paths type error — both fixed, re-green
- evidence: `cargo check -p parzi-core` green

## P2+P3 — accepted (2026-09-09, attempts: 2)
- trait + OpenAI-compat engine + anthropic native + codex (Responses) +
  claude-code (OAuth) + antigravity (ported MIT logic: fallback, wrap, thinking-strip,
  schema allowlist) + catalog + router for 9 ids
- fix loop: dyn-compat via async-trait + dirs dep
- evidence: `cargo check -p parzi-providers` green; 4 schema/catalog tests pass

## P4 — accepted (2026-09-09, attempts: 1)
- handler state machine (stream → approve → execute → loop), budget, cancel,
  crash-safe appends, ui.* widget tools; orchestrator spawn/kill/fork + limits + recover
- evidence: compiles; kill/fork paths exercised via CLI (P6)

## P5 — accepted (2026-09-09, attempts: 2)
- tools (fs.read/write/list, shell.exec, sandbox escapes rejected, allowlist deny-default),
  native STDIO MCP client (lazy spawn, cached list, idle reap), plugins (manifest packs)
- DEVIATION (documented): hand-rolled NDJSON JSON-RPC client instead of rmcp —
  fewer deps, deterministic, same protocol. rmcp re-integration optional, never required.
- fix loop: RunEvent Clone (oneshot), sid move, toml dep
- evidence: 5 allowlist/sandbox tests pass, incl. `../../secret` rejection

## P6 — accepted (2026-09-09, attempts: 2)
- CLI: init/list/show/export/send/fork/kill/doctor/models, id-prefix resolution,
  terminal prompt approver, `--yes` flag
- fix loop: drift-proof token scrape flipped opencode+codex to ok on real files
- evidence: `parzi init` + `parzi doctor` live run — dirs/config/theme ok,
  4/9 providers authenticated with zero configuration
  (ollama local, opencode, codex, claude-code), webview2 152 detected

## P7 — accepted-code, PENDING HUMAN GATE (screenshot parity)
- Tauri shell compiles (`cargo check` green in src-tauri): 14 commands, GuiApprover
  with 120s cap, event forwarding, asset-protocol background, capabilities
- fixes: protocol-asset feature, BMP-format icon.ico (old rc.exe rejects PNG ICOs),
  Model Serialize, Asuka icon generated from default background
- DEVIATION: 14 commands vs 10 cap (background_url, save_key, list_plugins,
  toggle_plugin are load-bearing for the UI contract)

## P8+P9 — accepted-code (2026-09-09)
- UI `npm run build` green: sidebar+search, glass bar, thread view, runs table,
  5 settings tabs, md-it+DOMPurify+hljs reader, 7 widget types, SVG diagrams,
  9 provider logos, theme CSS vars + user.css tail
- fix loop: svelte-preprocess, `{@const}` TS-cast restriction, unused export
- KNOWN: bundle 1.1MB (full highlight.js) — P10 follow-up is per-language imports;
  Ctrl+K fuzzy palette is structural (search ships, full palette next)

## P10 — accepted (2026-09-09)
- `cargo test --workspace`: 15/15 pass (6 core + 4 providers + 5 runtime)
- clippy gate (correctness/suspicious/complexity/perf deny): clean
- release: `target/release/parzi.exe` = 4.9MB (opt-z + lto + strip)
- style/pedantic stays advisory by decision (JSON `and_then(|x| x.as_str())` idiom)

## QoL + catalog depth (2026-09-09, t3code-informed)
- Capability overlay (t3code ModelManifest pattern): tools/vision/legacy/default
  per model; handler strips tools for incapable models with a system note;
  legacy badges in the model menu; `catalog_refresh` kill-switch in config
- Manifest discipline: bundled truth + live /models merge + `catalog.json` disk
  cache (24h TTL) + never-fail fallback chain — offline keeps last-seen models
- Appearance packs: save/apply/gallery in Settings (themes live under
  ~/.parzi/themes/), new Tauri commands list_packs/save_pack/apply_pack
- Command palette on Ctrl+K: actions + threads + models, full keyboard nav
- Copy-transcript button in the breadcrumb; hljs github-dark code theme;
  toasts on startup failure (no more silent dead UI)
- Verified: 18/18 tests, clippy gate clean, UI builds, tauri check green

## Glass v3 functional (2026-09-09, recompiled clean)
- Recompiled baseline green first, then built on it
- Moodier defaults (sidebar #07070B, dim 0.66, vignette 0.5) + built-in packs
  seeded on first run: asuka-retro + sleek-dark (solid, no wallpaper)
- Backgrounds are savable: gallery with thumbnails, path upload, solid toggle,
  guarded reads (ext + size caps, t3code discipline); live-apply without restart
- Runs view: per-provider groups with logos + counts, sorter
  (status/tokens/cost/model), empty state, cost column
- Lab logos redrawn: OpenAI blossom, Anthropic A, Google G (monochrome,
  nominative use); claude-code reuses Anthropic mark
- First-run onboarding: 3-step card (project → model → ask) with live status,
  dismiss persists; only shows on empty homes
- Verified: 18/18 tests, clippy gate clean, UI builds, tauri check green

## Lag kill + T3 form + 10-point spec (2026-09-09)
- Perf root causes removed: sidebar/menu backdrop-blur gone (solid translucent),
  grain blend-mode gone, blur kept only on composer + small popovers.
  This is what froze dragging — verify drag is smooth now.
- T3 Code form copied (minus top junk): channel rows (Threads/PRs-soon/
  Automations-soon/More), neon active marker, workspace footer card,
  breadcrumb + run status line, right-side user bubbles, task-style tool cards
- Spec 1: micro-header (workspace · branch · tokens) + jewel send bloom
- Spec 2: velvety fade masks on the scroll
- Spec 3: tool micro-cards (icon per family, running pulse, ✓ms/✗ms pills,
  accordion outputs) — timing plumbed from handler (new ms field)
- Spec 4: provider ambient glow (Claude amber, OpenAI emerald, Gemini indigo,
  DeepSeek cyan, Ollama moonlight), 600ms transitions
- Spec 5: halftone grade + deeper vignette + reading defocus (blur 3→8px)
- Spec 6: reasoning rail (new Reasoning event end to end: antigravity thought
  parts → handler → store → CLI → UI accordion, auto-open while live)
- Spec 7: channels + neon marker + workspace footer (above)
- Spec 8: code headers (ext badge + line count + copy), gutter numbers,
  diff +/- tint, 30-line clamp with expander
- Spec 9: serif-italic metallic hero accent + tabular numerals everywhere
- Spec 10: neon caret, press-scale springs, focal dim behind menus
- Pack art automatch: applying a pack points the theme at its art
- Version stamp in sidebar footer + About (proves which build is on screen)
- Verified: 18/18 tests, clippy gate clean, UI builds, tauri check green

## agy-driven functionality (2026-09-09)
- Asked `agy`: `agy models` works on your machine and exposed the REAL catalog
  (gemini-3.8/3.7/3.6-flash × high/med/low, 3.1-pro × high/low, sonnet-4-6,
  opus-4-6-thinking, gpt-oss-120b-medium) — catalog replaced stale 4.5-era ids
- Effort pill now MEANS something on Antigravity: family bases map to the
  -low/-medium/-high variant the backend serves (ChatReq.effort end to end,
  CLI --effort flag, orchestrator derives budgets centrally)
- Per-message model switch: picking another model mid-thread moves the thread
  (SessionStore.set_model), no more silent old-model runs
- First send now works: composer defaults to the first authenticated provider
  instead of unauthed openai/gpt-5 erroring out
- Slash commands: /new /fork /kill /clear /help /model /effort (local, instant)
- Settings Models tab: refresh button re-probes all providers
- Verified: 19/19 tests, clippy gate clean, UI builds, tauri check green

## Review: parallel overhaul audit (2026-09-09)
- Verified every claim in the Antigravity session's changes_summary.md: window
  capabilities + native commands + Titlebar/Sidebar/TaskPlanningView/Omnibar
  split + chronological Thread + Settings provider split + 19/19 tests +
  UI build + tauri check + release binary — all reproduce green here.
- Work was additive, not destructive: reasoning events, tool ms, packs,
  backgrounds, palette, onboarding, t3/antigravity adapters all intact.
- Hardened the new task system (was the weakest spot):
  path-traversal guards on project/task/subfolder (delete_task could escape
  the store before this), unique task ids (same title no longer overwrites),
  TaskStatus enum instead of free string, errors propagate instead of
  `let _` swallowing, pure slugify/uniquify extracted + tested
- New tests: task id slug/uniquify, TaskStatus default/parse (21/21 total)
- Full gate re-run: tests + clippy + UI build + tauri check all green

## Smart cockpit: router + bar (2026-09-09)
- Router brain (new providers::router): `auto` tier — subscriptions first
  (antigravity/codex/claude-code/t3/opencode), paid keys next, ollama terminal
  fallback; effort-aware picks per provider; unauthed skipped, never fails
- Failover: 429/overload retries the next slot with a timeline badge
  ("Switched to codex/gpt-5.3-codex (rate limit reached)"); auth/config
  errors never fail over. New RunEvent::Notice (toast-only) end to end.
- Thinking budgets native: Anthropic thinking param (clamped under max_tokens,
  sonnet/opus only), Codex reasoning_effort, Antigravity variants (existing)
- Families: 14 antigravity rows collapse to 7 family rows; effort pill resolves
  the variant under the hood; legacy/variant metadata in catalog + cache
- Model menu: ⚡ Smart Auto row, Subscriptions/API/Local sections, no-key
  states route to key settings, full keyboard nav, provider filter + sorting
- @ file picker restored in the componentized bar (chips, 12k caps);
  / command menu (/plan /auto /model /effort /new /clear /fork /kill /doctor
  /help) wired to real actions; clickable task chip + chat/plan/build mode
  badge (plan mode is a real prompt prefix, not decoration)
- Verified: 26/26 tests (incl. auto ordering, effort picks, retriable
  classification, family collapse), clippy gate clean, UI builds, tauri green

## Skill audit: ui-ux-pro-max (MIT) applied (2026-09-09)
- Installed nothing; used the library's checklist + Master/Overrides pattern.
- Audit result: cursors covered (39), SVG icons already, chips wrap, toasts
  everywhere. Fixed the 3 real violations: ⚡ emoji icon → SVG bolt,
  missing :focus-visible rings, no prefers-reduced-motion support.
- Wrote design-system/parzi/MASTER.md: tokens, glass discipline (perf rules
  learned the hard way), components + states, a11y, anti-patterns, checklist.
  All agents (including parallel) build from this now.
- Verified: UI builds clean.

## Skill research round 2 (2026-09-09)
- Installed ui-ux-pro-max globally (repo untouched); ran its generator +
  glassmorphism/Svelte guidance queries. Direction confirmed; took two
  surgical rules, skipped the rest (identity already locked).
- Restored the regressed command palette (rewrite dropped it): actions +
  threads + models, full keyboard nav, F11, Esc hierarchy (palette →
  modals → stop run).
- Semantic-element pass per skill guidance: chip-x, pin stars, empty-hint
  → real buttons; `|local` on all list transitions; thread rows documented
  as the deliberate div+role exception (nested actions).
- MASTER.md updated with both rules.
- Verified: UI builds clean.

## Antigravity-form pass + queue mode (2026-09-09)
- Diagnosed the flat stage from your screenshot: your live theme.toml had
  dim 0 + vignette 1.0 + white accent (sliders can dig that hole) — the
  wallpaper was loading but buried. Asuka file itself is present and healthy.
- Queue mode backend: SessionStatus::Queued, queue_when_busy (default on),
  shared pump (Notify-driven, headless, AutoApprover), kill dequeues + pumps,
  CLI stays loud-reject. Queued dot style + queued toast in UI.
- Settings restructured toward the Antigravity sections: General (queue
  segmented, approval Turbo/Ask/Lockdown, steps, shell toggle), Application
  (build, counts, catalog refresh, diagnostics), Appearance/Models/
  Connectors/Plugins kept, Projects (roots/lanes/SYSTEM), Conversations
  (counts + purge finished), Shortcuts, Feedback. Plus get/save_config,
  reset_theme, purge_sessions commands.
- Pack art automatch: applying a pack points the theme at its art.
- Env lesson: vite binds IPv6 ::1 here — probe via localhost, not 127.0.0.1.
  Also: kill stale parzi-app processes before relaunch or dup windows pile up.
- Verified: tests + clippy + UI build + tauri check green.

## Review: Gemini plan implementation audit (2026-09-09)
- The parallel session implemented the full plan; verified claim by claim:
  sidebar CTA/workspace/unified history/bottom dock, 4-tab settings, slider
  percent-conversion fix, masked keys + Enter-to-save, OAuth card +
  login_antigravity backend command, modal + Esc/Ctrl+N/Ctrl+,.
- All gates re-run green: 20 tests, clippy clean, UI builds, tauri check.
- One touch-up: model-pill logo now renders at text color (was faint dim).
- Note: authorizationUrl-style Google sign-in still needs YOUR live click
  in Settings → Models to prove end to end.

## Pixel-match rebuild: Antigravity sidebar + composer (2026-09-09)
- Deleted and rewrote Sidebar.svelte (1096 lines) and Omnibar.svelte from zero
  against your two screenshots: collapse + back/forward thread history,
  New Conversation, Conversation History (flat mode), Scheduled Tasks (live
  badge → runs), Projects header with filter/new actions, per-project collapse
  + hover new-conversation, title…time rows, See all (N), Settings + version.
- Composer: breadcrumb, card, +/model/effort/circle-send, source strip that
  opens the picker, all logic preserved (@, /, families, kb nav, autosize).
- Deliberate deviations (no fake controls): no mic (no voice backend), no
  back/forward IDE nav (thread selection history instead), effort segmented
  kept (our approved differentiator).
- theme.css left intact: every component carries scoped styles; global sheet
  is tokens + shared render styles (code/widgets/toasts/modals). Deleting
  blind would break the parallel agent's surfaces — dedup needs coordination.
- Verified: UI builds, svelte-check 0 errors (was 8; warnings 47→39),
  20 backend tests, clippy gate clean.

## T3 picker + provider-aware effort + shell fixes (2026-09-09)
- Model menu rebuilt to the T3 screenshots: search, Smart Auto, Favorites
  section, Subscriptions/API/Local groups, colored brand marks (OpenAI white,
  Anthropic terracotta, Google 4-color, xAI white, ollama white, antigravity
  gradient), tier right, Ctrl+1..5 on rows, star toggles persisted to
  config.toml favorite_models (cap 9).
- Effort is provider-aware now: backend effort_options per provider
  (antigravity variants, anthropic thinking budgets, codex reasoning levels,
  output budgets elsewhere); segmented control renders per current provider
  with native-meaning hints; thinking_budget centralized in router.
- Burger button collapses the sidebar (was wrongly opening Runs); slim rail
  reopens it. Project + confirms with a toast naming the project.
- Wallpaper root cause FIXED: overlay styles were hardcoded (0.35 opacity +
  75% dim) ignoring the theme — now driven by theme vars (blur/dim/vignette
  sliders work again). Your live theme (dim 0, vignette max) still crushes
  edges — click asuka-retro once.
- Verified: UI builds, 0 check errors, 21 tests, clippy gate, tauri check.

## Glass return + chrome fixes (2026-09-09)
- Sidebar and composer are glass again (narrow-strip 14px + card 20px blur;
  fullscreen blur layers stay off for the lag fix).
- Fullscreen button removed from titlebar (F11 shortcut stays); frameless
  fullscreen/maximize now drops the rounded root corners (was black wedges)
  via live is-full sync on resize.
- Top-left tidied: spacing + proper titles on thread-history nav.
- Verified: UI builds, 0 check errors.

## Shell layout pass (2026-09-09)
- Sidebar runs full height; titlebar moved over the stage; project/session
  breadcrumb always visible (was empty with no active thread).
- Collapse is a smooth slide now (stays mounted, margin + opacity spring)
  instead of the instant unmount snap; rail is top-aligned and fades in.
- Top-left: burger → serif Parzi wordmark → thread-history nav right-aligned;
  sidebar top doubles as drag region.
- Verified: UI builds, 0 check errors.

## Human gates still open
- [ ] P7 screenshot parity (needs your eyes on `cargo tauri dev`)
- [x] P9 updater signing keys — keypair generated 2026-09-14, pubkey wired into
  tauri.conf.json, `.github/workflows/release.yml` ships signed NSIS+MSI +
  latest.json on tag. Still manual: create the GitHub repo (or retarget the
  endpoint), `git init` (no repo yet), add `TAURI_SIGNING_PRIVATE_KEY` secret,
  push a `v*` tag, publish the draft release.
- [ ] Windows SmartScreen trust (Authenticode cert / Azure Trusted Signing —
  separate from updater minisign; unsigned installers warn but install)
- [ ] Antigravity live-token test (opt-in; TOS warning shipped in UI + CLI)

## Style pass + T3 refinement (2026-09-09)
- Pinned threads: `SessionMeta.pinned`, `toggle_pin` command, ★ section in sidebar
- Effort pill: low/med/high → 4096/16384/65536 max_tokens, plumbed handler→Tauri→UI
- Model pill: provider logo SVGs (10 incl. t3) + friendly names + cost tiers
  (free/$/$$/$$$, thom-chat pattern) in dropdown and Settings
- Thread rows: subtitle (model · rel time · tokens) + group counts
- Wallpaper: static film-grain overlay (SVG noise, zero per-frame cost)
- `t3` provider via t3router protocol (MIT, attributed, ToS-warned, paid-sub required)
- Verified: 15/15 tests, clippy gate clean, UI builds, tauri check green
## UI v2 Glass (2026-09-09 night, from your mockup)
- Mockup at `ui/design/parzi-redesign-mockups.html` implemented: 248px glass
  sidebar, Instrument Serif wordmark, Ctrl-K search, gradient New-thread,
  glowing status dots, active-row gradient + inset bar, live counter footer
- Home: 32px hero headline, centered 720px darker-glass composer (r16),
  suggestion chips, breadcrumb bar (project/thread + auth line + tok/cost + Fork)
- Model menu 420px: search, provider status dots, selected highlight, tier mono,
  "no key" labels, Manage-keys footer, full keyboard nav, spring entrance
- Thread: glass user bubble, tool lines with icons, code headers (lang + copy),
  pulsing "writing…" row, message copy buttons, toast stack for errors
- Motion: svelte fade/fly/scale, spring easings, hover lifts, shelve slide
- Frameless: `decorations:false`, custom titlebar (drag region, F11, min/max/close)
- Wallpaper grade: dim + purple grade + vignette + accent glow + overlay grain
- Verified: UI builds, 17/17 tests, clippy gate clean, tauri check green

## Antigravity first-class (t3code-informed, MIT)
- Real OAuth constants (attributed), version-pinned + rotating headers,
  refresh-token flow with 401 auto-retry, `parzi login` (Google sign-in via
  localhost callback, explicit opt-in) + `parzi logout`, tool-use hardening
  in the lane system prompt
- t3code steal list (all MIT, patterns only): auth state machine, no-scrape
  credentials, user-facing failure copy
- Still needs YOUR live test: `parzi login` → `parzi doctor` → send via antigravity

## Overnight overhaul (2026-09-09 night)
- Sidebar: brand mark, icon search, primary New-thread button, pinned ★,
  hover actions (pin/rename/fork/kill), capitalized groups + counts
- Composer: project/lane pills + project menu (lanes, roots, inline create),
  model menu (search, logos, tiers, auth dots, manage-keys footer),
  Low/Med/High effort pill, @ autocomplete + chips, stop button, Esc/Ctrl+K
- Thread: user bubbles, per-message copy, auto-follow scroll, hero empty state
- Settings: working Appearance (sliders/color/dimmer/glass), key show/hide,
  connectors, plugins, about+diagnostics — all wired to the same tomls
- Projects are real: root per project/lane, @-files + default cwd resolve from root
- `t3` provider + named catalog + tiers (see Style pass entry)
- Bundled Inter + JetBrains Mono (no more system-font fallback)
- Sleek pass: glass top-edge light, accent edge on active thread, hairline
  dividers, focus rings, custom scrollbars, refined message rhythm —
  Antigravity discipline, retro glass soul kept
- Verified: 15/15 tests, clippy gate clean, UI builds, tauri check green
- Still open for morning: screenshot parity vs inspo, updater signing,
  Antigravity + t3 live-token tests, PR/Automation rows + worktree pill (cut)

## Theme Packs, Per-Provider Sorter & Moody Overhaul (2026-09-09)
- Theme packs (t3code style): 9 built-in presets (Moody Midnight, Tokyo Night, Catppuccin Mocha, Dracula, Cyberpunk Noir, Nordic Frost, OLED Black, Emerald Matrix, Rose Pine) with palette dots and active indicator
- Wallpaper manager: background selector gallery, custom image file upload (`save_background_data` Tauri command), solid moody gradient option, and interactive dim/vignette/blur/radius sliders
- Per-provider agent sorter: Provider pill filter bar (All, Anthropic, OpenAI, Google, xAI, OpenRouter, Ollama, Antigravity, etc.) + sorting by Cost/Tier, Context window limit, and Alphabetical (A-Z)
- Authentic AI lab vector logos: replaced placeholder shapes with official SVGs for OpenAI, Anthropic/Claude, Google Gemini, xAI/Grok, DeepSeek, Meta/Llama, Mistral, Groq, Ollama, OpenRouter, Antigravity, and T3
- Standardized color system: eliminated hardcoded `rgba()` values in `theme.css`; all surfaces (sidebar, glass bar, menus, bubbles, glow) derive dynamically from `--parzi-*` CSS custom properties
- Verified: `npm run build` green, 18/18 workspace tests pass, `cargo check --manifest-path src-tauri/Cargo.toml` clean, `parzi-app.exe` binary recompiled


## UI v3 � T3 sidebar top + settings stage-mode (2026-09-11)
- Sidebar top replaced with T3 block: Search row (opens palette, edit icon = new
  conversation), All-projects scope row (folder + chevron dropdown + new-workspace
  plus), + New Conversation, Conversation History (flat toggle), Scheduled Tasks
  (live badge). Burger/wordmark/thread-nav arrows removed; collapse is a
  hover-reveal chevron (top-right) + existing rail.
- Settings is now T3-style stage mode: sidebar swaps to SettingsNav
  (General/Appearance/Models/System + search + auth badge + back), stage shows the
  section via Settings bareSection; modal dialog deleted. openSettings(section)
  helper routes /doctor /keys /palette entries to the right section.
- Evidence: `npm run check` 0 errors (35 warnings), `npm run build` green 4.4s.
- NEXT: delete Task system end-to-end; top-crumb replacement; darker defaults +
  composer tone; collapsed-rail polish; full verify + relaunch.

## Artifacts + widget render + tasks removal (2026-09-12)
- Artifacts (new, v1 code-only HTML): `parzi-core/src/artifacts.rs`
  (validate/slug/version/dedup, 64k cap, 8 kinds, 13-lang allowlist),
  `Event::Artifact` end to end (store/context/compact/session.md read_session),
  `ui.show_artifact` tool + version bump + same-content dedup, heuristic
  auto-save of long fenced blocks (off by default, `with_auto_artifacts`),
  system-prompt lines, `parzi-artifact` fence in `splitSegments`.
- Widgets fixed: stored `Widget` events now render in `Thread` (were dropped);
  `markdown` is a validated kind (was bypass); chart point cap 200; `Widget`
  + `Diagram` rebuilt as glass cards with headers, empty/error/cap states,
  theme vars only (no hardcoded rgba); `ArtifactCard` with copy/save,
  version badge, 30-line expander; route/system lines render in thread.
- Tasks deleted end-to-end: `TaskConfig/Status` + CRUD gone from `lanes.rs`
  (kept one-time `migrate_tasks_to_lanes`), 4 Tauri commands collapsed to
  `migrate_tasks`, `TaskPlanningView.svelte` deleted, Sidebar/App/api
  task-free (`/plan` now sets Plan mode, cwd = project root).
- IPC: 14 -> 11 commands (migrate_tasks is the only task remnant).
- Verified: `cargo test --workspace` 53 pass, clippy gate
  (correctness/suspicious/complexity/perf deny) clean, `npm run check`
  0 errors, `npm run build` green, `cargo check` green in src-tauri.
- Clippy drive-bys fixed to hold the gate: `type_complexity` allow in
  theme buckets, `too_many_arguments` allow on `HarnessBridge::spawn_session`,
  `Ok(x?)` -> direct return in orchestrator.

## Sidebar selection fixes (2026-09-12)
- Pinned threads vanished on pin (`threadsFor` excluded pinned, no pinned
  section rendered): added a ★ Pinned section above projects.
- New `ThreadRow.svelte` (one row component for pinned + tree rows):
  valid HTML (`div[role=button]` instead of `<button>` containing spans +
  input), keyboard Enter/Space selection that ignores inner controls,
  autofocused rename input, hover actions also show on `:focus-within`.
- Auto-reveal: selecting via palette/subsessions uncollapses the project,
  lifts the See-all clamp, opens collapsed ancestor subtrees, scrolls the
  row into view (`revealedFor`-guarded, no reactive loops).
- Filter no longer hides matches behind the 6-row clamp; See-all hidden
  while filtering.
- App drops stale selections (purged thread) in `loadThreads` without
  touching draft input.
- Verified: `npm run check` 0 errors, `npm run build` green.

## Project overview on select (2026-09-12)
- New `ProjectOverview.svelte`: header (name, branch, root, live count,
  lane chips with modes), "Live now" section (active + queued roots with
  subsessions nested at any depth, stop/new-subsession actions), "Recent"
  section (idle/done/killed, latest 30 roots with children), empty state
  with New-conversation CTA. Click any row to open the thread.
- `App.svelte`: explicit project picks (`switchProject`) set
  `projectSelected` and render the overview; `newThread` returns to the
  home hero; thread view still wins when a thread is open.
- Verified: `npm run check` 0 errors, `npm run build` green.

## Right inspector deck: Docs + Agents (2026-09-13)
- New `ui/src/lib/inspector/`: `RightPanel.svelte` (frosted docked split,
  340–680px drag/keyboard resize on the left grip, Docs/Agents tabs,
  auto-reveal toggle, close), `DocReader.svelte` (quick tabs for project
  `SYSTEM.md` + workspace `*.md`, `session.md` transcript, native file
  picker; artifact toolbar with kind badge, version menu, line count,
  copy/save, Preview/Raw; heading TOC for markdown; fences never clamped),
  `AgentVisualizer.svelte` (SVG swarm tree with pulsing live rings and
  dashed `session.send_message` vectors, process table per PLAN §6 with
  Focus/Fork/Kill/Spawn, live tool trace + inline approval).
- Backend: `read_text_file` (2 MiB cap, lossy UTF-8) and
  `list_project_docs` (SYSTEM.md + root `.md` ≤ depth 2, well-known names
  first) in `src-tauri/src/main.rs`; `api.readTextFile/listProjectDocs` +
  `InspectorArtifact/InspectorDoc/DocEntry/SwarmNode` types in `api.ts`.
- `App.svelte`: `.rb-wrap` next to `.stage-col` (margin-slide like the
  sidebar, width as CSS var, visibility flips after the slide), state
  persisted in `localStorage` (width/tab/auto-reveal; closed on cold boot),
  `Ctrl+\` / `Ctrl+Shift+D` toggle, Esc closes when focus is inside.
  Auto-reveal: `subsession_created` → Agents; `ui.show_artifact` result on
  the open thread → re-fetch events (live stream untouched) → Docs.
  Per-session last tool tracked from run events for the process table.
- `Titlebar.svelte`: Docs / Agents pills (loaded dot, live count) + panel
  toggle left of the window buttons. `Thread.svelte`: "Inspect swarm ↗" on
  the team banner; `ArtifactCard.svelte`: "open in deck ↗".
- Verified: `cargo check` (src-tauri) green, `npm run check` 0 errors,
  `npm run build` green; Playwright screenshots against the dev server with
  a mocked Tauri bridge for closed / docs-empty / PLAN.md / artifact v2 /
  agents / resized (420→584) / closed-again states.

## Appearance system rework (2026-09-13)
- Diagnosis: theme.toml carried 7 colours + glass + type, but only the accent
  reached the UI (components hardcoded slate greys, indigo tints, near-black
  panels); glass opacity/shadow were emitted into nothing; slider previews set
  inline vars that outlived pack switches (packs could not change a previewed
  value); active-pack detection matched accent only; fonts, sidebar/stage/bar/
  border/text colours, user.css and Save-pack had no UI.
- Token layer (`ui/src/theme.css`): inputs `--parzi-*` come from Rust; every
  component colour is now a ROLE token derived with color-mix: `--text/-2/-3/-4`,
  `--stage --sidebar --panel --menu --surface-1/2/3 --input --code`,
  `--line/-2/-3/-hi`, `--accent --accent-ink --accent-soft/-mid/-line/-glow/-text`,
  status `--ok/--warn/--bad/--info` (+`-soft`/`-line`), `--glass-blur/-radius/
  -shadow`, `--menu-shadow`, `--radius-1..4/-pill`. ~750 lines of dead v2 CSS
  dropped; kept blocks (thread, code, palette, pills) retokenised.
- Rust (`parzi-core/src/theme.rs`): f64 fields with clamp+round2 on save
  (no float noise in theme.toml), quoted font lists, `accent_ink` (WCAG
  luminance), percent twins for color-mix, `--parzi-glass-shadow` 0/1,
  `PackInfo` + `list_pack_infos` (real colours, built-ins first), `delete_pack`
  (user packs only), `read/write_user_css` (64 KB cap, empty removes file),
  `apply_pack` keeps the current wallpaper when the pack ships no art.
  Presets trimmed to 7 (cyberpunk-noir + emerald-matrix retired only while
  untouched). +4 unit tests. Tauri: `list_pack_infos`, `delete_pack`,
  `get_user_css`, `save_user_css`; `theme_css` lost its `unwrap()`.
- UI: `lib/theme.ts` (applyThemeCss clears inline previews; previewTheme mirrors
  the Rust vars), `settings/Slider.svelte`, `settings/ColorField.svelte`,
  `AppearanceSection.svelte` rebuilt: Theme cards drawn from real pack colours
  (thumbnail of sidebar/stage/composer/accent), save/delete packs, Wallpaper
  gallery + dim/blur/vignette + accent-follows-wallpaper, Accent (8 quiet
  presets + custom), Colours (all six), Type (font + size for UI and code),
  Glass (opacity now real, blur, radius, shadows), Custom CSS editor, Reset.
  Edits preview instantly, persist after 400 ms, then the authoritative CSS
  replaces the preview. `shared.css` tokenised; switch-on is accent, not green.
- De-leak pass over Sidebar, SettingsNav, ThreadRow, Omnibar (composer now uses
  `--panel`/`--glass-*` so the Glass sliders act), Titlebar, ProjectOverview,
  Thread, widgets, all settings sections, App shell/background/toasts/modal:
  0 hardcoded colours left outside `theme.css` status tokens, shadows, the
  accent preset list and one thumbnail fade.
- Verified: `cargo test --workspace` green (theme css tests added), clippy gate
  clean, `npm run check` 0 errors, `npm run build` green; live screenshots of
  the running app with Tokyo Night and Rose Pine (Appearance page, home,
  thread, inspector deck all follow the theme). Lane claim in
  `.lane_claim_appearance.md`; the concurrent inspector lane adopted the tokens.

## Model picker + provider routing rework (2026-09-13, night)
- Roster cut to five: `claude`, `codex`, `antigravity`, `opencode`, `xai`
  (`parzi_providers::PROVIDERS`, one list every surface iterates). Retired:
  openai, openrouter, ollama, t3, and the split anthropic/claude-code pair.
  Legacy ids still resolve (`canonical_id`: anthropic/claude-code → claude,
  openai → codex, grok/grok-cli → xai) so old threads and configs keep working;
  `ParziConfig::migrate` folds provider entries, `default_provider`, the auto
  order (`preferred_subscriptions` loads as `routing.auto_order`) and favorites.
- Billing is decided by the credential that actually won, not the provider
  id: `Provider::billing()` → subscription | api_key | none, plus
  `account_label()` ("Claude Max", "ChatGPT / Codex"). claude: keyring
  `claude-code` → `ANTHROPIC_AUTH_TOKEN` → `~/.claude/.credentials.json`
  (expiry read, plan label read) → keyring `anthropic`/`ANTHROPIC_API_KEY`.
  codex: keyring `codex` → `~/.codex/auth.json` `tokens.access_token` +
  `account_id` (ChatGPT backend, `store:false`, account header — not yet
  exercised live) → `OPENAI_API_KEY` (api.openai.com). xai: keyring/env/Grok
  CLI settings. opencode: serve token, classed as subscription.
- Router: `auto_chain` walks `routing.auto_order` and takes signed-in
  subscriptions only; `routing.keys_in_auto` (default off) lets keys queue up
  behind them. No local terminal fallback any more — an empty chain is a
  "sign in" error, never a silent key spend. `tier_fallback_chain` keeps the
  explicit pick at slot 0 (keys allowed) and follows the same rule after.
  Subscription slots are priced 0 in the orchestrator so the cost meter is honest.
- Anthropic engine: current models get `thinking: adaptive` +
  `output_config.effort` (the Low/Med/High pill), Haiku 4.5 and older keep
  `budget_tokens`; `thinking_delta` streams into the reasoning rail; OAuth
  beta header only on the bearer path. Catalog ids are the bare aliases
  (claude-opus-5 default, sonnet-5, haiku-4-5, fable-5-1; 4.8/4.6 legacy).
- Tauri: `get_models` rows carry `billing`, `hint`, `account`, `takes_key`,
  fixed roster order; `save_key` writes the provider's key slot
  (claude → `anthropic`, codex → `openai`), new `delete_key`.
- UI: Omnibar picker regrouped — Smart Auto row shows the live chain
  ("Codex → Claude → OpenCode · subscriptions only"), Favorites, then one
  section per provider with a status chip (plan label / API key / Sign in /
  Expired); signed-out providers collapse to their header + hint and open
  Settings on click; Ctrl+1..5 walk the visible list. Settings › Models
  rebuilt: Smart Auto order (▲▼), "Let API keys join Smart Auto", failover
  switch (moved here from General), five provider cards with how-to-sign-in
  copy, Google sign-in, add/remove key, model list with star + default.
  Default model at boot is `auto`.
- Verified: `cargo test --workspace` 68 passed (new: config migration,
  router key gating, catalog aliases, budget-vs-effort split); `npm run
  check` 0 errors; `npm run build` green; `parzi doctor` on this machine:
  claude/codex/opencode ok, chain `codex -> claude -> opencode`; `parzi
  health`: claude=subscription (Claude Max), codex=subscription (ChatGPT).
  Not verified live: an actual Codex-subscription completion against the
  ChatGPT backend, and a Claude OAuth completion with the new effort body.

## Signed installer + auto-update (2026-09-14)
- Updater wired end to end: tauri-plugin-updater (Rust) + @tauri-apps/plugin-updater 2.11.0 (JS) + updater:default capability + pubkey/endpoints in tauri.conf.json (feed: github.com/parzi/parzi releases latest.json); bundle now carries publisher/category/descriptions, createUpdaterArtifacts, currentUser NSIS + en-US WiX config
- Settings � System has an Updates card: version stamp, Check for updates, progress bar, Download & install, restart notice; dev builds degrade to a friendly message
- Keypair: minisign key at %USERPROFILE%\\.tauri\\parzi.key (private, never committed) + .pub; CI secret TAURI_SIGNING_PRIVATE_KEY signs artifacts in .github/workflows/release.yml (tag v* -> draft release with .exe/.msi/.sig/latest.json)
- Drive-bys: 4 pre-existing clippy denies in src-tauri fixed (too_many_arguments allow on send_message, Ok(x?) flatten in refresh_provider, format! -> to_string, split.last -> next_back)
- Verified: cargo check green, clippy gate clean, workspace tests pass, npm check 0 errors, npm build green


## Private GitHub repo (2026-09-14)
- https://github.com/lucas19919/Parzi (PRIVATE), branch main, 2 commits pushed; .gitignore added (target/, node_modules/, dist/, *.key); secret scan clean; TAURI_SIGNING_PRIVATE_KEY stored as repo secret; updater feed + Cargo repository retargeted to lucas19919/Parzi
- Note: a parallel session's dev-server fallback hunk in src-tauri/src/main.rs rode along in the 2nd commit (compiles, gate holds, 1 unused-parens warning is theirs)

## MCP connectors rework (2026-09-14)
- Fixed dead MCP wiring: tools are now advertised to the model (`defs_with_mcp`,
  12s best-effort per server, lane allowlist applied), `server.allow` is enforced
  (was a dead field), and saves hot-apply (Orchestrator cfg is shared + `apply_config`
  pushes fresh MCP configs; no restart needed).
- Two permission layers: general allowlist (`lanes.default_allowed_tools`, exact /
  `prefix.*` / `*`) + per-connector exposure (`allow`/`deny`) + per-tool override
  (`tool_modes`: auto/ask/deny, wins over lane mode in handler + executor).
- New commands `list_mcp_tools` / `list_all_mcp_tools` feed the tool browser.
- Connectors page rebuilt: General permissions (policy seg, fs/shell toggles,
  per-connector `server.*` switches, custom patterns) + installed cards with lazy
  tool lists (expose switch + Inherit/Auto/Ask/Deny per tool, `blocked above` badge)
  + 15 grouped one-click presets (fetch/memory/thinking/everything, filesystem,
  github, gitlab, postgres, sqlite, brave-search, puppeteer, playwright, slack,
  gdrive, gmail; official/community labelled, required keys/paths validated) +
  paste-anything + manual + how-permissions-combine explainer.
- Drive-by: 2 pre-existing clippy `manual_inspect` denies in providers/claude.rs
  (new toolchain lint) fixed with semantics-identical `.inspect()`.
- Verified: `cargo test --workspace` 72 pass, clippy deny-gate 0 errors,
  `npm run check` 0 errors, `npm run build` green.
- OPEN: `cargo check src-tauri` currently fails in `generate_context!` decoding
  `src-tauri/icons/icon.ico` (rewritten 13:36 by another lane; 6 valid BMP entries
  but the 256px entry trips the pinned decoder). Unrelated to this change — the bin
  compiled green with these edits before the icon rewrite; icon owner should
  regenerate decoder-friendly.


## Custom Windows installer (2026-09-14)
- Branded installers: Parzi dark-gradient banner/dialog (WiX) + header/sidebar (NSIS) art in src-tauri/installer/, MIT LICENSE, per-user NSIS config; WiX banner/dialog wired (this tauri-cli has no license-page keys, so no license screen � noted, installerHooks/template is the escape hatch)
- Toolchain: portable WiX 3.14 under %USERPROFILE%\\.tauri\\tools\\wix314 (NSIS blocked: winget needs admin UAC, SourceForge behind Cloudflare) � local builds do MSI only, CI builds NSIS+MSI
- Hooks fixed to ../ui (beforeBuildCommand ran with doubled ui/ui path and could never have worked)
- Local MSI verified built (Parzi_0.1.0_x64_en-US.msi, 9.8MB) but installs per-MACHINE (Tauri WiX default, Error 1925 without admin) � friend build = per-user NSIS from CI; tag v* to produce it
- Trap: local build hangs at updater signing prompt unless TAURI_SIGNING_PRIVATE_KEY (content) is set; _PATH alone is not enough for this CLI
- Note: parallel lane live-edited tauri.conf.json (devUrl removed, beforeDevCommand now runs build) + regenerated icons/icon.ico with malformed BMP headers (planes/bpp/comp shifted) that broke generate_context � icon.ico restored from HEAD, their 32x32.png + other work untouched and uncommitted


## v0.1.0 shipped to CI (2026-09-14)
- Tag v0.1.0 -> Release workflow green: draft release 'Parzi v0.1.0' holds latest.json + NSIS setup.exe/.sig + MSI/.sig (all updater-signed)
- Friend file: Parzi_0.1.0_x64-setup.exe (6.8MB, per-user NSIS) downloaded to ~/Downloads � send that; unsigned Authenticode so SmartScreen will ask once (More info -> Run anyway); repo stays private so in-app updates stay quiet until the release is published (drafts don't serve latest.json)


## Logo fix: circle-lines everywhere (2026-09-14)
- Root cause: icon.svg + favicons were already the circle-lines mark, but the master rasters (icon.png, 128x128, .ico) still carried the old serif-P � that P is what the taskbar/Start/installer showed
- Fix: cargo tauri icon from icon.svg regenerated all 50 icon files (valid PNG-compressed ICO, generate_context decodes it); apple-touch-icon was already correct
- Bumped to 0.1.1 (workspace + tauri + app + ui versions) so the rebuilt installer carries the new mark


## Custom installer voice (2026-09-14)
- NSIS wizard now speaks Parzi: installer/hooks.nsh sets every page title/copy (welcome, MIT license page via bundle.licenseFile, folder, start-menu, finish with 'Launch Parzi now', uninstall confirm, abort guard); installer/English.nsh re-voices stock strings (desktop shortcut, delete-data, app-running)
- Uninstaller branded too (icon + header art); homepage set for Add/Remove Programs links; mechanism verified against tauri-bundler 2.9.4 template (hooks include precedes all MUI_PAGE_*, language files included last so overrides win)
- No local NSIS (toolchain blocked) � verification is the CI-built installer + silent install test on v0.1.2


## Custom installer voice verified (2026-09-14)
- v0.1.2 CI failed on config schema (customLanguageFiles is a lang->path MAP not an array; homepage lives under bundle not root) � fixed, schema now pre-validated locally with ajv + Tauri schema before tagging
- v0.1.3 green: draft release holds setup.exe/.sig + MSI/.sig + latest.json; silent per-user install test passed (LOCALAPPDATA\\Parzi, app launches with Parzi window, uninstall removes everything incl. desktop shortcut)
- Friend file: Downloads\\Parzi_0.1.3_x64-setup.exe (6.8MB). Wizard pages: Welcome to Parzi -> MIT license -> folder -> Start Menu -> install -> 'Parzi is installed' with Launch + desktop-shortcut options; uninstaller branded with delete-data option


## Blank-app root cause + fix (2026-09-14)
- Installed app showed empty transparent window; CDP remote-debug proved the WebView sat on about:blank and never loaded the frontend
- ROOT CAUSE: src-tauri/Cargo.toml lacked the custom-protocol Tauri feature, so cfg(dev) was true in ALL builds and every release binary loaded devUrl (localhost:1420) instead of the bundled dist. The parallel lane's navigate-fallback hunk then raced the load into about:blank
- Fix: +custom-protocol feature, removed the probe-navigate hunk, restored devUrl + dev-server beforeDevCommand. Verified locally: release binary loads tauri.localhost and renders the full UI (CDP screenshot)
- Lane WIP note: their uncommitted main.rs still contains the hunk � they must drop it on rebase, else local builds keep the race


## v0.1.4 verified working (2026-09-14)
- Installed v0.1.4 from CI setup.exe loads tauri.localhost and renders the full UI (sidebar, Inbox, Asuka stage, glass composer, inspector deck) � CDP screenshot proof; uninstalled after, machine clean
- Installer wizard pages carry the Parzi voice + art (verified in config/template review; interactive click-through still untested � MUI pages are stock layout with custom art/copy)


## Dark installer (2026-09-14)
- Chose dark 6-page flow (no template fork): new vector-style art (ring+lanes, no raster text, no accent-line artifact, #0B0B10-blended), hooks.nsh now sets MUI_BGCOLOR + per-page dark SHOW painters + all Parzi copy
- Deviation from antigravity plan: dark theme inlined in hooks.nsh (relative !include would resolve against the bundle out-dir); Component 4 (asuka _up_ lookup) SKIPPED � installed-app screenshot proves the wallpaper already loads


## Dark installer verified live (2026-09-14)
- v0.1.5 wizard captured via PrintWindow: dark #0B0B10 dialog, vector mark sidebar blending seamlessly, Welcome to Parzi + brand copy, new mark in title bar � the Win32 clash is gone
- Silent install/uninstall cycle green; machine left clean. Note: fullscreen GDI screenshots go black when the display sleeps � PrintWindow by HWND is the reliable capture method


## Installer dark fix, round 2 (2026-09-14)
- Live screenshot showed the dark bg + art applied but body text dim, button bar light, title bar white: the per-page MUI_*PAGE_SHOWFUNCTION defines don't exist � replaced with the single documented MUI_PAGE_CUSTOMFUNCTION_SHOW hook; added DWM dark title bar call in the painter
- Start Menu audit: only orphan was a stale root Parzi.lnk (early installs), already gone � one clean Parzi folder left; no source work lost, everything is tagged

