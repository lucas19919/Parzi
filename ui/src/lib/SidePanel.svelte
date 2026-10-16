<script lang="ts">
  // The plan panel: the project's living plan as a picture. Items read their
  // state live (planModel); the only things to click are an item (it becomes
  // a quote in the composer), a PR badge (the PR opens in a tab) and a
  // session in Also running. Agents change the plan, not this panel.
  import { createEventDispatcher, onDestroy, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import PlanCard from "./PlanCard.svelte";
  import PlanNode, { railWidth } from "./PlanNode.svelte";
  import { brain, onPlan, onRunEvent, planApi, type PlanQuote, type PlanView, type PrInfo, type Project, type Question, type SessionMeta } from "./api";
  import type { Approval } from "./live";
  import { activity } from "./activity";
  import { clip, shortLabel } from "./format";
  import { coalesce } from "./threadList";
  import { folderName } from "./tabs";
  import { tooltip } from "./tooltip";
import {
  alsoRunning,
  deriveItems,
  groupItems,
  isRunning,
  newestActivity,
    progress,
    railLayout,
    sameSlug,
    sessionTone,
    STATE_WORDS,
    type ItemView,
    type RailRow,
  } from "./planModel";

  // The project's slug ("" when the folder isn't a project) and its name.
  export let project = "";
  export let projectTitle = "";
  // The composer's folder: where `gh` looks for PRs.
  export let folder = "";
  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let approvals: Approval[] = [];
  export let questions: Question[] = [];
  // On a draft the panel is its own glass card; in a session it shares the
  // session's surface.
  export let floating = false;
  export let dockW = 360;

  const dispatch = createEventDispatcher<{
    quote: PlanQuote;
    example: { text: string };
    openUrl: { url: string };
    openSession: { id: string };
    close: void;
  }>();

  const EXAMPLE = "Plan the next release.";
  const PR_EVERY = 120_000;

  function startResize(e: PointerEvent) {
    e.preventDefault();
    e.stopPropagation();
    const pointer = e.pointerId;
    const startX = e.clientX;
    const startW = dockW;
    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      window.removeEventListener("blur", end);
      try {
        localStorage.setItem("parzi.dock.w", String(dockW));
      } catch {}
    };
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      if ((ev.buttons & 1) === 0) {
        end();
        return;
      }
      dockW = Math.min(640, Math.max(280, Math.round(startW + (startX - ev.clientX))));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    window.addEventListener("blur", end);
  }

  // The plan: loaded when the panel opens or the project changes, and again
  // whenever it changes on disk or a run ends.
  let view: PlanView | null = null;
  let loadError = "";
  let viewFor: string | null = null;

  // A call made while the app is still starting can fail or never answer:
  // give it a few seconds, then say so (focus and run ends try again).
  const LOAD_WAIT = 8000;
  const loadPlan = coalesce(async () => {
    const slug = project;
    if (!slug) {
      view = null;
      return;
    }
    let timer = 0;
    try {
      const late = new Promise<never>((_, reject) => {
        timer = window.setTimeout(() => reject(new Error("it took too long to answer")), LOAD_WAIT);
      });
      const next = await Promise.race([planApi.view(slug), late]);
      if (slug !== project) return;
      view = next;
      loadError = "";
    } catch (e) {
      if (slug === project && !view) loadError = e instanceof Error ? e.message : String(e);
    } finally {
      window.clearTimeout(timer);
    }
  }, 150);

  $: if (project !== viewFor) {
    viewFor = project;
    view = null;
    loadError = "";
    hide();
    void loadPlan();
  }

  // Projects, for the name and the folder of a project opened from a server
  // session (whose folder isn't on this PC's composer).
  let projects: Project[] = [];
  $: known = projects.find((p) => sameSlug(p.slug, project)) ?? null;
  $: title = projectTitle || known?.title || project;
  $: prDir = folder || known?.folder || "";

  // PRs on the items' branches, from `gh` (an empty list without it).
  let prs: PrInfo[] = [];
  let prsFor: string | null = null;
  const loadPrs = coalesce(async () => {
    const dir = prDir;
    if (!dir || !plan) return;
    const list = await planApi.prs(dir).catch(() => [] as PrInfo[]);
    if (dir === prDir) prs = list;
  }, 500);

  $: plan = view?.plan ?? null;
  $: if (plan && prDir !== prsFor) {
    prsFor = prDir;
    prs = [];
    void loadPrs();
  }

  $: waiting = new Map<string, string>([
    ...approvals.map((a): [string, string] => [a.session, "Waiting for your approval"]),
    ...questions.map((q): [string, string] => [q.session, `Asks: ${clip(q.question, 80)}`]),
  ]);
  $: views = plan ? deriveItems(plan, { project, threads, running, waiting, prs, activity: $activity }) : [];
  $: groups = plan ? groupItems(plan, views) : { open: [], closed: [] };
  $: count = progress(views);
  $: pct = count.total ? Math.round((count.done / count.total) * 100) : 0;
  $: rows = groups.open.flatMap((g) => [
    ...(groups.open.length > 1 || g.phase ? [{ kind: "phase" as const, key: `phase:${g.phase}`, phase: g.phase }] : []),
    ...g.items.map((v) => ({ kind: "item" as const, key: v.item.id, v })),
  ]);
  $: layout = railLayout(
    rows.map((r) => (r.kind === "item" ? r.v.item.id : null)),
    new Map(views.map((v) => [v.item.id, v.item.needs])),
  );
  $: closedLayout = railLayout(
    groups.closed.map((v) => v.item.id),
    new Map(),
  );
  $: lanes = layout.lanes;
  $: gutter = railWidth(lanes);

  let showClosed = false;

  // This project's sessions outside the plan while they run or wait on you.
  $: linked = new Set(views.flatMap((v) => v.sessions.map((s) => s.id)));
  $: also = project ? alsoRunning(threads, project, linked, running, waiting) : [];
  // With no plan to show, the panel is the agents list: every live session.
  $: live = plan
    ? []
    : threads
        .filter((t) => isRunning(t, running) || waiting.has(t.id))
        .sort((a, b) => (Date.parse(b.updated) || 0) - (Date.parse(a.updated) || 0))
        .map((session) => ({ session, ids: [session.id] }));

  function subLine(v: ItemView): { text: string; strong: boolean } {
    if (v.state === "needs") return { text: v.reason, strong: true };
    if (v.state === "working") {
      const doing = v.doing ? clip(shortLabel(v.doing), 60) : v.agents > 1 ? "" : "Working";
      return { text: [doing, v.agents > 1 ? `${v.agents} agents` : ""].filter(Boolean).join(" · "), strong: false };
    }
    if ((v.state === "todo" || v.state === "started") && v.after.length) {
      const first = clip(v.after[0], 40);
      return { text: v.after.length > 1 ? `after ${first} and ${v.after.length - 1} more` : `after ${first}`, strong: false };
    }
    return { text: "", strong: false };
  }

  function alsoLine(ids: string[], first: SessionMeta): string {
    const ask = ids.map((id) => waiting.get(id) ?? "").find(Boolean);
    if (ask) return ask;
    const doing = newestActivity(ids, $activity);
    const agents = ids.length > 1 ? `${ids.length} agents` : "";
    return [doing ? clip(shortLabel(doing), 60) : agents ? "" : first.status === "queued" ? "Queued" : "Running", agents].filter(Boolean).join(" · ");
  }

  const CHECKS: Record<PrInfo["checks"], string> = { pass: "Checks pass", fail: "Checks fail", pending: "Checks running", none: "No checks" };

  function quote(v: ItemView) {
    hide();
    dispatch("quote", { project, id: v.item.id, title: v.item.title });
  }

  // The hover card: after a short rest on an item, at once when moving from
  // one item to the next.
  let hovered: { v: ItemView; anchor: HTMLElement } | null = null;
  let hoverTimer = 0;
  let closedAt = 0;

  function hoverIn(v: ItemView, anchor: HTMLElement) {
    clearTimeout(hoverTimer);
    const show = () => (hovered = { v, anchor });
    if (hovered || performance.now() - closedAt < 300) show();
    else hoverTimer = window.setTimeout(show, 350);
  }

  function hide() {
    clearTimeout(hoverTimer);
    if (hovered) closedAt = performance.now();
    hovered = null;
  }

  function focusIn(v: ItemView, e: FocusEvent) {
    const btn = e.currentTarget as HTMLElement;
    if (btn.matches(":focus-visible") && btn.parentElement) hoverIn(v, btn.parentElement);
  }

  // The card follows the live item (its state can change while it shows).
  $: card = hovered ? (views.find((v) => v.item.id === hovered?.v.item.id) ?? null) : null;

  onMount(() => {
    const loadProjects = () =>
      void brain
        .projects()
        .then((list) => (projects = list))
        .catch(() => {});
    loadProjects();
    void loadPlan();
    // Back to the window: anything that failed while it started loads again.
    const onFocus = () => {
      if (!projects.length) loadProjects();
      void loadPlan();
    };
    window.addEventListener("focus", onFocus);
    const unPlan = onPlan((e) => {
      if (sameSlug(e.project, project)) void loadPlan();
    });
    const unRun = onRunEvent((e) => {
      if (e.kind !== "done" && e.kind !== "error" && e.kind !== "paused") return;
      void loadPlan();
      void loadPrs();
    });
    const timer = window.setInterval(() => {
      if (!document.hidden) void loadPrs();
    }, PR_EVERY);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", onFocus);
      for (const un of [unPlan, unRun]) un.then((f) => f()).catch(() => {});
    };
  });

  onDestroy(() => clearTimeout(hoverTimer));
</script>

<aside class="panel" class:floating style:width="{dockW}px" aria-label="Plan">
  <div class="grip" on:pointerdown={startResize} title="Drag to resize the plan" />
  <header class="p-head">
    <div class="top">
      <h2>
        <!-- A plan named after its project says the name once. -->
        {#if plan && plan.title.trim().toLowerCase() !== title.trim().toLowerCase()}<span class="proj">{`${title} · `}</span>{plan.title}{:else}{plan?.title || title || "Plan"}{/if}
      </h2>
      <button class="x" use:tooltip={"Close plan (Ctrl+J)"} on:click={() => dispatch("close")}><Icon name="close" size={12} /></button>
    </div>
    {#if plan}
      {#if plan.goal}<p class="goal">{plan.goal}</p>{/if}
      <div class="meter">
        <span class="bar" aria-hidden="true"><i style:width="{pct}%" /></span>
        <span class="count">{count.done} of {count.total} done</span>
      </div>
    {/if}
  </header>

  <div class="body" on:scroll={hide}>
    {#if !project}
      <p class="empty">
        {#if !floating}This session has no project, so it has no plan.{:else if folder}{folderName(folder)} isn't a project yet, so it has no plan.{:else}Pick a folder in the composer to see its plan.{/if}
      </p>
    {:else if loadError && !view}
      <p class="empty">Couldn't load the plan: {loadError}</p>
    {:else if !view}
      <p class="empty">Loading the plan</p>
    {:else if view && !plan}
      {#if view.error}
        <p class="empty">This project's plan can't be read: {view.error}</p>
      {:else}
        <div class="empty">
          <p>No plan for this project. Ask an agent to plan it, for example:</p>
          <button class="example" on:click={() => dispatch("example", { text: EXAMPLE })}>{EXAMPLE}</button>
        </div>
      {/if}
    {:else if plan}
      <ol class="items" style:--gutter="{gutter}px">
        {#each rows as r, i (r.key)}
          {@const geo = layout.rows[i]}
          {#if r.kind === "phase"}
            <li class="phase">
              <PlanNode row={geo} {lanes} />
              <span role="heading" aria-level="3">{r.phase || "Plan"}</span>
            </li>
          {:else}
            {@const v = r.v}
            {@const sub = subLine(v)}
            <!-- svelte-ignore a11y-no-noninteractive-element-interactions -->
            <li class="item" on:pointerenter={(e) => hoverIn(v, e.currentTarget)} on:pointerleave={hide}>
              <PlanNode row={geo} {lanes} node={v.state} checks={v.pr?.state === "open" ? v.pr.checks : ""} />
              <button
                class="pick"
                aria-describedby={hovered?.v.item.id === v.item.id ? "plan-card" : undefined}
                on:click={() => quote(v)}
                on:focus={(e) => focusIn(v, e)}
                on:blur={hide}
              >
                <span class="t">{v.item.title}</span>
                <span class="state" role="img" aria-label={STATE_WORDS[v.state]} />
                {#if sub.text}<span class="sub" class:strong={sub.strong}>{sub.text}</span>{/if}
              </button>
              {#if v.pr && v.pr.state === "open"}
                {@const pr = v.pr}
                <button
                  class="pr"
                  use:tooltip={`Open ${pr.draft ? "draft " : ""}PR #${pr.number}: ${pr.title}. ${CHECKS[pr.checks]}`}
                  on:click={() => dispatch("openUrl", { url: pr.url })}
                >
                  <Icon name="pr" size={12} />
                  <span>PR #{pr.number}</span>
                  {#if pr.checks !== "none"}
                    <span class="checks {pr.checks}"><Icon name={pr.checks === "pass" ? "checkCircle" : pr.checks === "fail" ? "xCircle" : "clock"} size={12} /></span>
                  {/if}
                </button>
              {/if}
            </li>
          {/if}
        {/each}
      </ol>
      {#if groups.closed.length}
        {@const done = groups.closed.filter((v) => v.state === "done").length}
        <button class="fold" aria-expanded={showClosed} on:click={() => (showClosed = !showClosed)}>
          <Icon name={showClosed ? "chevDown" : "chevRight"} size={12} />
          <span>Done · {done}</span>
        </button>
        {#if showClosed}
          <ol class="items closed" style:--gutter="{gutter}px">
            {#each groups.closed as v, i (v.item.id)}
              <!-- svelte-ignore a11y-no-noninteractive-element-interactions -->
              <li class="item" on:pointerenter={(e) => hoverIn(v, e.currentTarget)} on:pointerleave={hide}>
                <PlanNode row={closedLayout.rows[i]} {lanes} node={v.state} />
                <button class="pick" on:click={() => quote(v)} on:focus={(e) => focusIn(v, e)} on:blur={hide}>
                  <span class="t" class:struck={v.state === "dropped"}>{v.item.title}</span>
                  <span class="state" role="img" aria-label={STATE_WORDS[v.state]} />
                </button>
              </li>
            {/each}
          </ol>
        {/if}
      {/if}
    {/if}

    {#if live.length}
      <h3 class="also-head">Running now</h3>
      <div class="also" data-ui-list>
        {#each live as row (row.session.id)}
          {@const tone = sessionTone(row.session, running, waiting)}
          <button class="also-row" on:click={() => dispatch("openSession", { id: row.session.id })}>
            <i class="dot t-{tone}" />
            <span class="a-text">
              <span class="t">{row.session.title || "Untitled session"}</span>
              <span class="sub">{alsoLine(row.ids, row.session)}</span>
            </span>
          </button>
        {/each}
      </div>
    {/if}
    {#if also.length}
      <h3 class="also-head">Also running</h3>
      <div class="also" data-ui-list>
        {#each also as row (row.session.id)}
          {@const tone = sessionTone(row.session, running, waiting)}
          <button class="also-row" on:click={() => dispatch("openSession", { id: row.session.id })}>
            <i class="dot t-{tone}" />
            <span class="a-text">
              <span class="t">{row.session.title || "Untitled session"}</span>
              <span class="sub">{alsoLine(row.ids, row.session)}</span>
            </span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
</aside>

{#if card && hovered}
  <PlanCard id="plan-card" view={card} anchor={hovered.anchor} {running} {waiting} />
{/if}

<style>
  .panel {
    position: relative;
    flex: none;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--line);
    background: transparent;
  }
  .panel.floating {
    border-left: none;
  }
  .grip {
    position: absolute;
    top: 0;
    bottom: 0;
    left: -4px;
    width: 9px;
    cursor: ew-resize;
    z-index: 5;
  }
  .grip:hover::after,
  .grip:active::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3px;
    width: 2px;
    border-radius: 1px;
    background: var(--accent);
    opacity: 0.7;
  }
  .p-head {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px 12px 10px 16px;
  }
  .top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 8px;
  }
  h2 {
    min-width: 0;
    margin: 3px 0 0;
    font-size: var(--fs-base);
    font-weight: 600;
    line-height: 1.4;
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .proj {
    color: var(--muted);
    font-weight: 500;
  }
  .goal {
    margin: 0;
    font-size: var(--fs-md);
    line-height: 1.45;
    color: var(--muted);
  }
  .meter {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 2px;
  }
  .bar {
    flex: 1;
    height: 3px;
    overflow: hidden;
    border-radius: 999px;
    background: var(--selected);
  }
  .bar i {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--muted);
    transition: width 400ms var(--ease-out);
  }
  .count {
    flex: none;
    font-size: var(--fs-sm);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .x {
    flex: none;
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .x:hover {
    background: var(--hover);
    color: var(--text);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 2px 8px 24px;
  }
  .empty {
    margin: 16px 8px;
    font-size: var(--fs-md);
    line-height: 1.6;
    color: var(--muted);
  }
  .empty p {
    margin: 0 0 8px;
  }
  .example {
    min-height: 28px;
    padding: 4px 12px;
    background: var(--hover);
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--text);
    font-size: var(--fs-md);
    cursor: pointer;
  }
  .example:hover {
    background: var(--selected);
  }
  .items {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .items li {
    position: relative;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding-left: var(--gutter);
  }
  .items li.phase {
    min-height: 30px;
    align-items: flex-end;
    padding-bottom: 4px;
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--muted);
  }
  .phase span {
    padding-left: 8px;
  }
  .pick {
    flex: 1;
    min-width: 0;
    min-height: 28px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    padding: 5px 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-md);
    line-height: 18px;
    text-align: left;
    cursor: pointer;
  }
  .pick:hover {
    background: var(--hover);
  }
  .t {
    max-width: 100%;
    overflow-wrap: anywhere;
  }
  .struck {
    color: var(--muted);
    text-decoration: line-through;
  }
  .sub {
    max-width: 100%;
    font-size: var(--fs-sm);
    line-height: 16px;
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .sub.strong {
    color: var(--text);
  }
  /* The state, for screen readers: the node beside the row shows it. */
  .state {
    position: absolute;
    width: 0;
    height: 0;
  }
  .pr {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    margin-top: 2px;
    padding: 0 8px;
    background: none;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .pr:hover {
    background: var(--hover);
    color: var(--text);
  }
  .checks {
    display: inline-flex;
  }
  .checks.pass {
    color: var(--ok);
  }
  .checks.fail {
    color: var(--bad);
  }
  .fold {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    margin: 10px 0 2px;
    padding: 0 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .fold:hover {
    background: var(--hover);
    color: var(--text);
  }
  .also-head {
    margin: 18px 8px 4px;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--muted);
  }
  .also {
    display: flex;
    flex-direction: column;
  }
  .also-row {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    min-height: 28px;
    padding: 6px 8px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-md);
    line-height: 18px;
    text-align: left;
    cursor: pointer;
  }
  .also-row:hover {
    background: var(--hover);
  }
  .a-text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    margin-top: 5px;
    border-radius: 50%;
    background: var(--faint);
  }
  .t-live {
    background: var(--ok);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 22%, transparent);
  }
  .t-need {
    background: var(--warn);
  }
  .t-bad {
    background: var(--bad);
  }
  .t-ok {
    background: var(--muted);
  }
</style>
