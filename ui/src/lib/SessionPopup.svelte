<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { fade, fly } from "svelte/transition";
  import type { SessionMeta } from "./api";
  import type { Tab } from "./tabTypes";

  export let open = false;
  export let threads: SessionMeta[] = [];
  export let tabs: Tab[] = [];
  export let activeTabId: string | null = null;
  export let workspaces: string[] = [];
  export let currentWorkspace = "";

  let query = "";
  let selectedIndex = 0;
  let inputEl: HTMLInputElement | null = null;

  const dispatch = createEventDispatcher<{
    close: void;
    selectTab: { id: string };
    openSession: { id: string; inNewTab?: boolean };
    deleteSession: { id: string };
    selectWorkspace: { name: string };
    newSession: void;
    newBrowserTab: void;
    openSettings: void;
  }>();

  $: q = query.trim().toLowerCase();

  interface Item {
    id: string;
    section: "tabs" | "sessions" | "workspaces" | "commands";
    title: string;
    sub?: string;
    badge?: string;
    badgeBg?: string;
    badgeColor?: string;
    onSelect: () => void;
    onDelete?: () => void;
  }

  $: {
    const items: Item[] = [];

    // Commands (always first if query matches or empty)
    const cmds = [
      { id: "cmd-new", title: "New Session", sub: "Start fresh agent harness draft", onSelect: () => dispatch("newSession") },
      { id: "cmd-web", title: "New Browser Tab", sub: "Open web browser viewport", onSelect: () => dispatch("newBrowserTab") },
      { id: "cmd-settings", title: "Settings", sub: "Open settings and providers", onSelect: () => dispatch("openSettings") },
    ];
    for (const c of cmds) {
      if (!q || c.title.toLowerCase().includes(q) || c.sub.toLowerCase().includes(q)) {
        items.push({ ...c, section: "commands", badge: ">", badgeBg: "rgba(124, 140, 255, 0.15)", badgeColor: "#7c8cff" });
      }
    }

    // Open tabs
    for (const tab of tabs) {
      if (!q || tab.title.toLowerCase().includes(q) || (tab.url && tab.url.toLowerCase().includes(q))) {
        items.push({
          id: `tab-${tab.id}`,
          section: "tabs",
          title: tab.title || "Untitled Tab",
          sub: tab.id === activeTabId ? "Active tab" : "Switch to tab",
          badge: tab.badge && tab.badge !== "🌐" ? tab.badge : (tab.kind === "browser" ? "B" : "T"),
          badgeBg: tab.badgeColor || "rgba(255, 255, 255, 0.1)",
          badgeColor: "#ffffff",
          onSelect: () => dispatch("selectTab", { id: tab.id }),
        });
      }
    }

    // Workspaces
    for (const ws of workspaces) {
      if (!q || ws.toLowerCase().includes(q)) {
        items.push({
          id: `ws-${ws}`,
          section: "workspaces",
          title: ws,
          sub: ws === currentWorkspace ? "Current workspace" : "Switch workspace",
          badge: "W",
          badgeBg: "rgba(59, 130, 246, 0.15)",
          badgeColor: "#60a5fa",
          onSelect: () => dispatch("selectWorkspace", { name: ws }),
        });
      }
    }

    // Sessions
    for (const t of threads) {
      if (!q || t.title.toLowerCase().includes(q) || (t.lane && t.lane.toLowerCase().includes(q)) || (t.model && t.model.toLowerCase().includes(q))) {
        const tokens = (t.tokens_in || 0) + (t.tokens_out || 0);
        const sub = `${t.model || "auto"} · ${tokens > 0 ? `${Math.round(tokens / 1000)}k tokens` : "0 tokens"}`;
        items.push({
          id: `sess-${t.id}`,
          section: "sessions",
          title: t.title || "Untitled session",
          sub,
          badge: t.lane ? t.lane.slice(0, 1).toUpperCase() : "S",
          badgeBg: "rgba(245, 158, 11, 0.15)",
          badgeColor: "#f59e0b",
          onSelect: () => dispatch("openSession", { id: t.id }),
          onDelete: () => dispatch("deleteSession", { id: t.id }),
        });
      }
    }

    filteredItems = items;
    if (selectedIndex >= filteredItems.length) selectedIndex = 0;
  }

  let filteredItems: Item[] = [];

  $: if (open) {
    query = "";
    selectedIndex = 0;
    setTimeout(() => {
      inputEl?.focus();
    }, 40);
  }

  function onKeyDown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      dispatch("close");
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIndex = (selectedIndex + 1) % Math.max(1, filteredItems.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIndex = (selectedIndex - 1 + filteredItems.length) % Math.max(1, filteredItems.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = filteredItems[selectedIndex];
      if (item) {
        item.onSelect();
        dispatch("close");
      }
    }
  }

  function onItemClick(item: Item) {
    item.onSelect();
    dispatch("close");
  }

  function onDeleteClick(e: MouseEvent, item: Item) {
    e.stopPropagation();
    if (item.onDelete) {
      item.onDelete();
    }
  }
</script>

<svelte:window on:keydown={onKeyDown} />

{#if open}
  <div class="popup-backdrop" transition:fade={{ duration: 120 }} on:click={() => dispatch("close")}>
    <div class="popup-box" transition:fly={{ y: -10, duration: 160 }} on:click|stopPropagation>
      <div class="search-head">
        <svg class="search-icon" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
          <circle cx="11" cy="11" r="8" />
          <line x1="21" y1="21" x2="16.65" y2="16.65" />
        </svg>
        <input
          bind:this={inputEl}
          bind:value={query}
          placeholder="Search sessions, tabs, workspaces or commands…"
          class="search-input"
        />
        <kbd class="esc-kbd">esc</kbd>
      </div>

      <div class="list-body">
        {#if filteredItems.length === 0}
          <div class="empty-state">No matching sessions or commands</div>
        {:else}
          {#each filteredItems as item, idx (item.id)}
            {@const isSelected = idx === selectedIndex}
            <div
              class="item-row"
              class:selected={isSelected}
              on:click={() => onItemClick(item)}
              on:mouseenter={() => (selectedIndex = idx)}
            >
              <span class="item-badge" style="background: {item.badgeBg}; color: {item.badgeColor}">
                {item.badge}
              </span>
              <div class="item-info">
                <div class="item-title">{item.title}</div>
                {#if item.sub}<div class="item-sub">{item.sub}</div>{/if}
              </div>
              {#if item.onDelete}
                <button
                  class="del-btn"
                  title="Delete session"
                  on:click={(e) => onDeleteClick(e, item)}
                >
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                    <path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                  </svg>
                </button>
              {/if}
            </div>
          {/each}
        {/if}
      </div>

      <div class="footer-hints">
        <span><kbd>↑</kbd><kbd>↓</kbd> Navigate</span>
        <span><kbd>↵</kbd> Select</span>
        <span><kbd>esc</kbd> Dismiss</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .popup-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    justify-content: center;
    padding-top: 14vh;
  }
  .popup-box {
    width: min(580px, 92vw);
    max-height: 480px;
    background: #18181b;
    border: 1px solid #27272a;
    border-radius: 12px;
    box-shadow: 0 24px 50px rgba(0, 0, 0, 0.6);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .search-head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--line-3, rgba(255, 255, 255, 0.08));
  }
  .search-icon {
    color: var(--text-3);
    flex-shrink: 0;
  }
  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text, #ffffff);
    font-size: 14px;
    font-family: inherit;
  }
  .search-input::placeholder {
    color: var(--text-3, rgba(255, 255, 255, 0.4));
  }
  .esc-kbd {
    font-size: 10px;
    color: var(--text-3);
    background: var(--surface-2, rgba(255, 255, 255, 0.06));
    border: 1px solid var(--line-3, rgba(255, 255, 255, 0.08));
    border-radius: 4px;
    padding: 2px 6px;
  }
  .list-body {
    flex: 1;
    overflow-y: auto;
    padding: 6px;
    max-height: 360px;
  }
  .empty-state {
    padding: 32px 16px;
    text-align: center;
    color: var(--text-3);
    font-size: 13px;
  }
  .item-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 7px;
    cursor: pointer;
    transition: background 0.1s ease;
  }
  .item-row:hover,
  .item-row.selected {
    background: var(--surface-2, rgba(255, 255, 255, 0.08));
  }
  .item-badge {
    width: 20px;
    height: 20px;
    border-radius: 5px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 700;
    flex-shrink: 0;
  }
  .item-info {
    flex: 1;
    min-width: 0;
  }
  .item-title {
    font-size: 12.5px;
    font-weight: 500;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .item-sub {
    font-size: 11px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-top: 1px;
  }
  .del-btn {
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-3);
    cursor: pointer;
    opacity: 0.5;
    transition: opacity 0.1s ease, color 0.1s ease, background 0.1s ease;
  }
  .del-btn:hover {
    opacity: 1;
    color: #ef4444;
    background: rgba(239, 68, 68, 0.15);
  }
  .footer-hints {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px 16px;
    border-top: 1px solid var(--line-3, rgba(255, 255, 255, 0.06));
    background: rgba(0, 0, 0, 0.2);
    font-size: 10.5px;
    color: var(--text-3);
  }
  .footer-hints kbd {
    background: var(--surface-2, rgba(255, 255, 255, 0.06));
    border: 1px solid var(--line-3, rgba(255, 255, 255, 0.08));
    border-radius: 3px;
    padding: 1px 4px;
    margin-right: 3px;
  }
</style>
