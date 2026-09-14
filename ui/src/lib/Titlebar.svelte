<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { api } from "./api";
  import Icon from "./Icon.svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  export let title = "Parzi";
  export let subtitle = "";
  /** Sidebar is collapsed: show an inline expand button before the crumb. */
  export let showExpand = false;
  /** Right inspector deck state (Docs / Agents pills + collapse toggle). */
  export let panelOpen = false;
  export let panelTab: "docs" | "agents" = "docs";
  export let docsLoaded = false;
  export let agentCount = 0;
  export let agentLive = 0;

  const dispatch = createEventDispatcher<{
    expand: void;
    toggleDocs: void;
    toggleAgents: void;
    togglePanel: void;
  }>();

  const lastPillTap: Record<string, number> = {};
  /**
   * First click toggles; a second click within 300ms is swallowed, so a
   * firm double-click on the active pill closes the deck instead of
   * closing it and instantly reopening it.
   */
  function pillTap(which: "docs" | "agents", run: () => void) {
    const now = Date.now();
    if (now - (lastPillTap[which] ?? 0) < 300) return;
    lastPillTap[which] = now;
    run();
  }

  const I = {
    min: "M5 12h14",
    max: "M5 5h14v14H5z",
    close: "M18 6L6 18M6 6l12 12",
    doc: "M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9zM14 3v6h6",
    bolt: "M13 2L4 14h7l-1 8 9-12h-7l1-8z",
    // Panel-right glyph: frame with a filled-in right column when open.
    panel: "M4 5h16v14H4zM15 5v14",
  };

  /** Fullscreen/maximized must drop the rounded root corners (black wedges). */
  async function syncChrome() {
    try {
      const w = getCurrentWindow();
      const full = await w.isFullscreen().catch(() => false);
      const max = full ? false : await w.isMaximized().catch(() => false);
      document.documentElement.classList.toggle("is-full", full || max);
    } catch {}
  }

  onMount(() => {
    syncChrome();
    window.addEventListener("resize", syncChrome);
    return () => window.removeEventListener("resize", syncChrome);
  });

  function onMouseDown(e: MouseEvent) {
    // Only trigger drag on primary mouse button and not on interactive buttons
    if (e.button === 0 && !(e.target as HTMLElement).closest("button")) {
      api.windowStartDragging().catch(() => {
        try {
          getCurrentWindow().startDragging();
        } catch {}
      });
    }
  }

  async function handleMin() {
    try {
      await api.windowMinimize();
    } catch {
      await getCurrentWindow().minimize();
    }
  }

  async function handleMax() {
    try {
      await api.windowMaximize();
    } catch {
      await getCurrentWindow().toggleMaximize();
    }
  }

  async function handleClose() {
    try {
      await api.windowClose();
    } catch {
      await getCurrentWindow().close();
    }
  }
</script>

<div class="parzi-titlebar" data-tauri-drag-region on:mousedown={onMouseDown}>
  <div class="drag-zone" data-tauri-drag-region>
    {#if showExpand}
      <button class="expand-btn" title="Show sidebar (Ctrl+B)" on:click={() => dispatch("expand")}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M4 6h16M4 12h16M4 18h16" /></svg>
      </button>
    {/if}
    <span class="title-crumb">
      {#if title !== "Settings"}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1 2 2H5a2 2 0 0 1-2-2z" /></svg>
      {/if}
      {#key title + "/" + subtitle}
        <span class="crumb-swap" in:fade={{ duration: 160 }}><b>{title}</b>{#if subtitle}<span class="sep">/</span><span class="sub">{subtitle}</span>{/if}</span>
      {/key}
    </span>
  </div>
  <div class="deck-controls">
    {#if !panelOpen}
    <button class="deck-pill" class:on={panelOpen && panelTab === "docs"} title="Docs — artifacts and markdown (Ctrl+\)"
      aria-pressed={panelOpen && panelTab === "docs"} on:click={() => pillTap("docs", () => dispatch("toggleDocs"))}>
      <Icon d={I.doc} size={12} />
      <span>Docs</span>
      {#if docsLoaded}<span class="deck-ind" />{/if}
    </button>
    <button class="deck-pill" class:on={panelOpen && panelTab === "agents"} title="Agents — swarm, processes, tool trace"
      aria-pressed={panelOpen && panelTab === "agents"} on:click={() => pillTap("agents", () => dispatch("toggleAgents"))}>
      <Icon d={I.bolt} size={12} />
      <span>Agents</span>
      {#if agentLive > 0}
        <span class="deck-ind live" />
        <span class="deck-count live">{agentLive}</span>
      {:else if agentCount > 1}
        <span class="deck-count">{agentCount}</span>
      {/if}
    </button>
    {/if}
    <button class="deck-toggle" class:on={panelOpen} title={panelOpen ? "Collapse inspector (Ctrl+\\)" : "Expand inspector (Ctrl+\\)"}
      aria-pressed={panelOpen} on:click={() => dispatch("togglePanel")}>
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="4" y="5" width="16" height="14" rx="2" />
        <path d="M15 5v14" />
        {#if panelOpen}<rect x="15" y="5" width="5" height="14" rx="1" fill="currentColor" stroke="none" opacity="0.55" />{/if}
      </svg>
    </button>
  </div>
  <div class="window-controls">
    <button class="win-btn" title="Minimize" on:click={handleMin} tabindex="-1">
      <Icon d={I.min} size={12} />
    </button>
    <button class="win-btn" title="Maximize / Restore" on:click={handleMax} tabindex="-1">
      <Icon d={I.max} size={12} />
    </button>
    <button class="win-btn close" title="Close" on:click={handleClose} tabindex="-1">
      <Icon d={I.close} size={12} />
    </button>
  </div>
</div>

<style>
  .parzi-titlebar {
    height: 38px;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    position: relative;
    z-index: 100;
    user-select: none;
    -webkit-app-region: drag;
    padding: 0 8px 0 16px;
  }
  .drag-zone {
    flex: 1;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: default;
  }
  .expand-btn {
    width: 26px; height: 26px; flex: none;
    display: inline-flex; align-items: center; justify-content: center;
    background: transparent; border: 1px solid transparent; border-radius: 7px;
    color: var(--text-3); cursor: pointer;
    font: inherit; line-height: 0; padding: 0;
    -webkit-app-region: no-drag;
    transition: background 0.12s ease, color 0.12s ease, border-color 0.12s ease;
  }
  .expand-btn:hover {
    background: var(--surface-3); color: var(--text);
    border-color: var(--line-2);
  }
  .title-crumb {
    font-size: 12px;
    color: var(--text-3);
    display: inline-flex;
    align-items: center;
    gap: 6px;
    letter-spacing: 0.2px;
  }
  .title-crumb b {
    color: var(--text);
    font-weight: 600;
  }

  .crumb-swap { display: inline-flex; align-items: center; gap: 6px; }
  .title-crumb .sep {
    opacity: 0.4;
  }
  .title-crumb .sub {
    color: var(--text-3);
  }
  /* Inspector deck controls: sit just left of the window buttons. */
  .deck-controls {
    display: flex; align-items: center; gap: 2px; margin-right: 6px; padding-right: 8px;
    border-right: 1px solid var(--line-2);
    -webkit-app-region: no-drag;
  }
  .deck-pill {
    display: inline-flex; align-items: center; gap: 5px; height: 24px; padding: 0 8px;
    background: transparent; border: 1px solid transparent; border-radius: 7px;
    color: var(--text-3); font: inherit; font-size: 11.5px; cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease, border-color 0.12s ease;
  }
  .deck-pill:hover { background: var(--surface-2); color: var(--text); }
  .deck-pill.on { background: var(--accent-soft); border-color: var(--accent-line); color: var(--text); }
  .deck-ind { width: 5px; height: 5px; border-radius: 50%; background: var(--accent); }
  .deck-ind.live { background: var(--ok); box-shadow: 0 0 6px var(--ok-line); animation: deck-pulse 2s ease-in-out infinite; }
  @keyframes deck-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }
  .deck-count {
    font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10px; min-width: 15px; height: 15px; padding: 0 4px;
    display: inline-flex; align-items: center; justify-content: center; border-radius: 999px;
    background: var(--surface-3); color: var(--text-3);
  }
  .deck-count.live { background: var(--ok-soft); color: var(--ok); }
  .deck-toggle {
    width: 26px; height: 24px; display: inline-flex; align-items: center; justify-content: center;
    background: transparent; border: 1px solid transparent; border-radius: 7px; padding: 0;
    color: var(--text-3); cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .deck-toggle:hover { background: var(--surface-2); color: var(--text); }
  .deck-toggle.on { color: var(--text); }
  .window-controls {
    display: flex;
    align-items: center;
    gap: 2px;
    -webkit-app-region: no-drag;
  }
  .win-btn {
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--text-3);
    cursor: pointer;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .win-btn:hover {
    background: var(--surface-3);
    color: var(--text);
  }
  .win-btn.close:hover {
    background: var(--bad);
    color: var(--stage);
  }
</style>
