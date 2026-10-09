<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import TabBar from "./TabBar.svelte";
  import type { Tab } from "./tabs";

  export let tabs: Tab[] = [];
  export let activeTabId = "";
  export let sessionLanes: Record<string, string> = {};

  const dispatch = createEventDispatcher<{
    select: { id: string };
    close: { id: string };
    move: { id: string; to: number };
  }>();
</script>

<aside class="sidebar">
  <div class="side-tabs">
    <TabBar
      {tabs}
      {activeTabId}
      {sessionLanes}
      vertical
      on:select={(e) => dispatch("select", e.detail)}
      on:close={(e) => dispatch("close", e.detail)}
      on:move={(e) => dispatch("move", e.detail)}
    />
  </div>
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
  .side-tabs {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 8px;
  }
</style>
