<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import TabBar from "./TabBar.svelte";
  import WinControls from "./WinControls.svelte";
  import NotificationDropdown from "./NotificationDropdown.svelte";
  import { notifications } from "./notificationStore";
  import type { Tab } from "./tabs";
  import { setOverlay } from "./overlay";
  import { updateVersion } from "./updateStore";
  import { startWindowDrag } from "./windowChrome";
  import { tooltip } from "./tooltip";

  export let tabs: Tab[] = [];
  export let activeTabId = "";

  const dispatch = createEventDispatcher<{
    select: { id: string };
    close: { id: string };
    move: { id: string; to: number };
    newTab: void;
    settings: void;
    search: void;
    update: void;
    brain: void;
    history: void;
    setup: void;
    report: void;
    openSession: { id: string };
  }>();

  let menuOpen = false;
  let notiOpen = false;

  $: setOverlay("topbar-menu", menuOpen || notiOpen);
  $: unreadCount = $notifications.filter((n) => !n.read).length;

  const MENU = [
    { label: "New session", key: "Ctrl+T", icon: "plus", run: () => dispatch("newTab") },
    { label: "Search", key: "Ctrl+K", icon: "search", run: () => dispatch("search") },
    { label: "History", key: "Ctrl+H", icon: "clock", run: () => dispatch("history") },
    { label: "Brain", key: "Ctrl+B", icon: "brain", run: () => dispatch("brain") },
    { label: "Settings", key: "Ctrl+,", icon: "settings", run: () => dispatch("settings") },
    { label: "Set up Parzi", key: "", icon: "spark", run: () => dispatch("setup") },
    { label: "Report a problem", key: "", icon: "alert", run: () => dispatch("report") },
  ] as const;

  function onDocClick(e: MouseEvent) {
    if (!(e.target as HTMLElement).closest(".menu")) menuOpen = false;
    if (!(e.target as HTMLElement).closest(".noti-wrap")) notiOpen = false;
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
      <button
        class="icon-btn"
        class:active={menuOpen}
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        use:tooltip={$updateVersion ? "Menu, update available" : "Menu"}
        on:click={() => (menuOpen = !menuOpen)}
      >
        <Icon name="menu" />
        {#if $updateVersion}<span class="dot" />{/if}
      </button>
      {#if menuOpen}
        <div class="dropdown" role="menu" data-ui-list>
          {#each MENU as item}
            <button
              class="menu-item"
              role="menuitem"
              on:click={() => {
                menuOpen = false;
                item.run();
              }}
            >
              <Icon name={item.icon} />
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
              <Icon name="arrowDown" />
              <span class="label">Update to v{$updateVersion}</span>
            </button>
          {/if}
        </div>
      {/if}
    </div>
    <TabBar
      {tabs}
      {activeTabId}
      on:select={(e) => dispatch("select", e.detail)}
      on:close={(e) => dispatch("close", e.detail)}
      on:move={(e) => dispatch("move", e.detail)}
      on:newTab={() => dispatch("newTab")}
    />
  </div>

  <div class="right">
    <div class="noti-wrap">
      <button
        class="icon-btn noti-btn"
        class:active={notiOpen}
        aria-haspopup="dialog"
        aria-expanded={notiOpen}
        use:tooltip={unreadCount ? `${unreadCount} unread notification${unreadCount === 1 ? "" : "s"}` : "Notifications"}
        on:click={() => (notiOpen = !notiOpen)}
      >
        <Icon name="bell" />
        {#if unreadCount > 0}
          <span class="noti-badge" />
        {/if}
      </button>
      {#if notiOpen}
        <NotificationDropdown
          on:openSession={(e) => {
            notiOpen = false;
            dispatch("openSession", e.detail);
          }}
          on:close={() => (notiOpen = false)}
        />
      {/if}
    </div>
    <WinControls />
  </div>
</header>

<style>
  /* 44px holds a 28px tab with 8px above it; the panels below start right
     under the bar, so the gap under the tab is the same 8px. */
  .topbar {
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    position: relative;
    z-index: 100;
    padding-left: 8px;
    user-select: none;
  }
  /* One 4px step between the menu, Home, the tabs and +. */
  .left {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 100%;
    flex: 1;
    min-width: 0;
  }
  .right {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 100%;
    flex: none;
    padding-right: 2px;
  }
  .noti-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }
  .noti-btn.active {
    color: var(--text);
    background: var(--hover);
  }
  /* Unread is a dot; the count is in the button's name and tooltip. */
  .noti-badge {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: var(--accent);
    border: 1.5px solid var(--bg);
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
    border: none;
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
  .icon-btn:hover {
    background: var(--hover);
    color: var(--text);
  }
  .icon-btn.active {
    background: var(--selected);
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
    font-size: var(--fs-sm);
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
    font-size: var(--fs-xs);
    color: var(--muted);
  }
</style>
