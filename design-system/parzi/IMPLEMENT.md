# Implement List — committed, from the T3 comparison

Three items. Everything else in T3_GAP.md is explicitly not scheduled.

## 1. Image generation

- New Parzi-native tool, e.g. `image.generate { prompt, size? }`,
  offered on Code lane (Research: allow — trivial "gen an image"
  questions are a headline Research use case).
- Needs a backend: provider-native where it exists, else a configured
  endpoint + key in Settings › Providers. Decide at build time; do not
  hardcode one vendor.
- Output must land somewhere visible: artifacts have no image kind
  today (`code|markdown|html|svg|json|csv|diff|text`) and EChart was
  removed — add an image-capable artifact/widget path in the same
  change, or generated images have nowhere to live.
- Approval: image calls go through the normal permission mode
  (Supervised asks, Full runs).

## 2. Web search for all agents (browser-backed)

- Today `browser.open` *navigates*; no tool *returns* search results as
  data. Add a data-returning search tool (e.g. `web.search { query }`
  → titles + URLs + snippets) on Code **and** Research lanes.
- Start with the DuckDuckGo html/lite endpoint (no key, no dependency);
  graduate to a keyed API (Brave/Tavily) in Settings if quality demands
  it. Keep the tool interface identical so the backend is swappable.
- Research answers cite sources by default (brief already says so;
  enforce by having URLs in-context, not by hoping).
- The connected tabs (open → read → click/type) are the verification
  loop: search returns candidates, the agent opens and quotes the real
  pages. That loop is also the test plan: seed a query, assert the
  answer cites a page the tabs actually loaded.

## 3. Message edit (maybe)

- Edit a user message = truncate everything after it + resend as a new
  turn (same mechanism as fork, minus the copy). Needs a
  delete-events-after-index store op; check `store` maintenance ops
  before promising.
- Explicitly out: full branching tree UI, regenerate button, enhance
  prompt, temp chat. Fork + stop + resend cover those needs today.

## Dropped with reasons

- Enhance prompt, temp chat, edit-retry/regenerate, branching — see
  T3_GAP.md. Revisit only with user demand, except share links which
  wait for ParziOS hosting.
