<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import TabBar from "./TabBar.svelte";
  import type { Tab } from "./tabTypes";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { startWindowDrag, windowClose, windowMaximize, windowMinimize } from "./windowChrome";

  export let tabs: Tab[] = [];
  export let activeTabId: string | null = null;

  let menuOpen = false;
  let isMaximized = false;

  const dispatch = createEventDispatcher<{
    selectTab: { id: string };
    closeTab: { id: string };
    newTab: void;
    home: void;
    openSettings: void;
    openSessionPopup: void;
    checkUpdates: void;
    togglePanel: void;
  }>();

  async function syncChrome() {
    try {
      const w = getCurrentWindow();
      const full = await w.isFullscreen().catch(() => false);
      const max = full ? false : await w.isMaximized().catch(() => false);
      isMaximized = Boolean(full || max);
      document.documentElement.classList.toggle("is-full", full || max);
      document.documentElement.classList.toggle("parzi-maximized", full || max);
    } catch {}
  }

  onMount(() => {
    syncChrome();
    window.addEventListener("resize", syncChrome);
    function onDocClick(e: MouseEvent) {
      if (!(e.target as HTMLElement).closest(".menu-container")) {
        menuOpen = false;
      }
    }
    document.addEventListener("click", onDocClick);
    return () => {
      window.removeEventListener("resize", syncChrome);
      document.removeEventListener("click", onDocClick);
    };
  });

  const onMouseDown = startWindowDrag;
  const handleMin = windowMinimize;
  const handleMax = windowMaximize;
  const handleClose = windowClose;

  function toggleMenu() {
    menuOpen = !menuOpen;
  }
</script>

<div class="topbar-shell" data-tauri-drag-region on:mousedown={onMouseDown}>
  <div class="left-cluster" data-tauri-drag-region>
    <!-- Menu Button (Hamburger) -->
    <div class="menu-container">
      <button
        class="icon-btn menu-btn"
        class:active={menuOpen}
        title="Menu"
        on:click={toggleMenu}
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
          <path d="M4 6h16M4 12h16M4 18h16" />
        </svg>
      </button>

      {#if menuOpen}
        <div class="dropdown-menu" role="menu">
          <button class="menu-item" on:click={() => { menuOpen = false; dispatch("newTab"); }}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
            <span class="mi-label">New Tab</span>
            <span class="mi-key">Ctrl+T</span>
          </button>
          <button class="menu-item" on:click={() => { menuOpen = false; dispatch("openSettings"); }}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
            <span class="mi-label">Settings</span>
            <span class="mi-key">Ctrl+,</span>
          </button>
          <button class="menu-item" on:click={() => { menuOpen = false; dispatch("openSessionPopup"); }}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
            <span class="mi-label">Search</span>
            <span class="mi-key">Ctrl+P</span>
          </button>
          <div class="menu-sep" />
          <button class="menu-item" on:click={() => { menuOpen = false; dispatch("home"); }}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10.5L12 3l9 7.5V20a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/></svg>
            <span class="mi-label">Home Screen</span>
          </button>
        </div>
      {/if}
    </div>

    <!-- Home Button (House icon) -->
    <button
      class="icon-btn home-btn"
      title="Home (New session draft)"
      on:click={() => dispatch("home")}
    >
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
        <path d="M3 10.5L12 3l9 7.5V20a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
        <polyline points="9 22 9 12 15 12 15 22" />
      </svg>
    </button>

    <!-- Tab Bar -->
    <TabBar
      {tabs}
      {activeTabId}
      on:select={(e) => dispatch("selectTab", e.detail)}
      on:close={(e) => dispatch("closeTab", e.detail)}
      on:newTab={() => dispatch("newTab")}
    />
  </div>

  <!-- Right Cluster (Frameless Window Controls) -->
  <div class="right-cluster">
    <div class="window-controls">
      <button class="win-btn" title="Minimize" on:click={handleMin} tabindex="-1">
        <svg width="10" height="10" viewBox="0 0 10 10">
          <line x1="0" y1="5" x2="10" y2="5" stroke="currentColor" stroke-width="1" />
        </svg>
      </button>
      <button class="win-btn" title={isMaximized ? "Restore" : "Maximize"} on:click={handleMax} tabindex="-1">
        {#if isMaximized}
          <svg width="10" height="10" viewBox="0 0 10 10">
            <path d="M2.5 2V0.5h7V7.5H8M0.5 2.5h7v7h-7z" fill="none" stroke="currentColor" stroke-width="1" />
          </svg>
        {:else}
          <svg width="10" height="10" viewBox="0 0 10 10">
            <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1" />
          </svg>
        {/if}
      </button>
      <button class="win-btn close" title="Close" on:click={handleClose} tabindex="-1">
        <svg width="10" height="10" viewBox="0 0 10 10">
          <line x1="1" y1="1" x2="9" y2="9" stroke="currentColor" stroke-width="1.1" />
          <line x1="9" y1="1" x2="1" y2="9" stroke="currentColor" stroke-width="1.1" />
        </svg>
      </button>
    </div>
  </div>
</div>

<style>
  .topbar-shell {
    height: 38px;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    position: relative;
    z-index: 100;
    user-select: none;
    -webkit-app-region: drag;
    padding: 0 0 0 10px;
    box-sizing: border-box;
    background: #0d0d0f;
    border-bottom: none;
  }
  .left-cluster {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 100%;
    flex: 1;
    min-width: 0;
  }
  .menu-container {
    position: relative;
    -webkit-app-region: no-drag;
  }
  .icon-btn {
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 6px;
    color: #71717a;
    cursor: pointer;
    padding: 0;
    -webkit-app-region: no-drag;
    transition: background 0.1s ease, color 0.1s ease;
  }
  .icon-btn:hover,
  .icon-btn.active {
    background: #18181b;
    border-color: #27272a;
    color: #f4f4f5;
  }
  .right-cluster {
    display: flex;
    align-items: stretch;
    height: 100%;
    flex-shrink: 0;
    -webkit-app-region: no-drag;
  }
  .window-controls {
    display: flex;
    align-items: stretch;
    height: 100%;
    -webkit-app-region: no-drag;
  }
  .win-btn {
    width: 44px;
    height: 100%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 0;
    color: #a1a1aa;
    cursor: pointer;
    padding: 0;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .win-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #f4f4f5;
  }
  .win-btn.close:hover {
    background: #e81123;
    color: #ffffff;
  }
  .dropdown-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    width: 200px;
    background: #141416;
    border: 1px solid #27272a;
    border-radius: 8px;
    box-shadow: 0 10px 24px rgba(0, 0, 0, 0.6);
    padding: 4px;
    z-index: 500;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-radius: 5px;
    background: transparent;
    border: none;
    color: #e4e4e7;
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    text-align: left;
    width: 100%;
    box-sizing: border-box;
    transition: background 0.1s ease;
  }
  .menu-item:hover {
    background: #27272a;
  }
  .mi-label {
    flex: 1;
  }
  .mi-key {
    font-size: 10px;
    color: #71717a;
  }
  .menu-sep {
    height: 1px;
    background: #27272a;
    margin: 3px 0;
  }
</style>
