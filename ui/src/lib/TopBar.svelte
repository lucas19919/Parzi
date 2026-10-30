<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import ChromeIcon from "./ChromeIcon.svelte";
  import type { ChromeIconName } from "./ChromeIcon.svelte";
  import TabBar from "./TabBar.svelte";
  import WinControls from "./WinControls.svelte";
  import type { Tab } from "./tabs";
  import { updateVersion } from "./updateStore";
  import { startWindowDrag } from "./windowChrome";
  import { tooltip } from "./tooltip";

  export let tabs: Tab[] = [];
  export let activeTabId = "";
  export let threads: { id: string; model: string }[] = [];
  export let liveModel = "";
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
    history: void;
    setup: void;
    report: void;
    panel: void;
  }>();

  $: panelAgents = panelCount > 99 ? "99+" : String(panelCount);

  let menuOpen = false;

  const MENU: { label: string; key: string; icon: ChromeIconName; run: () => void }[] = [
    { label: "History", key: "Ctrl+H", icon: "clock", run: () => dispatch("history") },
    { label: "Settings", key: "Ctrl+,", icon: "settings", run: () => dispatch("settings") },
    { label: "Set up Parzi", key: "", icon: "wrench", run: () => dispatch("setup") },
    { label: "Report a problem", key: "", icon: "alert", run: () => dispatch("report") },
  ];

</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="chrome">
<header class="topbar" on:mousedown={startWindowDrag}>
  <div class="left">
    
    <div class="menu-pill" class:open={menuOpen}>
      <button
        class="icon-btn"
        class:active={menuOpen}
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        use:tooltip={$updateVersion ? "Menu, update available" : "Menu"}
        on:click={() => (menuOpen = !menuOpen)}
      >
        <ChromeIcon name="menu" />
        {#if $updateVersion}<span class="dot" />{/if}
      </button>
      {#if menuOpen}
        <div class="opts" role="menu" data-ui-list>
          {#each MENU as item, i}
            <button
              class="opt"
              style="--i: {i}"
              role="menuitem"
              aria-label={item.key ? `${item.label} (${item.key})` : item.label}
              use:tooltip={item.key ? `${item.label} (${item.key})` : item.label}
              on:click={() => item.run()}
            >
              <ChromeIcon name={item.icon} />
            </button>
          {/each}
          {#if $updateVersion}
            <button
              class="opt update"
              style="--i: {MENU.length}"
              role="menuitem"
              aria-label="Update to v{$updateVersion}"
              use:tooltip={"Update to v" + $updateVersion}
              on:click={() => dispatch("update")}
            >
              <ChromeIcon name="arrowDown" />
            </button>
          {/if}
        </div>
      {/if}
    </div>
      <TabBar
        {tabs}
        {activeTabId}
        {threads}
        {liveModel}
        on:select={(e) => dispatch("select", e.detail)}
        on:close={(e) => dispatch("close", e.detail)}
        on:move={(e) => dispatch("move", e.detail)}
        on:newTab={() => dispatch("newTab")}
      />
  </div>

  <div class="right">
      {#if panelable}
        <button
          class="icon-btn panel-btn"
          class:active={panelOpen}
          aria-pressed={panelOpen}
          aria-label={panelCount > 0 ? `Session column, ${panelAgents} agents running` : "Session column"}
          use:tooltip={"Session column (Ctrl+J)"}
          on:click={() => dispatch("panel")}
        >
          <ChromeIcon name="panel" />
          {#if panelCount > 0}<span class="count">{panelAgents}</span>{/if}
        </button>
      {/if}
      <WinControls />
  </div>
</header>
</div>

<style>
  .chrome {
    flex: none;
    display: flex;
    flex-direction: column;
  }

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
  .panel-btn :global(svg) {
    transform: scaleX(-1);
  }
  .left {
    display: flex;
    align-items: center;
    gap: var(--space);
    height: 100%;
    flex: 1;
    min-width: 0;
  }
  .right {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    height: 100%;
    flex: none;
    padding-right: var(--space);
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
  .icon-btn:hover,
  .icon-btn.active {
    background: var(--panel);
    color: var(--text);
  }
  .panel-btn {
    background: transparent;
    border: 1px solid var(--line);
    border-radius: 999px;
  }
  .panel-btn:hover,
  .panel-btn.active {
    background: transparent;
    border: 1px solid var(--line);
    color: var(--text);
  }
  .count {
    position: absolute;
    top: 0;
    right: -1px;
    min-width: 14px;
    height: 14px;
    padding: 0 var(--space);
    border-radius: 999px;
    background: var(--ok);
    color: var(--on-ok);
    font-size: var(--fs-xs);
    font-weight: 700;
    line-height: 14px;
    text-align: center;
  }
  .menu-pill {
    display: flex;
    align-items: center;
    flex: none;
    height: 28px;
    background: none;
    border: 1px solid transparent;
  }
  .menu-pill.open {
    background: transparent;
    border: 1px solid var(--line);
    border-radius: 999px;
  }
  .menu-pill.open .icon-btn,
  .menu-pill.open .icon-btn.active {
    background: transparent;
    color: var(--muted);
  }
  .menu-pill .icon-btn:hover,
  .menu-pill.open .icon-btn:hover {
    background: transparent;
    color: var(--text);
  }
  .opts {
    display: flex;
    align-items: center;
    flex: none;
    height: 28px;
    background: none;
    border: none;
    transform-origin: left center;
    animation: menu-in 200ms var(--ease-out);
  }
  @keyframes menu-in {
    from {
      opacity: 0;
      transform: translateX(-10px);
    }
  }
  .opt {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 28px;
    height: 28px;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    animation: opt-in 180ms var(--ease-out) backwards;
    animation-delay: calc(var(--i, 0) * 30ms);
  }
  @keyframes opt-in {
    from {
      opacity: 0;
      transform: translateX(-6px) scale(0.92);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .opt,
    .opts {
      animation: none;
    }
  }
  .opt:hover {
    background: transparent;
    color: var(--text);
  }
  .opt.update {
    color: var(--text);
  }
</style>
