<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { DeckTab } from "./fixtures";

  export let title = "";
  export let workspace = "";
  /** Sprint n of m; 0/0 while there is no plan. */
  export let sprint = { n: 0, m: 0 };
  export let lanesLive = 0;
  /** Round 1 is one machine: that is you, plus whoever the journal names. */
  export let people: string[] = [];
  export let tab: DeckTab = "project";
  /** Transfer cards waiting for a person (badge on the Plan tab). */
  export let waiting = 0;

  const dispatch = createEventDispatcher<{ tab: { tab: DeckTab } }>();

  const TABS: DeckTab[] = ["project", "plan", "activity"];
  const NAME: Record<DeckTab, string> = { project: "Project", plan: "Plan", activity: "Activity" };
</script>

<div class="deck-head">
  <span class="title" title={title}>{title}</span>
  <span class="sep">·</span>
  <span class="ws">{workspace}</span>
  {#if sprint.m}
    <span class="sep">·</span>
    <span class="sprint">sprint {sprint.n}/{sprint.m}</span>
  {/if}
  {#if lanesLive}
    <span class="sep">·</span>
    <span class="live"><span class="dot" />{lanesLive} lane{lanesLive === 1 ? "" : "s"}</span>
  {/if}
  {#each people as p (p)}
    <span class="person" title={p}>{p}</span>
  {/each}

  <span class="spacer" />

  <div class="tabs" role="tablist">
    {#each TABS as t (t)}
      <button class="tab" class:on={tab === t} role="tab" aria-selected={tab === t} on:click={() => dispatch("tab", { tab: t })}>
        {NAME[t]}
        {#if t === "plan" && waiting}<span class="badge">{waiting}</span>{/if}
      </button>
    {/each}
  </div>
</div>

<style>
  .deck-head {
    display: flex; align-items: center; gap: 7px; flex-wrap: nowrap;
    padding: 10px 2px 12px; border-bottom: 1px solid var(--line-2); min-width: 0;
  }
  .title {
    flex: 1 1 8em; min-width: 8em;
    font-size: 15px; font-weight: 700; color: var(--text); letter-spacing: -0.2px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .sep { color: var(--text-4); }
  .ws, .sprint { font-size: 12px; color: var(--text-3); }
  .live { display: inline-flex; align-items: center; gap: 5px; font-size: 11.5px; color: var(--ok); }
  .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 8px var(--ok-line); }
  .person {
    font-size: 11px; color: var(--text-3); background: var(--surface-2);
    border-radius: var(--radius-pill); padding: 1px 8px;
  }
  .spacer { flex: 1; min-width: 8px; }
  .tabs { display: inline-flex; flex: none; background: var(--surface-1); border: 1px solid var(--line-2); border-radius: var(--radius-2); padding: 2px; }
  .tab {
    background: transparent; border: none; border-radius: var(--radius-1); color: var(--text-3);
    font: inherit; font-size: 12px; padding: 3px 12px; cursor: pointer;
    display: inline-flex; align-items: center; gap: 5px;
  }
  .tab:hover { color: var(--text); }
  .tab.on { background: var(--surface-3); color: var(--text); }
  .badge {
    font-size: 10px; background: var(--warn-soft); border: 1px solid var(--warn-line);
    color: var(--warn); border-radius: var(--radius-pill); padding: 0 5px;
  }
</style>
