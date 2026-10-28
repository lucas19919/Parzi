<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";
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
  export let threads: { id: string; model: string }[] = [];
  // The recent sessions column: offered on session tabs, and whether it shows.
  export let railable = false;
  export let railOpen = false;
  // The session column: offered on session tabs, and whether it shows.
  export let panelable = false;
  export let panelOpen = false;
  export let panelCount = 0;

  const dispatch = createEventDispatcher<{
    select: { id: string };
    close: { id: string };
    move: { id: string; to: number };
    newTab: void;
    settings: void;
    update: void;
    brain: void;
    history: void;
    setup: void;
    report: void;
    openSession: { id: string };
    rail: void;
    panel: void;
  }>();

  $: panelAgents = panelCount > 99 ? "99+" : String(panelCount);

  let menuOpen = false;
  let notiOpen = false;

  $: setOverlay("topbar-menu", menuOpen || notiOpen);
  $: unreadCount = $notifications.filter((n) => !n.read).length;

  const MENU: { label: string; key: string; icon: IconName; img?: string; run: () => void }[] = [
    { label: "New session", key: "Ctrl+T", icon: "plus", run: () => dispatch("newTab") },
    { label: "History", key: "Ctrl+H", icon: "clock", run: () => dispatch("history") },
    { label: "Brain", key: "Ctrl+B", icon: "brain", run: () => dispatch("brain") },
    { label: "Settings", key: "Ctrl+,", icon: "settings", run: () => dispatch("settings") },
    { label: "Set up Parzi", key: "", icon: "spark", img: "/favicon.svg", run: () => dispatch("setup") },
    { label: "Report a problem", key: "", icon: "alert", run: () => dispatch("report") },
  ];

  function onDocClick(e: MouseEvent) {
    if (!(e.target as HTMLElement).closest(".menuzone")) menuOpen = false;
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
<div class="chrome">
<header class="topbar" on:mousedown={startWindowDrag}>
  <div class="left">
    <div class="menuzone">
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
    </div>
    {#if railable}
      <button
        class="icon-btn rail-btn"
        class:active={railOpen}
        aria-pressed={railOpen}
        use:tooltip={railOpen ? "Hide sessions (Ctrl+Shift+S)" : "Sessions by project (Ctrl+Shift+S)"}
        on:click={() => dispatch("rail")}
      >
        <Icon name="panel" />
      </button>
    {/if}
      <TabBar
        {tabs}
        {activeTabId}
        {threads}
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
      {#if panelable}
        <button
          class="icon-btn"
          class:active={panelOpen}
          aria-pressed={panelOpen}
          aria-label={panelCount > 0 ? `Session column, ${panelAgents} agents running` : "Session column"}
          use:tooltip={"Session column (Ctrl+J)"}
          on:click={() => dispatch("panel")}
        >
          <Icon name="panel" />
          {#if panelCount > 0}<span class="count">{panelAgents}</span>{/if}
        </button>
      {/if}
      <button class="icon-btn" use:tooltip={"Settings (Ctrl+,)"} aria-label="Settings" on:click={() => dispatch("settings")}>
        <Icon name="settings" />
      </button>
      <WinControls />
  </div>
</header>
{#if menuOpen}
  <!-- The menu expands into the bar as its own row, never a floating popout. -->
  <div class="menuzone menurow" role="menu" data-ui-list>
    {#each MENU as item}
      <button
        class="opt"
        role="menuitem"
        on:click={() => {
          menuOpen = false;
          item.run();
        }}
      >
        {#if item.img}<img class="fav" src={item.img} alt="" />{:else}<Icon name={item.icon} />{/if}
        <span class="label">{item.label}</span>
        {#if item.key}<span class="key">{item.key}</span>{/if}
      </button>
    {/each}
    {#if $updateVersion}
      <button
        class="opt update"
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

<style>
  .chrome {
    flex: none;
    display: flex;
    flex-direction: column;
  }
  /* 44px holds a 28px tab with 8px above it; the panels below start right
     under the bar, so the gap under the tab is the same 8px. */
  .topbar {
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    position: relative;
    z-index: 100;
    padding-left: var(--gap);
    user-select: none;
  }
  /* One 4px step between the menu, Home, the tabs and +. */
  /* The session column's icon, mirrored: this one opens on the left. */
  .rail-btn :global(svg) {
    transform: scaleX(-1);
  }
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
  .icon-btn:hover {
    background: var(--hover);
    color: var(--text);
  }
  .icon-btn.active {
    background: var(--selected);
    color: var(--text);
  }
  .count {
    position: absolute;
    top: 0;
    right: -1px;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    border-radius: 999px;
    background: var(--ok);
    color: var(--on-ok);
    font-size: var(--fs-xs);
    font-weight: 700;
    line-height: 14px;
    text-align: center;
  }
  /* The menu expands into the bar as its own row: one quiet strip of
     options, never a floating popout. */
  .menuzone.menurow {
    display: flex;
    align-items: center;
    gap: 2px;
    overflow-x: auto;
    padding: 4px var(--gap);
    border-bottom: 1px solid var(--line);
  }
  .opt {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex: none;
    padding: 5px 10px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    white-space: nowrap;
    cursor: pointer;
  }
  .opt:hover {
    background: var(--hover);
    color: var(--text);
  }
  .opt.update {
    color: var(--accent);
  }
  .opt .label {
    flex: none;
  }
  .opt .key {
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .fav {
    width: 16px;
    height: 16px;
    flex: none;
    border-radius: 4px;
  }
</style>
