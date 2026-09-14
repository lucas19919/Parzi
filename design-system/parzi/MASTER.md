# Parzi Design System — MASTER

> Single source of truth for every agent touching `ui/`.
> Pattern: ui-ux-pro-max Master + Overrides (MIT). Page overrides live beside
> components as `*.override.md` — none exist yet; add one only for deviations.

## Product

Lean Rust agent harness. Dark, retro-glass, vibey. Wallpaper art + grain,
glass composer, frosted menus. Local-first, keyboard-first, silent failures
are bugs — every action toasts success or error.

## Tokens (`ui/src/theme.css` `:root`)

Two layers. **Inputs** `--parzi-*` are emitted from `~/.parzi/theme.toml` by
`Theme::to_css_vars()` (seven colours, type, wallpaper grade, glass); the
defaults in `theme.css` only cover a missing file. **Role tokens** are derived
from the inputs with `color-mix` and are the only thing a component may use.
No hex, no `rgba()`, no `var(--parzi-x, #fallback)` inside a `<style>` block.
`user.css` loads last and may override any token.

| Role | Tokens | Use |
|---|---|---|
| text | `--text` `--text-2` `--text-3` `--text-4` | primary · secondary · muted · faint (never below 11px) |
| surfaces | `--stage` `--sidebar` `--panel` `--menu` | window ground · sidebar wash · glass (composer, palette; opacity from theme) · popovers |
| raised | `--surface-1` `--surface-2` `--surface-3` `--input` `--code` | card · hover · selected · text fields · code blocks |
| lines | `--line` `--line-2` `--line-3` `--line-hi` | theme border · hairline · strong · top-edge light |
| accent | `--accent` `--accent-ink` `--accent-soft` `--accent-mid` `--accent-line` `--accent-glow` `--accent-text` | one filled action per surface; ink is the text that reads on it |
| status | `--ok` `--warn` `--bad` `--info` (+ `-soft`, `-line`) | colour means state, never decoration |
| glass | `--glass-blur` `--glass-radius` `--glass-shadow` `--menu-shadow` | shadows collapse to 0 when the theme turns them off |
| radius | `--radius-1..4` (6/8/10/12) · `--radius-pill` | never mix other radii |
| type | `--parzi-font` `--parzi-font-size` `--parzi-mono` `--parzi-mono-size` | Inter + JetBrains Mono ship; Instrument Serif italic for the wordmark |
| spacing | 4 / 8 / 12 / 16 / 24 scale | no magic numbers |
| motion | `--ease-spring`, `--ease-snap`, `--dur-pop` 180ms, `--dur-lift` 140ms | springs, not tweens |

Appearance UI (`settings/AppearanceSection.svelte`): edits preview through
inline vars (`lib/theme.ts` `previewTheme`), persist after 400 ms, then the
authoritative stylesheet from Rust replaces the preview (`applyThemeCss`
always clears inline vars). Theme packs live in `~/.parzi/themes/<slug>/`;
a pack without art keeps the current wallpaper.

## Glass discipline (performance is a feature)

- Real `backdrop-filter` lives ONLY on: composer bar, model/project menus,
  toasts, approval card. Sidebar is translucent solid, never blurred.
- Grain overlay: plain opacity, NEVER `mix-blend-mode` (killed dragging once).
- Static layers (wallpaper, grain, halftone) cost zero per frame. Keep them static.
- Release profile stays lean; UI bundle warnings get fixed, not ignored.

## Components (states: default / hover / active / focus-visible / disabled)

- Sidebar rows (grid: dot · main · actions), active = accent gradient + 3px
  neon inset strip. Hover rail: pin ★ · rename · fork · kill.
- Pills 28px: default / hover top-edge brightening / active scale(.96).
- Model menu 420px: search-first, ⚡ Smart Auto row, provider sections with
  status dots, tier mono, no-key routes to key settings, full arrow+enter nav.
- Composer: micro-header (workspace · branch · tokens) + textarea + pill row
  (+ · project · model · effort · send jewel). Stop = red square, Esc works.
- Thread: user glass bubbles right, tool micro-cards (running pulse / ✓ms /
  ✗ms + accordion), reasoning rail (accent left bar, collapsed when done),
  code blocks (ext badge + lines + copy + gutter + diff tint + 30-line clamp).
- Palette (Ctrl+K): actions + threads + models, toast-only notices.
- Toasts: every save/apply/failover/error. No silent failures, ever.

## Accessibility (non-negotiable)

- SVG icons only — no emoji-as-icon (⚡-style glyphs render as emoji on mobile).
- Semantic elements: real `<button>` for leaf actions; `div role=button` only
  when nesting forbids buttons (thread rows contain inputs/actions).
- `transition:fade|local` inside `{#each}` blocks — never global list transitions.
- `cursor: pointer` on all clickables; visible `:focus-visible` rings.
- `prefers-reduced-motion: reduce` kills animation (state stays correct).
- Live counts need context ("3 live", not "3"); badges never color-only
  (tier text, status text accompany dots).
- Text reflows: ellipsis + title/full-value path; chips wrap.

## Anti-patterns (instant reject in review)

- New `backdrop-filter` surfaces without a perf note.
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
