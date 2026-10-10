<script lang="ts">
  import { onDestroy } from "svelte";
  import { setOverlay } from "../overlay";

  export let enabled = false;
  let open = false;
  // Registered as an overlay while zoomed so the app's own Esc handling
  // (stop the run, leave fullscreen) stands down while this one closes.
  const key = `zoom-${Math.random().toString(36).slice(2)}`;
  $: setOverlay(key, open);
  onDestroy(() => setOverlay(key, false));
  function set(v: boolean) {
    if (!enabled && v) return;
    open = v;
  }
  function toggle() {
    set(!open);
  }
  function onKey(e: KeyboardEvent) {
    if (!open || e.key !== "Escape") return;
    e.preventDefault();
    set(false);
  }
</script>

<svelte:window on:keydown={onKey} />

{#if open}
  <button class="zback" aria-label="Close zoomed view" on:click={() => set(false)} />
{/if}
<div class="zwrap" class:open>
  <slot {open} {toggle} />
</div>

<style>
  .zback {
    position: fixed; inset: 0; z-index: 500; cursor: zoom-out;
    background: rgba(0, 0, 0, 0.62);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    border: none; padding: 0; width: 100vw; height: 100vh;
  }
  .zwrap.open {
    position: fixed; z-index: 501;
    left: 50%; top: 50%; transform: translate(-50%, -50%);
    width: min(1020px, 94vw); max-height: 88vh; overflow: auto;
    background: var(--bg);
    border: 1px solid var(--line); border-radius: 14px;
    box-shadow: var(--shadow);
    padding: 18px 20px;
  }
</style>
