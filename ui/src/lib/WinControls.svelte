<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { windowClose, windowMaximize, windowMinimize } from "./windowChrome";

  let maximized = false;

  async function syncChrome() {
    try {
      const w = getCurrentWindow();
      const full = await w.isFullscreen().catch(() => false);
      maximized = full || (await w.isMaximized().catch(() => false));
      document.documentElement.classList.toggle("parzi-maximized", maximized);
    } catch {}
  }

  onMount(() => {
    void syncChrome();
    window.addEventListener("resize", syncChrome);
    return () => window.removeEventListener("resize", syncChrome);
  });
</script>

<div class="window-controls">
  <button class="win-btn" title="Minimize" tabindex="-1" on:click={windowMinimize}>
    <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5h10" stroke="currentColor" /></svg>
  </button>
  <button class="win-btn" title={maximized ? "Restore" : "Maximize"} tabindex="-1" on:click={windowMaximize}>
    {#if maximized}
      <svg width="10" height="10" viewBox="0 0 10 10">
        <path d="M2.5 2V.5h7v7H8M.5 2.5h7v7h-7z" fill="none" stroke="currentColor" />
      </svg>
    {:else}
      <svg width="10" height="10" viewBox="0 0 10 10"><rect x=".5" y=".5" width="9" height="9" fill="none" stroke="currentColor" /></svg>
    {/if}
  </button>
  <button class="win-btn close" title="Close" tabindex="-1" on:click={windowClose}>
    <svg width="10" height="10" viewBox="0 0 10 10"><path d="M1 1l8 8M9 1L1 9" stroke="currentColor" stroke-width="1.1" /></svg>
  </button>
</div>

<style>
  .window-controls {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
  }
  .win-btn {
    width: 28px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--muted);
    cursor: pointer;
  }
  .win-btn:hover {
    background: var(--line);
    color: var(--text);
  }
  .win-btn.close:hover {
    background: var(--bad);
    color: white;
  }
</style>
