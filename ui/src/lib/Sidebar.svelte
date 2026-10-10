<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import TabBar from "./TabBar.svelte";
  import { isSystemTab, type Tab } from "./tabs";

  export let tabs: Tab[] = [];
  export let activeTabId = "";
  export let sessionLanes: Record<string, string> = {};

  const dispatch = createEventDispatcher<{
    select: { id: string };
    close: { id: string };
    move: { id: string; to: number };
    newTab: void;
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
  <div class="side-head">
    <button class="icon-btn wide" title="New tab (Ctrl+T)" on:click={() => dispatch("newTab")}>
      <Icon name="plus" size={13} stroke={2.2} />
      <span>New tab</span>
    </button>
  </div>

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
      <div class="sys-label">System</div>
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
  .sidebar {
    width: 204px;
    flex: none;
    height: 100%;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--line);
    background: var(--glass-bg);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    user-select: none;
  }
  .side-head {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: none;
    padding: 8px 8px 4px;
  }
  .icon-btn {
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 0 8px;
    min-width: 28px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 12px;
    cursor: pointer;
  }
  .icon-btn.wide {
    flex: 1;
  }
  .icon-btn:hover {
    background: var(--panel);
    border-color: var(--line);
    color: var(--text);
  }
  .side-tabs {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 8px 8px;
  }
  .side-foot {
    flex: none;
    max-height: 40%;
    overflow-y: auto;
    padding: 4px 8px 8px;
    border-top: 1px solid var(--line);
  }
  .sys-label {
    margin: 6px 4px 2px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--faint);
  }
</style>
