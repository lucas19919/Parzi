# Third-party notices

## opencode-antigravity-auth (MIT)

The Antigravity request/response shapes in `crates/parzi-providers/src/antigravity.rs`
(OAuth token use, endpoint fallback order, `{project, model, request}` envelope,
Claude↔Gemini transform strategy, thinking-strip approach, schema allowlist idea,
synthetic `tool_result` recovery) are derived from `opencode-antigravity-auth`,
MIT-licensed by its authors. Translated to Rust; no code copied verbatim.

Upstream: https://github.com/GrigorTonikyan/antigravity-auth

## t3code (MIT)

Auth-flow patterns in `crates/parzi-providers/src/antigravity_oauth.rs`
(explicit sign-in state machine with expiry, cancel semantics, sign-out that
clears cached credentials, user-facing failure messages) are informed by
t3code's `AntigravityAuth` controller design. Re-implemented in Rust;
no code copied verbatim.

Upstream: https://github.com/pingdotgg/t3code (MIT)

Provider brand marks in `ui/src/assets/providers/` (OpenAI knot, Claude
starburst, Grok mark, OpenCode mark, Antigravity app icon) match the icons
T3 uses in `apps/web/src/components/Icons.tsx` (`PROVIDER_ICON_BY_PROVIDER`).
Adapted to static SVG/PNG assets (theme-adaptive marks use `currentColor`);
MIT-licensed upstream.

Upstream: https://github.com/pingdotgg/t3code (MIT)

## t3router (MIT)

The `t3` provider in `crates/parzi-providers/src/t3.rs` (cookie auth,
`POST /api/chat`, generic SSE delta extraction, credit-aware design) follows the
protocol documented by MIT-licensed `t3router`. Translated to Parzi's Provider
trait; no code copied verbatim.

Upstream: https://github.com/vibheksoni/t3router

Note: t3.chat itself is closed source. Its Google login is account identity, not
model auth — models resolve server-side under a paid subscription. Parzi stays
local-first: the `t3` adapter routes a user's own paid subscription, opt-in,
with the same TOS caution as Antigravity.

## Bundled artwork (not licensed for redistribution)

The default wallpaper "assets/backgrounds/eva-crosses.jpg" is Neon Genesis
Evangelion fan art (artwork (c) khara / Gainax, original artist unknown). It
ships as a personal-use default while the repo stays private. Do not include
it in any public release or public installer: replace with owned/CC0 art
first (see AUDIT.md B6). The previously bundled "asuka.png" was removed;
existing installs clean it up automatically.
