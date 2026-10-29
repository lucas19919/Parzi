<script lang="ts">
  import { onDestroy } from "svelte";
  import { setOverlay } from "../overlay";
  import { tooltip } from "../tooltip";

  export let enabled = false;
  let open = false;
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
  <button class="zback" use:tooltip={"Close zoomed view"} on:click={() => set(false)} />
{/if}
<div class="zwrap" class:open>
  <slot {open} {toggle} />
</div>

<style>
  .zback {
    position: fixed; inset: 0; z-index: 500; cursor: zoom-out;
    background: var(--scrim);
    border: none; padding: 0; width: 100vw; height: 100vh;
  }
  .zwrap.open {
    position: fixed; z-index: 501;
    left: 50%; top: 50%; transform: translate(-50%, -50%);
    width: min(1020px, 94vw); max-height: 88vh; overflow: auto;
    background: var(--bg);
    border: 1px solid var(--line); border-radius: var(--radius-lg);
    padding: calc(var(--space) * 4) calc(var(--space) * 6);
  }
</style>
