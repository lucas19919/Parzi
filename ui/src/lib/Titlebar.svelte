<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { api } from "./api";
  import Icon from "./Icon.svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { WIN_ICON, startWindowDrag, windowClose, windowMaximize, windowMinimize } from "./windowChrome";

  export let title = "Parzi";
  export let subtitle = "";
  export let showExpand = false;
  export let panelOpen = false;
  export let agentLive = 0;

  const dispatch = createEventDispatcher<{
    expand: void;
    togglePanel: void;
  }>();

  const I = WIN_ICON;

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

  const onMouseDown = startWindowDrag;
  const handleMin = windowMinimize;
  const handleMax = windowMaximize;
  const handleClose = windowClose;
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
    <button class="deck-toggle" class:on={panelOpen} title={panelOpen ? "Collapse inspector (Ctrl+\\)" : "Expand inspector (Ctrl+\\)"}
      aria-pressed={panelOpen} on:click={() => dispatch("togglePanel")}>
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="4" y="5" width="16" height="14" rx="2" />
        <path d="M15 5v14" />
        {#if panelOpen}<rect x="15" y="5" width="5" height="14" rx="1" fill="currentColor" stroke="none" opacity="0.55" />{/if}
      </svg>
      {#if agentLive > 0}<span class="deck-ind live" />{/if}
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
  .deck-controls {
    display: flex; align-items: center; gap: 2px; margin-right: 6px; padding-right: 8px;
    border-right: 1px solid var(--line-2);
    -webkit-app-region: no-drag;
  }
  .deck-ind {
    position: absolute; top: 3px; right: 3px;
    width: 5px; height: 5px; border-radius: 50%; background: var(--ok);
    box-shadow: 0 0 6px var(--ok-line); animation: deck-pulse 2s ease-in-out infinite;
  }
  @keyframes deck-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }
  .deck-toggle {
    position: relative;
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
