<script lang="ts">
  export let enabled = false;
  let open = false;
  function set(v: boolean) {
    if (!enabled && v) return;
    open = v;
  }
  function toggle() {
    set(!open);
  }
</script>

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
