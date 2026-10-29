# Parzi Design System — MASTER

> Single source of truth for every agent touching `ui/`.

## Product

Parzi is the agent harness and the browser. Local-first, keyboard-first.
Silent failures are bugs, so every action toasts success or error.

## Tokens (`ui/src/theme.css` `:root`)

A theme sets four colors: `--bg`, `--text`, `--accent`, and `--panel` (empty
panel means text at 6% over the background). Muted, line, faint, hover,
selected, and link are mixed from those in `theme.css`. `--ok` `--warn`
`--bad` are fixed status colors. `npm run lint:tokens` rejects a raw px
padding, margin, gap, or an off-scale radius. No hex in a `<style>` block.
`user.css` loads last and may override any token. Charts read tokens through
`resolveColor()` in `lib/theme.ts`.

Spacing is `--space` (4px). Legal steps are 1, 2, 3, 4, 6, and 8 times that.
`--radius` is 12px, the chat window's corner, and `--radius-lg` is the same.
A box inside a rounded box sets its radius to `calc(var(--r) - var(--pad))`
and does not redefine `--r` (that cycles). `--radius-sm` is one `--space`
step, for chips and thin bars. Pills are `999px`, dots are `50%`, marks may use `25%`.
Motion is `--ease-out` and `140ms`. Disabled is `0.5` everywhere; dimmed
rows are `0.55`; icons are 12 or 16 only.

Appearance UI (`settings/AppearanceSection.svelte`) edits the four input
colours, type and the one home backdrop picture (plus which slice of it
shows, `--bg-y`). Edits preview through inline vars (`lib/theme.ts`
`previewTheme`), persist after 400 ms, then the authoritative stylesheet
from Rust replaces the preview (`applyThemeCss` always clears inline
vars). Theme packs live in `~/.parzi/themes/<slug>/`; a pack without art
keeps the current backdrop. There is no glass, no blur, no wallpaper
grade: surfaces are solid `--panel` on `--bg`.

## Layout

- Top bar (38px): menu, home, tabs, window controls. The bar is the drag region.
- Tabs are sessions and pages. Settings and history open inside the
  session, with a back arrow at the top right, and the session column stays.
  Drag reorders tabs (pointer events,
  not HTML5 drag, which WebView2 hands to the file-drop handler);
  Ctrl+Shift+←/→ moves the focused tab. A session tab shows the thread card
  and the docked composer; an empty draft shows the composer centred.
- Each page tab owns a native WebView2 child that stays alive while hidden,
  so switching tabs never reloads. It fills everything under the address bar
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
- History opens in the session (Ctrl+H): Sessions, Pages, Bookmarks, Downloads, one search field, rows
  grouped by day with a hover ×. A new tab focuses the composer.
- An empty session tab is Home: the composer. New tab and the Home button land here.
- Switcher (Ctrl+P / Ctrl+K): actions, open tabs, sessions. Settings opens in
  the session (Ctrl+,).

## Components (states: default / hover / active / focus-visible / disabled)

- Projects live in the composer's project menu, nowhere else. The menu lists
  each project (click to work there), Make-this-folder-a-project, New
  project, and No project. Deleting is a trash button on the project row
  itself: first click arms ("Sure?"), second removes the project record
  (its notes stay in the vault). There is no notes UI: notes are vault
  files for agents (`projects/<slug>/<name>.md` plus plan notes). A note
  link anywhere pins the note into the session's side column; outside a
  session it toasts instead of opening anything.
- Composer: one evenly padded box. The textarea sits above a quiet row:
  model, effort, and permission as plain text on the left; a browser icon,
  attach, and send on the right. Send is the arrow, with no circle, and it
  takes the text color once there is text. The browser icon turns the box
  into one search row under the tab (Tab does the same on a new tab). Above
  the box, "Let's build in {project} on {branch}" is plain text and opens the
  same pickers. `/` opens commands, `@` lists files. Esc stops a run.
- Buttons: `.btn` and settings `.sbtn` are the same outline (transparent,
  text color, line border, hover wash). `.primary` is the accent fill with
  the background as its label. Every button presses (scale 0.97). Disabled is 45%.
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

- `backdrop-filter` anywhere: surfaces are solid, there is nothing to blur through.
- Dead controls: every visible button and command has a handler.
- Hardcoded hex where a token exists; spacing or radii off the `--space` / `--radius` scale.
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
