<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import TabBar from "./TabBar.svelte";
  import WinControls from "./WinControls.svelte";
  import type { Tab } from "./tabs";
  import { installUpdate, updateState, updateVersion } from "./updateStore";
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
    manager: void;
    settings: void;
    panel: void;
  }>();

</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="chrome">
<header class="topbar" on:mousedown={startWindowDrag}>
  <div class="left">
      <button
        class="icon-btn"
        aria-label="Settings"
        use:tooltip={"Settings (Ctrl+,)"}
        on:click={() => dispatch("settings")}
      >
        <Icon name="menu" />
      </button>
      <button
        class="icon-btn manager-btn"
        aria-label="Session manager"
        use:tooltip={"Sessions"}
        on:click={() => dispatch("manager")}
      >
        <Icon name="chat" />
      </button>
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
      {#if $updateState === "ready"}
        <button
          class="update"
          aria-label={`Update to ${$updateVersion} and restart Parzi`}
          use:tooltip={`Update to ${$updateVersion} and restart`}
          on:click={installUpdate}
        >
          <Icon name="arrowUp" />
        </button>
      {/if}
      {#if panelable}
        <button
          class="icon-btn panel-btn flip"
          class:active={panelOpen}
          aria-pressed={panelOpen}
          aria-label={panelCount > 0 ? `Side bar, ${panelCount} agents running` : "Side bar"}
          use:tooltip={"Side bar (Ctrl+J)"}
          on:click={() => dispatch("panel")}
        >
        <Icon name="panel" />
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
  .panel-btn.flip :global(svg) {
    transform: scaleX(-1);
  }
  .left {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
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
  /* The composer's send button, ready to send. */
  .update {
    width: calc(var(--space) * 6);
    height: calc(var(--space) * 6);
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: var(--accent);
    border: none;
    border-radius: 50%;
    color: var(--bg);
    cursor: pointer;
  }
  .update:active {
    transform: scale(0.94);
    transition-duration: var(--dur-fast);
  }
  .icon-btn:hover,
  .icon-btn.active {
    background: var(--panel);
    color: var(--text);
  }
  .panel-btn {
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
  }
  .panel-btn:hover {
    background: transparent;
    border-color: transparent;
    color: var(--text);
  }
  .panel-btn.active {
    background: color-mix(in srgb, var(--panel) var(--card-fill), transparent);
    border-color: color-mix(in srgb, var(--line) var(--card-edge), transparent);
    color: var(--text);
  }
</style>
