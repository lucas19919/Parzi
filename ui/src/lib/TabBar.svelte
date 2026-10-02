<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { Tab } from "./tabTypes";

  export let tabs: Tab[] = [];
  export let activeTabId: string | null = null;

  const dispatch = createEventDispatcher<{
    select: { id: string };
    close: { id: string };
    newTab: void;
  }>();

  function onTabClick(id: string) {
    dispatch("select", { id });
  }

  function onCloseClick(e: MouseEvent, id: string) {
    e.stopPropagation();
    dispatch("close", { id });
  }

  function onNewTabClick() {
    dispatch("newTab");
  }

  function getBadge(tab: Tab): { text: string; bg: string; color: string; dot?: boolean } {
    if (tab.badge) {
      return { text: tab.badge, bg: tab.badgeColor || "#2e2e34", color: "#f4f4f5" };
    }
    if (tab.kind === "browser") {
      return { text: "🌐", bg: "#1e293b", color: "#38bdf8" };
    }
    if (tab.title.toLowerCase().includes("work") || tab.sessionId?.startsWith("ws-")) {
      return { text: "W", bg: "#2e2e34", color: "#f4f4f5" };
    }
    return { text: "I", bg: "#78350f", color: "#fde68a", dot: true };
  }
</script>

<div class="tabs-container" role="tablist">
  <div class="tabs-scroll">
    {#each tabs as tab (tab.id)}
      {@const isActive = tab.id === activeTabId}
      {@const badge = getBadge(tab)}
      <button
        class="tab-chip"
        class:active={isActive}
        role="tab"
        aria-selected={isActive}
        title={tab.title}
        on:click={() => onTabClick(tab.id)}
      >
        <span class="tab-badge" style="background: {badge.bg}; color: {badge.color}">
          {badge.text}
          {#if badge.dot}
            <span class="badge-dot" />
          {/if}
        </span>
        <span class="tab-title">{tab.title || "New session"}</span>
        {#if tabs.length > 1}
          <button
            class="tab-close"
            title="Close tab"
            aria-label="Close tab"
            on:click={(e) => onCloseClick(e, tab.id)}
          >
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        {/if}
      </button>
    {/each}
  </div>

  <button class="new-tab-btn" title="New tab (Ctrl+T)" on:click={onNewTabClick}>
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round">
      <path d="M12 5v14M5 12h14" />
    </svg>
  </button>
</div>

<style>
  .tabs-container {
    display: flex;
    align-items: center;
    gap: 4px;
    max-width: calc(100vw - 320px);
    overflow: hidden;
    -webkit-app-region: no-drag;
  }
  .tabs-scroll {
    display: flex;
    align-items: center;
    gap: 4px;
    overflow-x: auto;
    scrollbar-width: none;
    -ms-overflow-style: none;
  }
  .tabs-scroll::-webkit-scrollbar {
    display: none;
  }
  .tab-chip {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 29px;
    padding: 0 9px 0 7px;
    border-radius: 8px;
    background: transparent;
    border: 1px solid transparent;
    color: #9ca3af;
    font-size: 12px;
    font-weight: 500;
    font-family: inherit;
    cursor: pointer;
    user-select: none;
    white-space: nowrap;
    max-width: 220px;
    transition: background 0.1s ease, border-color 0.1s ease, color 0.1s ease;
  }
  .tab-chip:hover {
    background: #18181b;
    color: #e4e4e7;
  }
  .tab-chip.active {
    background: #27272a;
    border-color: #3f3f46;
    color: #ffffff;
  }
  .tab-badge {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 17px;
    height: 17px;
    border-radius: 5px;
    font-size: 10px;
    font-weight: 700;
    flex-shrink: 0;
    line-height: 1;
  }
  .badge-dot {
    position: absolute;
    top: -2px;
    right: -2px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: #2dd4bf;
    border: 1px solid #0d0d0f;
  }
  .tab-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tab-close {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 4px;
    background: transparent;
    border: none;
    color: #71717a;
    cursor: pointer;
    padding: 0;
    margin-left: 2px;
    transition: background 0.1s ease, color 0.1s ease;
  }
  .tab-chip:hover .tab-close {
    color: #a1a1aa;
  }
  .tab-close:hover {
    background: #3f3f46;
    color: #ffffff;
  }
  .new-tab-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: 6px;
    background: transparent;
    border: 1px solid transparent;
    color: #71717a;
    cursor: pointer;
    padding: 0;
    flex-shrink: 0;
    transition: background 0.1s ease, color 0.1s ease;
  }
  .new-tab-btn:hover {
    background: #18181b;
    border-color: #27272a;
    color: #f4f4f5;
  }
</style>
