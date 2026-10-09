<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import TabBar from "./TabBar.svelte";
  import WinControls from "./WinControls.svelte";
  import type { Tab } from "./tabs";
  import { startWindowDrag } from "./windowChrome";

  export let tabs: Tab[] = [];
  export let activeTabId = "";
  export let sessionLanes: Record<string, string> = {};

  const dispatch = createEventDispatcher<{
    select: { id: string };
    close: { id: string };
    move: { id: string; to: number };
    newTab: void;
    home: void;
    settings: void;
    search: void;
    update: void;
    brain: void;
    history: void;
    setup: void;
  }>();

</script>

<!-- svelte-ignore a11y-no-static-element-interactions a11y-no-noninteractive-element-interactions -->
<aside class="sidebar" on:mousedown={startWindowDrag}>
  <div class="side-head">
    <button class="icon-btn" title="Home" on:click={() => dispatch("home")}>
      <Icon name="home" />
    </button>
    <button class="icon-btn" title="New tab (Ctrl+T)" on:click={() => dispatch("newTab")}>
      <Icon name="plus" />
    </button>
    <span class="head-spacer" />
  </div>

  <div class="side-tabs">
    <TabBar
      {tabs}
      {activeTabId}
      {sessionLanes}
      vertical
      on:select={(e) => dispatch("select", e.detail)}
      on:close={(e) => dispatch("close", e.detail)}
      on:move={(e) => dispatch("move", e.detail)}
      on:newTab={() => dispatch("newTab")}
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
  .side-head {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 38px;
    flex: none;
    padding: 0 8px;
  }
  .head-spacer {
    flex: 1;
  }
  .side-tabs {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 2px 8px 8px;
  }
  .icon-btn {
    position: relative;
    width: 28px;
    height: 28px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .icon-btn:hover {
    background: var(--panel);
    border-color: var(--line);
    color: var(--text);
  }
</style>
