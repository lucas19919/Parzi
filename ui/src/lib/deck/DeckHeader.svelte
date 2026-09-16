<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { DeckTab } from "./fixtures";

  export let title = "";
  /** Sprint n of m; 0/0 while there is no plan. */
  export let sprint = { n: 0, m: 0 };
  export let lanesLive = 0;
  /** Round 1 is one machine: that is you, plus whoever the journal names. */
  export let tab: DeckTab = "dashboard";
  /** Transfer cards waiting for a person (badge on Build). */
  export let waiting = 0;

  const dispatch = createEventDispatcher<{ tab: { tab: DeckTab } }>();

  const TABS: DeckTab[] = ["dashboard", "build", "settings"];
  const NAME: Record<DeckTab, string> = { dashboard: "Dashboard", build: "Build", settings: "Settings" };
</script>

<div class="deck-head">
  <div class="row">
    <h1 class="title" title={title}>{title}</h1>
    {#if sprint.m}<span class="meta">sprint {sprint.n}/{sprint.m}</span>{/if}
    {#if lanesLive}<span class="live"><span class="dot" />{lanesLive} live</span>{/if}
  </div>
  <div class="tabs" role="tablist">
    {#each TABS as t (t)}
      <button class="tab" class:on={tab === t} role="tab" aria-selected={tab === t} on:click={() => dispatch("tab", { tab: t })}>
        {NAME[t]}
        {#if t === "build" && waiting}<span class="badge">{waiting}</span>{/if}
      </button>
    {/each}
  </div>
</div>

<style>
  .deck-head { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .row { display: flex; align-items: baseline; gap: 10px; min-width: 0; padding: 4px 2px 0; }
  .title {
    margin: 0; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-size: 18px; font-weight: 600; letter-spacing: -0.3px; color: var(--text);
  }
  .meta { font-size: 12px; color: var(--text-3); flex: none; }
  .live { display: inline-flex; align-items: center; gap: 5px; font-size: 12px; color: var(--ok); flex: none; }
  .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 8px var(--ok-line); }
  .tabs {
    display: flex; gap: 2px; border-bottom: 1px solid var(--line-2);
  }
  .tab {
    background: none; border: none; border-bottom: 2px solid transparent;
    color: var(--text-3); font: inherit; font-size: 13px; font-weight: 500;
    padding: 8px 12px 7px; margin-bottom: -1px; cursor: pointer;
  }
  .tab:hover { color: var(--text-2); }
  .tab.on { color: var(--text); font-weight: 600; border-bottom-color: var(--text); }
  .badge {
    margin-left: 6px; font-size: 10px; background: var(--warn-soft); border: 1px solid var(--warn-line);
    color: var(--warn); border-radius: var(--radius-pill); padding: 0 5px;
  }
</style>
