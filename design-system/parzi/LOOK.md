# LOOK — appearance system plan (2026-10-06)

Problem: glass and color values are copy-pasted per component
(51 color-mix/backdrop-filter sites), magic hexes recur (`#06110a`,
`#e8b64c`), the hero fallback image is hardcoded in App, and fills
each invent their own chrome. One background, everything else glass —
from tokens, not patches.

## Layers (top to bottom)

1. **Backdrop** — exactly one: DefaultArt wallpaper, else plain theme
   stage. No fallback art, no bands. Nothing else paints a full-bleed
   background, ever.
2. **Glass** — every floating surface (session panel, fills, composer,
   side panel, popovers, toasts) from `--glass-*` tokens. Content
   (bubbles, code, text) stays solid for readability.
3. **Content** — text, code, math. Links `--link`, never accent.

## Tokens (theme.css `:root`)

- `--glass-bg: color-mix(in srgb, var(--panel) 60%, transparent)`
- `--glass-blur: blur(24px) saturate(1.2)`
- `--glass-border: 1px solid color-mix(in srgb, var(--line) 70%, transparent)`
- `--glass-shadow: 0 10px 30px rgba(0, 0, 0, 0.3)`
- `--glass-strong-bg` (composer, 58%) — only exception, documented here
- `--on-ok: #06110a` (dark text on green pills)
- `--work: #e8b64c` (replaces hardcoded ambers)
- `--link: #8ab4ff`, `--link-hover: #aecbff`
- `--scrim: rgba(0, 0, 0, 0.6)` (modal scrims)

## Rules

- No new `backdrop-filter`, `color-mix(panel...)`, or hex outside
  theme.css. New surfaces compose the tokens.
- Server-driven values (`--bg`, `--accent`, `--hero-*`, `--bg-dim`)
  stay server-driven; everything else is static tokens.
- Fills (history/brain/settings) share `PanelHeader`, the session-head
  look: 42px, title left, close right. Browser is exempt (native page).

## Status

- [x] Phase 1: ungated-thread note deleted, tokens added, fills share
  PanelHeader, hero via `--hero-art` (this commit)
- [ ] Phase 2: audit remaining `color-mix` one-offs (menus, pills,
  codeblocks) into tokens; fold composer exception into tokens
- [ ] Phase 3: hero height/fade per-wallpaper memory (optional)
