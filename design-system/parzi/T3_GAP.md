# T3 Gap Inventory — Parzi vs t3.chat

Full list from the 2026-10-06 comparison. t3.chat is closed-source,
web-only, pure chat: 50+ models on one $8 sub, no tools, no agents, no
MCP, no mobile app. We compete on depth; they win on chat polish.
Nothing here is a commitment — the commitment lives in IMPLEMENT.md.

## Message UX (their home turf)

- **Branching** — visual message tree, fork from any message. We have
  thread fork only. Dropped for now: fork covers the need; tree UI is a
  second product.
- **Edit & retry** — edit a sent message, get a fresh answer. See
  IMPLEMENT.md (edit = maybe).
- **Regenerate** — one-click fresh answer. Dropped: stop + resend covers
  it; a dedicated button adds UI for little.
- **Enhance prompt** — rewrite my prompt better. Dropped: gimmick;
  modes route intent better than a polish button.
- **Temporary chat** — incognito threads, nothing persists. Dropped:
  conflicts with the brain-learning story; sessions are cheap.
- **Share links** — public thread URLs. Blocked on ParziOS hosting;
  revisit there, not before.
- **Message copy** — have it (per-message copy under every message).

## Content

- **Image generation in-chat** — T3 makes images via DALL-E etc.
  inside chat. We render artifacts beautifully but cannot *make*
  images. → IMPLEMENT.md.
- **Web search for any model** — T3 pairs any model with search +
  citations. Our Research lane does this via agents; plain Code turns
  still have no data-returning search (browser.open navigates, it does
  not return results). → IMPLEMENT.md.
- **File / image uploads** — have it (up to 8 attachments, images
  render, vision models consume them).
- **Markdown + syntax highlighting** — have it (md.ts + hljs, 30-line
  code clamp with expand).

## Organization

- **Full-text search across threads** — Switcher/History search titles
  and folders only, not message bodies. Wanted; unscheduled.
- **Folders for threads** — T3 folders vs our projects (folder-linked
  notes + cwd). Ours is deeper; unaffiliated threads stay flat.
  No action.
- **Personas** — saved per-thread personalities. We have global
  instructions only. Wanted; unscheduled (quick win when wanted).
- **Streaming speed / lightweight UI** — T3's whole brand. We counter
  with the thinking orb + LiveMarkdown incremental render; keep the
  bundle diet going (the 1.1 MB index chunk is already flagged).

## Where we win (do not sand off)

Agents with real tools, MCP connectors, approval gates, subagents +
Agents tab, brain vault, connected browser tabs, per-mode lanes,
thinking orb, theming/wallpapers, desktop integration, onboarding that
installs agents. T3 has none of this by design.
