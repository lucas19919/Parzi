# Implement List — committed, from the T3 comparison

Status 2026-10-06: image generation + doc reading + page screenshots
shipped (below). Remaining: keyed image endpoint in Settings, DDG
search API (browser navigation covers it crudely today), message edit.

## 1. Image generation — SHIPPED (Pollinations default)

- `image.generate {prompt, size?}` on Build (approval-gated) and
  Work (auto). Endpoint template in config (`image.endpoint`,
  `{prompt} {width} {height} {model}`), default Pollinations flux.
- Saves under `generated/`, publishes a kind-`image` artifact rendered
  inline. Still open: keyed provider endpoint + Settings UI for it.

## 2. Web search for all agents (browser-backed) — PARTIAL

- `doc.read {source}` extracts PDFs (arxiv!) and text from URLs/paths
  on all lanes; `browser.shot` saves a JPEG the agent opens with its
  own vision to verify pages and diagrams. Connected tabs stay the
  verification loop.
- Still open: a data-returning `web.search` API (today: navigate DDG
  in-browser and read the results page — works, inelegant).

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

## Next: wake-ups (scheduled runs)

- Queued runs already survive restarts (sidecar per session,
  `recover_queue` at boot, covered by test) and plans/transcripts live
  on disk. What does not exist: anything that wakes up on its own.
- Shape when built: persisted schedule list (cron-ish), a timer in the
  pump loop, `schedule.create/list/delete` agent tools, per-fire
  approval posture inherited like children. No UI until agents use it.
