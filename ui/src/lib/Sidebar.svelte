<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import TabBar from "./TabBar.svelte";
  import { isSystemTab, type Tab } from "./tabs";

  export let tabs: Tab[] = [];
  export let activeTabId = "";
  export let sessionLanes: Record<string, string> = {};

  const dispatch = createEventDispatcher<{
    select: { id: string };
    close: { id: string };
    move: { id: string; to: number };
  }>();

  $: workTabs = tabs.filter((t) => !isSystemTab(t));
  $: systemTabs = tabs.filter((t) => isSystemTab(t));

  // Each TabBar reorders its own list; translate back to the global index.
  function moveWork(id: string, toLocal: number) {
    const globals = tabs.flatMap((t, i) => (isSystemTab(t) ? [] : [i]));
    dispatch("move", { id, to: globals[Math.max(0, Math.min(toLocal, globals.length - 1))] ?? 0 });
  }

  function moveSystem(id: string, toLocal: number) {
    const globals = tabs.flatMap((t, i) => (isSystemTab(t) ? [i] : []));
    dispatch("move", { id, to: globals[Math.max(0, Math.min(toLocal, globals.length - 1))] ?? 0 });
  }
</script>

<aside class="sidebar">
  <div class="side-tabs">
    <TabBar
      tabs={workTabs}
      {activeTabId}
      {sessionLanes}
      vertical
      on:select={(e) => dispatch("select", e.detail)}
      on:close={(e) => dispatch("close", e.detail)}
      on:move={(e) => moveWork(e.detail.id, e.detail.to)}
    />
  </div>

  {#if systemTabs.length}
    <div class="side-foot">
      <TabBar
        tabs={systemTabs}
        {activeTabId}
        {sessionLanes}
        vertical
        on:select={(e) => dispatch("select", e.detail)}
        on:close={(e) => dispatch("close", e.detail)}
        on:move={(e) => moveSystem(e.detail.id, e.detail.to)}
      />
    </div>
  {/if}
</aside>

<style>
  /* One glass column from the window's top edge down: the top bar's menu,
     Home and New tab buttons sit on it (the top bar is drawn above). The
     negative margin lifts it under the 38px top bar; stretch keeps it full height. */
  .sidebar {
    width: 204px;
    flex: none;
    margin-top: -38px;
    padding-top: 38px;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--line);
    background: var(--glass-bg);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    user-select: none;
  }
  .side-tabs {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 8px 8px 4px;
  }
  /* Settings, Brain and History stay at the bottom, set apart by the space alone. */
  .side-foot {
    flex: none;
    max-height: 40%;
    overflow-y: auto;
    padding: 4px 8px 8px;
  }
</style>
