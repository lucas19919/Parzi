# Parzi Design System — MASTER

> Single source of truth for every agent touching `ui/`.

## Product

Lean Rust agent harness and browser. Dark, matte, quiet. Local-first,
keyboard-first; silent failures are bugs, so every action toasts success or
error.

## Tokens (`ui/src/theme.css` `:root`)

Ten colours, nothing else. `theme.toml` sets four of them (background, text,
muted, accent) through `Theme::to_css_vars()`; `--panel`, `--line` and
`--faint` are mixed from those. Components use only these names. No hex or
`rgba()` colours in a `<style>` block (shadows, diff tints and the Windows
close-button red excepted). `user.css` loads last and may override any token.
Charts read tokens through `resolveColor()` in `lib/theme.ts`.

| Token | Role |
|---|---|
| `--bg` | window background, inputs, code |
| `--panel` | raised surfaces: cards, composer, menus, popovers |
| `--line` | borders, dividers, hover and selected fills |
| `--text` | primary text |
| `--muted` | secondary text and icons |
| `--faint` | placeholders, hints, disabled |
| `--accent` | the one colour with a job: focus, selection, primary action |
| `--ok` `--warn` `--bad` | state only, never decoration |

Non-colour tokens: `--font` `--font-size` `--mono` `--mono-size` (type),
`--bg-dim` `--bg-blur` `--vignette` (wallpaper), `--radius` (8px)
`--radius-lg` (12px), `--shadow`. Pills use `999px`; motion is `140ms ease`.

Appearance UI (`settings/AppearanceSection.svelte`) edits the four input
colours, type and the wallpaper grade. Edits preview through
inline vars (`lib/theme.ts` `previewTheme`), persist after 400 ms, then the
authoritative stylesheet from Rust replaces the preview (`applyThemeCss`
always clears inline vars). Theme packs live in `~/.parzi/themes/<slug>/`;
a pack without art keeps the current wallpaper.

## Layout

- Top bar (38px): menu, home, tabs, window controls. The bar is the drag region.
- Tabs are sessions, pages, the brain or history. Drag reorders them (pointer events,
  not HTML5 drag, which WebView2 hands to the file-drop handler);
  Ctrl+Shift+←/→ moves the focused tab. A session tab shows the thread card
  and the docked composer; an empty draft shows the composer centred.
- Each page tab owns a native WebView2 child that stays alive while hidden,
  so switching tabs never reloads. It fills everything under the 40px toolbar
  (back, forward, reload, address, shield, pin); F11 or a page's own
  fullscreen hides all chrome. Any HTML overlay above it (top menu, switcher,
  popovers) registers in `lib/overlay.ts` so the page hides while the overlay
  is open.
- No white flashes: a page webview starts in the theme background, and the
  slot under it takes the page's own background colour (read after each
  load), so switching tabs never shows a mismatched frame. Restored page
  tabs are created hidden in the background shortly after startup.
- Shield: accent with the blocked count while blocking; faint when the site
  is allowed or the blocker is off. One click toggles the site and reloads.
- Key: fills a Bitwarden login on https pages; one match fills at once, more
  open a short list. Unlocking happens in Bitwarden's own prompt.
- Address suggestions (`SuggestList.svelte`): search row, DuckDuckGo
  phrases, then history/bookmark matches; inline completion selects the
  completed part. Over a page, the native view hides while the list is open
  and a JPEG snapshot of the page (taken on focus) stands in for it.
- History is a tab (Ctrl+H): History | Bookmarks, one search field, rows
  grouped by day with a hover ×. A new tab focuses the composer.
- An empty session tab is Home: the composer with pinned (or most visited)
  sites and recent sessions under it. New tab and the Home button land here.
- Switcher (Ctrl+P / Ctrl+K): actions, open tabs, sessions. Settings is a full
  pane (Ctrl+,).

## Components (states: default / hover / active / focus-visible / disabled)

- Brain is a folder tree, kept simple. Folders: All sessions, each project,
  Not used, with no counts. Clicking a folder opens or closes it; a project
  also opens its project note. Notes sit inside; a pin marks "always in
  full". Drag a note onto a folder to move it, Ctrl+drag to copy (pointer
  events, not HTML5 drag). A note shows one header row (title, pin, Read/Edit,
  Open in Obsidian, Open file, Delete) and the note. No chips, no footer.
  Reloads on window focus.
- Composer: one box (attachments, textarea, send), then a quiet row: attach,
  Agent (robot) / Web (globe) mode (Tab), permissions, project (+ git
  branch) on the left; model, effort, context ring on the right. The project chip opens a
  picker (projects, make this folder a project, new project from a folder, no
  project); it never opens a bare folder dialog. Send fills with the accent
  once there is text. `/` opens commands, `@` lists files. Esc stops a run.
- Buttons: `.btn` is a raised neutral (hover brightens fill and border);
  `.btn.primary` is the accent with an inner highlight and no glow,
  lifting 1px on hover. Every button presses (scale 0.97). Disabled is 45%.
- Model picker: one list sized to its content. Search, Starred
  (set in Settings › Providers), then each ready agent's models under its
  name, and agents that are not set up last. Opens from its button.
- Popovers go through `lib/popover.ts` (portal, placement, outside click, Esc).
- Thread: user bubbles right (14px radius), tool stacks (running / ok / failed with
  output), reasoning rail, code blocks (ext badge, lines, copy, gutter, diff
  tint, 30-line clamp), per-message copy under the message.
- Icons come from `lib/icons.ts` through `Icon.svelte`; no inline SVG paths
  in components except the window controls.
- Toasts (`lib/toast.ts`): every save, apply and failure. No silent failures.

## Accessibility (non-negotiable)

- SVG icons only — no emoji-as-icon (⚡-style glyphs render as emoji on mobile).
- Semantic elements: real `<button>` for leaf actions; `role=tab`/`role=button`
  on a div only when nesting forbids buttons (a tab contains its close button).
- `transition:fade|local` inside `{#each}` blocks — never global list transitions.
- `cursor: pointer` on all clickables; visible `:focus-visible` rings.
- `prefers-reduced-motion: reduce` kills animation (state stays correct).
- Live counts need context ("3 live", not "3"); badges never color-only
  (tier text, status text accompany dots).
- Text reflows: ellipsis + title/full-value path; chips wrap.

## Anti-patterns (instant reject in review)

- New `backdrop-filter` surfaces without a perf note.
- Dead controls: every visible button and command has a handler.
- Hardcoded hex where a token exists; new radii/spacing off-scale.
- Silent catch blocks in UI code; missing toast on failure paths.
- `unwrap()` in Rust outside tests; secrets in logs; unbounded lists
  without caps (200 files, 50 rows, 8 attachments, 12k chars).
- Copy-pasted provider code instead of extending the `Provider` trait.

## Pre-delivery checklist (run before calling UI done)

- [ ] Keyboard: tab order sane, arrows+enter in every menu/popup, Esc closes
- [ ] Every button has hover + active + focus-visible + disabled states
- [ ] Toasts on all save/apply/fail paths; startup failure toasts, never blank
- [ ] Narrow window (960px min): no overflow, ellipsis holds, menus flip
- [ ] Reduced-motion pass: `prefers-reduced-motion` respected
- [ ] No new fullscreen blur; `npm run build` clean; bundle size not regressed
