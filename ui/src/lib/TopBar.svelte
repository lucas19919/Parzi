<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import TabBar from "./TabBar.svelte";
  import type { Tab } from "./tabTypes";
  import Icon from "./Icon.svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { WIN_ICON, startWindowDrag, windowClose, windowMaximize, windowMinimize } from "./windowChrome";

  export let tabs: Tab[] = [];
  export let activeTabId: string | null = null;
  export let hasUpdate = false;

  let menuOpen = false;

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

  const I = WIN_ICON;

  async function syncChrome() {
    try {
      const w = getCurrentWindow();
      const full = await w.isFullscreen().catch(() => false);
      const max = full ? false : await w.isMaximized().catch(() => false);
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
            <span class="mi-icon">＋</span>
            <span class="mi-label">New Session</span>
            <span class="mi-key">Ctrl+T</span>
          </button>
          <button class="menu-item" on:click={() => { menuOpen = false; dispatch("openSessionPopup"); }}>
            <span class="mi-icon">🔍</span>
            <span class="mi-label">Switch Session…</span>
            <span class="mi-key">Ctrl+P</span>
          </button>
          <div class="menu-sep" />
          <button class="menu-item" on:click={() => { menuOpen = false; dispatch("home"); }}>
            <span class="mi-icon">⊞</span>
            <span class="mi-label">Home</span>
          </button>
          <button class="menu-item" on:click={() => { menuOpen = false; dispatch("openSettings"); }}>
            <span class="mi-icon">⚙</span>
            <span class="mi-label">Settings</span>
            <span class="mi-key">Ctrl+,</span>
          </button>
          <div class="menu-sep" />
          <button class="menu-item" on:click={() => { menuOpen = false; dispatch("checkUpdates"); }}>
            <span class="mi-icon">🔄</span>
            <span class="mi-label">Check for Updates</span>
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

  <!-- Right Cluster (Exact OpenCode items: Download/Update, Panel toggle, Window controls) -->
  <div class="right-cluster">
    <!-- Download / Update Status Icon -->
    <button
      class="icon-btn download-btn"
      title={hasUpdate ? "Update available" : "Updates and downloads"}
      on:click={() => dispatch("checkUpdates")}
    >
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
        <circle cx="12" cy="12" r="9.5" stroke="#166534" fill="#052e16" />
        <path d="M12 7.5v9M8.5 13l3.5 3.5 3.5-3.5" stroke="#22c55e" stroke-width="2.2" />
      </svg>
    </button>

    <!-- Panel / Sidebar toggle button (OpenCode icon) -->
    <button
      class="icon-btn panel-btn"
      title="Toggle panel"
      on:click={() => dispatch("togglePanel")}
    >
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
        <rect x="3" y="3" width="18" height="18" rx="3" stroke="#71717a" />
        <path d="M15 3v18" stroke="#71717a" />
      </svg>
    </button>

    <!-- Window controls -->
    <div class="window-controls">
      <button class="win-btn" title="Minimize" on:click={handleMin} tabindex="-1">
        <Icon d={I.min} size={11} />
      </button>
      <button class="win-btn" title="Maximize / Restore" on:click={handleMax} tabindex="-1">
        <Icon d={I.max} size={11} />
      </button>
      <button class="win-btn close" title="Close" on:click={handleClose} tabindex="-1">
        <Icon d={I.close} size={11} />
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
    padding: 0 10px 0 10px;
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
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
    -webkit-app-region: no-drag;
  }
  .window-controls {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: 6px;
    -webkit-app-region: no-drag;
  }
  .win-btn {
    width: 28px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: #71717a;
    cursor: pointer;
    transition: background 0.1s ease, color 0.1s ease;
  }
  .win-btn:hover {
    background: #27272a;
    color: #f4f4f5;
  }
  .win-btn.close:hover {
    background: #ef4444;
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
  .mi-icon {
    width: 16px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    color: #a1a1aa;
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
