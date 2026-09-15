# Projects, reimagined — vision doc

Status: proposal, 2026-09-15. Scope agreed with Lucas: vision first, build after.
Replaces the current "shitty trello board" (LivingPlanView checkbox list + hidden roster + dead ProjectOverview).

## 1. What Lucas actually said (requirements, verbatim-ish)

- "Nothings useful, this isnt made to iterate." The current project page is write-once: type a plan, dispatch once, done. Real work is loops — run, see, correct, re-run.
- "I have to hand type the model names." The roster is three blank text inputs. No defaults shown, no picker, no validation. Typing `provider/model` strings is the opposite of a harness's job.
- "I need to start a new convo everytime." Continuing work means a fresh thread with no lineage. Retry / continue-in-place / fork-at-step exist in the backend but are buried or missing in the project view.
- "Subconvos arent linked to the project." Backend *does* inherit `project`/`lane` on `create_subsession` (orchestrator.rs:546-547) and on UI-spawned subsessions (main.rs:164-175). The link breaks in the UI: `ProjectMainPage` filters `t.project === project` but only shows live top-8; the sidebar groups by time, not project; opening a thread drops all project context (no breadcrumb, no rail). So this is a UI-legibility bug, not a storage bug — fix by making project membership visible everywhere, and by treating the whole thread *tree* (root + descendants) as the unit, never a lone thread.
- "The project is for some reason only seen on the workspace." Today the Mission Control only exists on the workspace click. Open a thread and the project vanishes. Project context must be persistent chrome (breadcrumb + rail), not a page you visit.
- "A workspace like strohmann might have a project called lemma, that would be a subfolder, and that subfolder would hold the domain specific context. So we need like a hierarchy here." Flat `projects/<name>` list is wrong. Need nesting with context inheritance.
- Project = mission with a goal (agreed). Not a folder with threads.

## 2. The new model: Workspace > Project > Thread tree

```
Workspace  (repo root, e.g. `strohmann` — has SYSTEM.md, parzi.toml, PLAN.md, KNOWLEDGE.md)
└── Project (subfolder, e.g. `lemma/` — has its OWN SYSTEM.md / PLAN.md / KNOWLEDGE.md overlays)
    └── Thread tree (root thread + subsessions, arbitrary depth — the unit of work)
```

- **Workspace** = what the sidebar today calls "Workspaces" and the backend calls "projects". Rename to Workspace everywhere. One workspace = one checkout root + one crew + one memory base.
- **Project** = a named mission *inside* a workspace, bound to a subfolder. `strohmann/lemma/` holds lemma's domain context. A workspace with no sub-projects has exactly one implicit project = the root mission (today's behaviour, zero migration pain).
- **Thread tree** = root + all descendants. Filtering, costs, and plan linkage always operate on the tree, never on a single thread. If any member is live, the tree is live.
- **Lanes stop being fake projects.** Today's lanes (`projects/<p>/lanes/<l>/SYSTEM.md`) overlap 90% with the proposed sub-project. Resolution: lanes become *execution targets* (mode + allowed tools + worktree flag + model override), addressable as `project/lane`, while *domain context* lives at the project level. A lane never owns a PLAN.md or KNOWLEDGE.md; only workspaces and projects do.

Disk truth (additive, backwards compatible):

```
~/.parzi/projects/<workspace>/
  parzi.toml          # root, crew, brief (goal)
  SYSTEM.md           # workspace brief + conventions
  PLAN.md             # workspace-level milestones (existing behaviour)
  KNOWLEDGE.md        # workspace memory (existing)
  subprojects/<project>/
    SYSTEM.md         # domain context overlay (NEW — the `lemma` case)
    PLAN.md           # project milestones (NEW — falls back to workspace PLAN.md)
    KNOWLEDGE.md      # project memory overlay (NEW — falls back to workspace one)
  lanes/<lane>/       # unchanged: execution targets only
```

Context builder precedence (inner wins, outer is background):
`global SYSTEM` < `workspace SYSTEM.md` < `project SYSTEM.md` < `lane SYSTEM.md` < `KNOWLEDGE.md (project, then workspace)` < `latest user msg`. Same rule the builder already uses for project>lane, extended one level down. No new file formats.

## 3. Design principles (the anti-Trello list)

1. **Iteration is the primitive, not the checkbox.** Every thread tree offers Continue (same thread), Retry (re-run last turn), Fork-at-step, Spawn-subsession — inline, one click. Starting a blank conversation is the *last* resort, never the only path. The project page answers "what happened, what next, who does it" without forcing a new convo.
2. **Never hand-type a model name again.** The crew shows *resolved* models (`roster value` → else `lane default` → else `Smart Auto`), each row a real picker fed by the model catalog, with auth state (`ok/missing/expired`) inline. Blank = Auto, displayed as Auto, not as an empty box. The omnibar model picker and the crew picker are the same component.
3. **Project context is chrome, not a page.** Breadcrumb `strohmann / lemma / thread title` in the titlebar everywhere; a slim project rail (goal, health, live trees, next action) follows you into threads. You never "lose" the project by opening work.
4. **State is derived, not declared.** Progress, health, and "what's next" compute from plan + thread trees + recency. Manual checkboxes remain as overrides, but an empty plan with live work still reads as "working", and a full plan with dead threads reads as "stalled". A board nobody updates is a lie; a page that derives is always fresh.
5. **Memory is visible and earned.** KNOWLEDGE.md renders as a timeline (parsed `- [ts] note` lines, newest first) with a one-line "log what you learned" box. Raw-textarea editing moves to a details disclosure. Agents append via the existing `append_knowledge` tool; humans see it land without reload.
6. **One overview, not two.** Delete `ProjectOverview.svelte` (dead code — `App.svelte` renders `ProjectMainPage`). One Mission Control per project, embedded in the same chrome as threads.

## 4. What the project page becomes (Mission Control v2)

Single scroll, five blocks, no tabs:

1. **Mission header.** Name + goal (first line of project SYSTEM.md / brief in parzi.toml, inline-editable), health pill (derived: `Working` / `Needs attention` / `Idle` / `Stalled`), progress bar (`done/total` from parsed plan), aggregate tokens + cost for the project, branch + subfolder path. Primary CTA is computed, not static: `Execute next: <task>` when idle with pending work, `Review <live tree>` when work runs, `Define the first milestone` when the plan is the default stub.
2. **Now / Next / Done — derived work list, not Trello.** Milestone-grouped tasks from the existing plan parser (`plan.rs` already gives status + lane tags + worktree flags). Each task shows: status cycler (pending → in-progress → done, writes back via existing `saveProjectPlan`), lane chip, worktree badge, and **linked trees** — thread trees whose `lane == task.lane`, with live dot + title + one-click open. "Next" is `next_pending_task` (already exists in core) surfaced as a button, not buried in markdown.
3. **Crew — the roster, fixed.** Three cards (Header/Architect, Orchestrator/PM, Implementation/Builder), each: one-line purpose, resolved-model picker (catalog + auth state), effort picker (low→ultra, existing options API). Save writes the existing `save_project_roster`. No blank string inputs ever again.
4. **Memory — knowledge + docs.** KNOWLEDGE timeline + add box (appends via existing save path, parsed client-side); project docs (`list_project_docs` already returns system+root entries) as quick-open chips into the inspector deck. SYSTEM.md overlay gets an inline editor with "inherits from workspace" hint when empty.
5. **Activity — the project's trees.** Thread trees in this project (root + descendants collapsed to one row each): status, live tool when running (`sessionTools` already flows to the Agents deck), tokens, rel-time, Continue / Fork / Kill inline. New conversation is a secondary button here, not the page's whole personality.

What dies: `LivingPlanView`'s checkbox-only list with hand-typed lane input; the collapsed `<details>Agents & models` hiding roster + header prompt + knowledge textarea; `ProjectOverview.svelte`; the "+ New conversation" as the only action.

## 5. Backend work (small, additive — no migrations)

1. **Subproject paths + fallback reads.** `plan_path`, `project_knowledge_path`, roster/system resolution each gain: `subprojects/<p>/FILE` when bound, else workspace-level file. Reuse `safe_name` + `lane_root` patterns. Nothing moves on disk for existing users.
2. **Project binding on threads.** `SessionMeta` gains `subproject: Option<String>` (default None = root mission). `create_subsession` / agent `session.spawn` inherit it exactly like `project`/`lane` today. Old sessions read as root mission — no backfill.
3. **Derived-state helpers in core (pure, testable).** `project_health(plan, trees) -> Working|NeedsAttention|Idle|Stalled`, `link_trees_to_tasks(tasks, trees) -> Vec<(task, tree_ids)>` by lane match, `project_totals(trees) -> tokens/cost`. UI consumes; agents can too (Header briefing).
4. **Goal storage.** `brief: Option<String>` (+ `subproject root: Option<String>`) in `parzi.toml` next to `[roles.*]`. `save_project_roster`'s merge pattern already preserves unknown keys — extend it, don't rewrite it.

Explicitly out: vector DB, cross-workspace queries, permissions, renaming `projects/` on disk (internal dir name stays; only UI + API naming says workspace/project).

## 6. Build order (after this doc is approved)

- P1 — UI-only Mission Control v2 over existing APIs (header with derived health + totals, task↔tree linking by lane, crew pickers with catalog, knowledge timeline parsing, project breadcrumb chrome). Delete `ProjectOverview.svelte`. No backend change; subproject blocks render "root mission" state.
- P2 — Backend: subproject dirs + fallback reads + `subproject` on sessions + `brief` in parzi.toml. Wire editors.
- P3 — Derived helpers in core + Header/Orchestrator prompts reference them (plan updates + knowledge appends keep working through existing tools).
- Each phase: `cargo test`, `cargo clippy -- -D warnings`, UI smoke (project page with live + idle + empty states).

## 7. Open questions for Lucas

1. Lane vs subproject boundary: proposal says lanes = execution targets, subprojects = domain missions. Do you buy that, or should lanes just *become* subprojects (merge the two concepts)?
2. Should subsessions be first-class rows in Activity, or always collapsed under their root tree?
3. Health derivation weights: is "killed recently + pending work = Needs attention" right, or do killed runs mean "user took over, all fine"?
