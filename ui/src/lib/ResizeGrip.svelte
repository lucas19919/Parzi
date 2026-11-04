<script lang="ts">
  import { createEventDispatcher } from "svelte";

  /** Drag handle on the left edge of a right-hand column. The parent must be position: relative. */
  export let label: string;
  export let min: number;
  export let max: number;

  const dispatch = createEventDispatcher<{ resize: { width: number } }>();
  let dragging = false;

  function down(e: PointerEvent) {
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function move(e: PointerEvent) {
    if (!dragging) return;
    const column = (e.currentTarget as HTMLElement).parentElement;
    const right = column?.getBoundingClientRect().right ?? window.innerWidth;
    dispatch("resize", { width: Math.round(Math.min(max, Math.max(min, right - e.clientX))) });
  }
  const up = () => (dragging = false);
</script>

<div
  class="grip"
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  title="Drag to resize"
  on:pointerdown={down}
  on:pointermove={move}
  on:pointerup={up}
  on:pointercancel={up}
/>

<style>
  .grip {
    position: absolute;
    left: calc(-1 * var(--gap));
    top: 0;
    bottom: 0;
    width: var(--gap);
    z-index: 2;
    cursor: ew-resize;
    touch-action: none;
  }
  .grip::after {
    content: "";
    position: absolute;
    left: 3px;
    top: 12px;
    bottom: 12px;
    width: 2px;
    border-radius: var(--radius-sm);
    background: transparent;
    transition: background 120ms ease;
  }
  .grip:hover::after {
    background: color-mix(in srgb, var(--text) 18%, transparent);
  }
</style>
