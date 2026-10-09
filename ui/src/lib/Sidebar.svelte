<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Icon from "./Icon.svelte";
  import TabBar from "./TabBar.svelte";
  import type { Tab } from "./tabs";
  import { updateVersion } from "./updateStore";
  import { startWindowDrag, windowClose, windowMaximize, windowMinimize } from "./windowChrome";

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

  let maximized = false;

  async function syncChrome() {
    try {
      const w = getCurrentWindow();
      const full = await w.isFullscreen().catch(() => false);
      maximized = full || (await w.isMaximized().catch(() => false));
      document.documentElement.classList.toggle("parzi-maximized", maximized);
    } catch {}
  }

  onMount(() => {
    void syncChrome();
    window.addEventListener("resize", syncChrome);
    return () => window.removeEventListener("resize", syncChrome);
  });
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
    <div class="window-controls">
      <button class="win-btn" title="Minimize" tabindex="-1" on:click={windowMinimize}>
        <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5h10" stroke="currentColor" /></svg>
      </button>
      <button class="win-btn" title={maximized ? "Restore" : "Maximize"} tabindex="-1" on:click={windowMaximize}>
        {#if maximized}
          <svg width="10" height="10" viewBox="0 0 10 10">
            <path d="M2.5 2V.5h7v7H8M.5 2.5h7v7h-7z" fill="none" stroke="currentColor" />
          </svg>
        {:else}
          <svg width="10" height="10" viewBox="0 0 10 10"><rect x=".5" y=".5" width="9" height="9" fill="none" stroke="currentColor" /></svg>
        {/if}
      </button>
      <button class="win-btn close" title="Close" tabindex="-1" on:click={windowClose}>
        <svg width="10" height="10" viewBox="0 0 10 10"><path d="M1 1l8 8M9 1L1 9" stroke="currentColor" stroke-width="1.1" /></svg>
      </button>
    </div>
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

  <div class="side-foot">
    <button class="icon-btn" title="Search (Ctrl+P)" on:click={() => dispatch("search")}>
      <Icon name="search" />
    </button>
    <button class="icon-btn" title="Brain (Ctrl+B)" on:click={() => dispatch("brain")}>
      <Icon name="brain" />
    </button>
    <button class="icon-btn" title="History (Ctrl+H)" on:click={() => dispatch("history")}>
      <Icon name="clock" />
    </button>
    <button class="icon-btn" title="Settings (Ctrl+,)" on:click={() => dispatch("settings")}>
      <Icon name="settings" />
      {#if $updateVersion}<span class="dot" />{/if}
    </button>
    <button class="icon-btn" title="Set up Parzi" on:click={() => dispatch("setup")}>
      <Icon name="spark" />
    </button>
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
    padding: 2px 8px;
  }
  .side-foot {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 38px;
    flex: none;
    padding: 0 8px;
    border-top: 1px solid var(--line);
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
  .dot {
    position: absolute;
    top: 5px;
    right: 5px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }
  .window-controls {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .win-btn {
    width: 28px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--muted);
    cursor: pointer;
  }
  .win-btn:hover {
    background: var(--line);
    color: var(--text);
  }
  .win-btn.close:hover {
    background: var(--bad);
    color: white;
  }
</style>
