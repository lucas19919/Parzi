# Parzi Design System — MASTER

> Single source of truth for every agent touching `ui/`.

## Product

Parzi is the agent harness and the browser. Local-first, keyboard-first.
Silent failures are bugs, so every action toasts success or error.

## Tokens (`ui/src/theme.css` `:root`)

A theme sets four colors: `--bg`, `--text`, `--accent`, and `--panel` (empty
panel means text at 6% over the background). Muted, line, faint, hover,
selected, and link are mixed from those in `theme.css`. `--ok` `--warn`
`--bad` are fixed status colors. No hex in a `<style>` block.
`user.css` loads last and may override any token. Charts read tokens through
`resolveColor()` in `lib/theme.ts`.

Spacing is `--space` (4px). Legal steps are 1, 2, 3, 4, 6, and 8 times that.
Icons come only from `Icon.svelte`, Lucide outlines at a constant 1px stroke, the
weight of the window controls, at 16 (12 for the small glyphs in a row).
Logos and letter badges are 28 in a list row, so the glyph inside is 16.
Motion is `--dur` (140ms) and `--dur-fast` (60ms) with `--ease-out`. Disabled
is `--disabled` (0.5) everywhere; dimmed rows are `0.55`.
Control heights are `--h-sm` (24), `--h` (28), `--h-row` (36), `--h-row-lg` (46).
Popovers sit at `--z-pop`, tooltips at `--z-tip`.

Radius has one rule and no nesting maths; spacing separates a box from what
is inside it, so an outer corner is never larger to stay concentric:

| What | Radius |
|---|---|
| Anything 24px or taller: cards, popovers, tooltips, buttons, inputs, segmented controls, chips, logos and letter badges | `--radius` (12px) |
| Anything shorter: inline code, kbd, progress tracks, small badges, except count chips on tabs, which stay `--radius` | `--radius-sm` (4px) |
| Dots, knobs, spinners | `50%` |
| Only the switch and the slider track | `999px` |

Text has four roles, global classes in `theme.css`. Use the class and set no
font size, weight or colour next to it:

| Role | Class | Size / weight / colour |
|---|---|---|
| Page title, section title | `.t-page`, `.t-section` | 14 / 600 / text |
| Label | `.t-label` | 13 / 500 / text |
| Hint | `.t-hint` | 12 / 400 / muted |
| Group label in a list | `.t-group` | 11 / 600 / muted |

Position gives the hierarchy, so a page title and a section title match. In
Settings the tabs are the page title and `.row-title` is the section title.

`npm run lint:tokens` runs in CI. It rejects raw px spacing, radii and font
sizes off the rules above, other font weights, hex outside `theme.css`,
`var()` of anything undefined, and tokens nothing uses.

Appearance UI (`settings/AppearanceSection.svelte`) edits the four input
colours, type and the one home backdrop picture (plus which slice of it
shows, `--bg-y`). Edits preview through inline vars (`lib/theme.ts`
`previewTheme`), persist after 400 ms, then the authoritative stylesheet
from Rust replaces the preview (`applyThemeCss` always clears inline
vars). Theme packs live in `~/.parzi/themes/<slug>/`; a pack without art
keeps the current backdrop. There is no glass, no blur, no wallpaper
grade: surfaces are solid `--panel` on `--bg`.

## Layout

- Top bar (44px): settings (the hamburger), sessions, tabs, window controls. The
  bar is the drag region.
- Tabs are sessions and pages. Settings and Sessions open inside the session,
  with a back arrow at the top right, and the session column stays. Both render
  through `Page.svelte`, which owns every position: the bar, a title or tab's text
  edge (28px), the 840px column and an optional rail on the same text edge. Pages
  fill its slots with `.rows` cards from `theme.css` and set no positions themselves.
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
  into the web search in place, at the same size (Tab does the same on a new tab). Above
  the box, "Let's build in {project} on {branch}" is plain text and opens the
  same pickers. `/` opens commands, `@` lists files. Esc stops a run.
- Buttons: `.btn` is the one button, an outline (transparent, text color,
  line border, hover wash). `.primary` is the accent fill with the background
  as its label, `.danger` the bad outline. `.icon-btn` is the one square icon
  button and `.tin` the one text input. Every button presses (scale 0.97).
- Model picker: one list sized to its content. Search, Starred
  (set in Settings › Providers), then each ready agent's models under its
  name, and agents that are not set up last. Opens from its button.
- Popovers go through `lib/popover.ts` (portal, placement, outside click, Esc).
- Thread: user bubbles right (`--radius`), tool stacks (running / ok / failed with
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
