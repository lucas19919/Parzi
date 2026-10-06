# Harness Backlog — agent probe 2026-10-06

An agent drove Parzi (via MCP, through the opencode harness) and wrote
its pain into the brain (`brain/projects/parzi-agent-harness-requirements.md`,
`brain/projects/strohmann/harness-issues.md`). Eleven items, with status
as of 2026-10-06. Parzi-side vs harness-side matters: we only fix ours.

## Fixed in Parzi

1. **Browser bound to the session** — agent `browser.open` now passes
   the calling session (`toolhost`), `open_connected` reuses the
   session tab, read/click/type act on it. Shipped before the probe;
   the open-path session was the last hole, closed same day.
2. **Delegation works (Parzi side)** — `session.spawn` wait/block and
   background id + `read_session` poll are covered by live tests. The
   probe's `idle` + `ServeError` came from the opencode Task layer, not
    us. Our defs are advertised per lane now (Build yes, Research never).
3. **Memory listing placement bug** — `projects/<slug>/…` notes now
   count as project members even without frontmatter (was: invisible
   to `brain.list(project)`).
4. **Quiet chrome, first pass** — glow purge + resting mode tint cut
   55% → 30% (bright accents reserved for focus/active).

## Exists, needs proving in practice

5. **Session identity + error honesty** — done/error toasts carry
   titles; tool failures return causes. Unproven under real load.
6. **Calm output** — tool stacks collapse per call with ok/failed
   states; long code clamps at 30 lines with expand. The 50-line wall
   complaint was about the *opencode* transcript UI, not our thread —
   out of scope.
7. **Pull-context** — pinned notes attach in full, everything else is
   one-line summaries fetched on demand. Already the design; watch
   token pressure on long builds.

## Open, Parzi-side

8. **Linkable, citable notes** — notes have no link scheme; agents cite
   vault paths as dead text. Needs: stable `brain://` (or https) links
   returned by search/list, surviving renames. Real work, unscheduled.
9. **Rendering the agent can verify** — `ui.show_*` returns "rendered"
   and the agent flies blind. Page screenshots exist (`tab.shot`) but
   are not wired as an agent tool (base64 JPEGs would flood context).
   Needs an image-capable tool result path first.
10. **Interactive questions (Parzi UI)** — approvals are the only
    mid-turn user contact. The opencode layer has a question tool; our
    thread has no equivalent. Wanted for Build lane forks.

## Open, harness-side (not ours to fix)

11. **Persistent shell** — each bash call is a fresh universe in the
    opencode layer. Our agents inherit the vendor's shell semantics;
    document the detached-process pattern instead of building one.
