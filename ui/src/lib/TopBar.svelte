<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import TabBar from "./TabBar.svelte";
  import WinControls from "./WinControls.svelte";
  import type { Tab } from "./tabs";
  import { setOverlay } from "./overlay";
  import { updateVersion } from "./updateStore";
  import { startWindowDrag } from "./windowChrome";

  export let tabs: Tab[] = [];
  export let activeTabId = "";
  export let activeTitle = "";
  export let sessionLanes: Record<string, string> = {};
  export let tabsVisible = true;

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

  let menuOpen = false;

  $: setOverlay("topbar-menu", menuOpen);

  const MENU = [
    { label: "New tab", key: "Ctrl+T", icon: "plus", run: () => dispatch("newTab") },
    { label: "Search", key: "Ctrl+P", icon: "search", run: () => dispatch("search") },
    { label: "History", key: "Ctrl+H", icon: "clock", run: () => dispatch("history") },
    { label: "Brain", key: "Ctrl+B", icon: "brain", run: () => dispatch("brain") },
    { label: "Settings", key: "Ctrl+,", icon: "settings", run: () => dispatch("settings") },
    { label: "Set up Parzi", key: "", icon: "spark", run: () => dispatch("setup") },
  ] as const;

  function onDocClick(e: MouseEvent) {
    if (!(e.target as HTMLElement).closest(".menu")) menuOpen = false;
  }

  onMount(() => {
    document.addEventListener("click", onDocClick);
    return () => {
      document.removeEventListener("click", onDocClick);
      setOverlay("topbar-menu", false);
    };
  });
</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
<header class="topbar" on:mousedown={startWindowDrag}>
  <div class="left">
    <div class="menu">
      <button class="icon-btn" class:active={menuOpen} title="Menu" on:click={() => (menuOpen = !menuOpen)}>
        <Icon name="menu" />
        {#if $updateVersion}<span class="dot" />{/if}
      </button>
      {#if menuOpen}
        <div class="dropdown" role="menu">
          {#each MENU as item}
            <button
              class="menu-item"
              role="menuitem"
              on:click={() => {
                menuOpen = false;
                item.run();
              }}
            >
              <Icon name={item.icon} size={13} />
              <span class="label">{item.label}</span>
              <span class="key">{item.key}</span>
            </button>
          {/each}
          {#if $updateVersion}
            <button
              class="menu-item update"
              role="menuitem"
              on:click={() => {
                menuOpen = false;
                dispatch("update");
              }}
            >
              <Icon name="arrowDown" size={13} />
              <span class="label">Update to v{$updateVersion}</span>
            </button>
          {/if}
        </div>
      {/if}
    </div>
    <button class="icon-btn" title="Home" on:click={() => dispatch("home")}>
      <Icon name="home" />
    </button>
    {#if tabsVisible}
      <TabBar
        {tabs}
        {activeTabId}
        {sessionLanes}
        on:select={(e) => dispatch("select", e.detail)}
        on:close={(e) => dispatch("close", e.detail)}
        on:move={(e) => dispatch("move", e.detail)}
        on:newTab={() => dispatch("newTab")}
      />
    {:else}
      <span class="top-title">{activeTitle || "New session"}</span>
    {/if}
  </div>

  <WinControls />
</header>

<style>
  .topbar {
    height: 38px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    position: relative;
    z-index: 100;
    padding-left: 10px;
    user-select: none;
  }
  .left {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 100%;
    flex: 1;
    min-width: 0;
  }
  .top-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: center;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--muted);
    user-select: none;
    pointer-events: none;
  }
  .menu {
    position: relative;
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
  .dot {
    position: absolute;
    top: 5px;
    right: 5px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }
  .menu-item.update {
    color: var(--accent);
  }
  .icon-btn:hover,
  .icon-btn.active {
    background: var(--panel);
    border-color: var(--line);
    color: var(--text);
  }
  .dropdown {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    width: 200px;
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    z-index: 500;
  }
  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .menu-item:hover {
    background: var(--line);
  }
  .label {
    flex: 1;
  }
  .key {
    font-size: 10px;
    color: var(--faint);
  }
</style>
