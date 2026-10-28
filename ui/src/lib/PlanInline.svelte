<script lang="ts">
  // The project's plan as a checklist at the end of the thread: what is
  // done, what runs, what waits on you. Clicking an item quotes it into
  // the composer; agents change the plan, not this list.
  import { createEventDispatcher } from "svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import type { PlanQuote, Question, SessionMeta } from "./api";
  import type { Approval } from "./live";
  import { activity } from "./activity";
  import { planState } from "./planStore";
  import { deriveItems, groupItems, progress, sameSlug, STATE_WORDS, type ItemView } from "./planModel";
  import { tooltip } from "./tooltip";

  export let project = "";
  export let session = "";
  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let approvals: Approval[] = [];
  export let questions: Question[] = [];
  // Items already quoted into the composer: clicking one quotes it, clicking
  // it again takes it back out.
  export let quotes: PlanQuote[] = [];
  // Shown even when this session works on no item (asked for from the
  // info column).
  export let force = false;
  // Out: whether the checklist shows, so the info column can skip its row.
  export let visible = false;

  const dispatch = createEventDispatcher<{ quote: PlanQuote; unquote: PlanQuote; close: void }>();

  let open = true;
  let showDone = false;

  $: plan = $planState.project === project ? $planState.plan : null;
  $: error = $planState.project === project ? $planState.error : "";
  $: waiting = new Map<string, string>([
    ...approvals.map((a): [string, string] => [a.session, "Waiting for your approval"]),
    ...questions.map((q): [string, string] => [q.session, "Has a question"]),
  ]);
  $: views = plan ? deriveItems(plan, { project, threads, running, waiting, prs: $planState.prs, activity: $activity }) : [];
  $: groups = plan ? groupItems(plan, views) : { open: [], closed: [] };
  $: items = groups.open.flatMap((g) => g.items);
  $: count = progress(views);
  // The project's plan is its long-lived roadmap: it shows in a thread
  // only when the person opens it (a session's own steps are its Tasks).
  // A forced-open panel stays open even with no plan yet or an unreadable
  // note, so a click always shows something and the X always hides it.
  $: shown = !!project && force;
  $: visible = shown;
  $: path = `projects/${project}/plan.md`;

  function quoted(v: ItemView) {
    return (quotes ?? []).some((q) => q.id === v.item.id && sameSlug(q.project, project));
  }

  function quote(v: ItemView) {
    const q = { project, id: v.item.id, title: v.item.title };
    if (quoted(v)) dispatch("unquote", q);
    else dispatch("quote", q);
  }
</script>

{#if shown}
  <section class="plan" aria-label="Plan">
    <div class="head-row">
      <button class="head" aria-expanded={open} on:click={() => (open = !open)}>
        <Icon name={open ? "chevDown" : "chevRight"} size={12} />
        <span class="title">{plan?.title || "Plan"}</span>
        {#if plan}<span class="count">{count.done} of {count.total}</span>{/if}
      </button>
      <button class="x" use:tooltip={"Hide plan"} aria-label="Hide plan" on:click={() => dispatch("close")}>
        <Icon name="close" size={12} />
      </button>
    </div>
    {#if open}
      {#if error}
        <p class="empty">The plan note {path} can't be read: {error}. Fix it in the Brain, or ask the agent for a new plan.</p>
      {:else if !plan}
        <p class="empty">No plan yet. Ask the agent for one when the work spans sessions.</p>
      {:else}
        <ul transition:slide={{ duration: 160, easing: cubicOut }}>
          {#each items as v (v.item.id)}
            <li>
              <button
                class="item"
                class:mine={v.sessions.some((s) => s.id === session)}
                class:picked={quoted(v)}
                use:tooltip={quoted(v) ? "Quoted. Click to remove it." : `${STATE_WORDS[v.state]}. Click to quote it.`}
                on:click={() => quote(v)}
              >
                <span class="mark {v.state}" aria-label={STATE_WORDS[v.state]} />
                <span class="t">{v.item.title}</span>
                {#if quoted(v)}<span class="why">Quoted</span>
                {:else if v.state === "needs" && v.reason}<span class="why">{v.reason}</span>{/if}
              </button>
            </li>
          {/each}
          {#if groups.closed.length}
            <li>
              <button class="item fold" on:click={() => (showDone = !showDone)}>
                <span class="mark done" />
                <span class="t">{showDone ? "Hide done" : `Done · ${groups.closed.length}`}</span>
              </button>
            </li>
            {#if showDone}
              {#each groups.closed as v (v.item.id)}
                <li>
                  <button class="item closed" class:picked={quoted(v)} on:click={() => quote(v)}>
                    <span class="mark {v.state}" />
                    <span class="t" class:struck={v.state === "dropped"}>{v.item.title}</span>
                  </button>
                </li>
              {/each}
            {/if}
          {/if}
        </ul>
      {/if}
    {/if}
  </section>
{/if}

<style>
  .plan {
    margin: 4px 0 0;
    padding: 8px 6px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .head-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
    padding: 2px 6px 6px;
    background: none;
    border: none;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
    text-align: left;
  }
  .x {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    margin-bottom: 4px;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .x:hover {
    background: color-mix(in srgb, var(--text) 6%, transparent);
    color: var(--text);
  }
  .empty {
    margin: 0 6px 4px;
    color: var(--muted);
    font-size: var(--fs-sm);
    line-height: 1.5;
    overflow-wrap: anywhere;
  }
  .item.picked {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .head:hover {
    color: var(--text);
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
    color: var(--text);
  }
  .count {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  ul {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 30px;
    padding: 4px 6px;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .item:hover {
    background: color-mix(in srgb, var(--text) 6%, transparent);
  }
  .item.mine .t {
    font-weight: 600;
  }
  .item.fold,
  .item.closed {
    color: var(--muted);
  }
  .t {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .struck {
    text-decoration: line-through;
  }
  .why {
    flex: none;
    max-width: 45%;
    margin-left: auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--warn);
  }
  /* One small circle per state, drawn: hollow to do, spinning while an
     agent works, amber when it needs you, filled when done. */
  .mark {
    position: relative;
    width: 14px;
    height: 14px;
    flex: none;
    border: 1.5px solid var(--muted);
    border-radius: 50%;
  }
  .mark.started {
    border-style: dashed;
  }
  .mark.working {
    border-color: var(--line);
    border-top-color: var(--accent);
    animation: spin 0.9s linear infinite;
  }
  .mark.review {
    border-color: var(--accent);
  }
  .mark.review::after {
    content: "";
    position: absolute;
    inset: 3px;
    border-radius: 50%;
    background: var(--accent);
  }
  .mark.needs {
    border-color: var(--warn);
    background: var(--warn);
  }
  .mark.done {
    border-color: var(--ok);
    background: var(--ok);
  }
  .mark.done::after {
    content: "";
    position: absolute;
    left: 3.5px;
    top: 1.5px;
    width: 3px;
    height: 6px;
    border: solid var(--bg);
    border-width: 0 1.5px 1.5px 0;
    transform: rotate(45deg);
  }
  .mark.dropped {
    border-color: var(--line);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .mark.working {
      animation: none;
    }
  }
</style>
