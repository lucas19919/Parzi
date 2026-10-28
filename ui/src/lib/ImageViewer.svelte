<script lang="ts">
  // An image at full size over the app: click outside it or press Esc to
  // close it.
  import { onDestroy } from "svelte";
  import { fade, scale } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import { viewed, closeImage } from "./imageViewer";
  import { setOverlay } from "./overlay";
  import { tooltip } from "./tooltip";

  // An overlay while open, so the app's own Esc (stop the run) stands down.
  $: setOverlay("image-viewer", !!$viewed);
  onDestroy(() => setOverlay("image-viewer", false));

  function onKey(e: KeyboardEvent) {
    if (!$viewed || e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    closeImage();
  }
</script>

<svelte:window on:keydown|capture={onKey} />

{#if $viewed}
  <!-- Esc closes it (window handler); the click outside is a mouse shortcut. -->
  <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-noninteractive-element-interactions -->
  <div class="viewer" role="dialog" aria-modal="true" aria-label={$viewed.alt || "Image"} on:click|self={closeImage} transition:fade={{ duration: 140 }}>
    <img src={$viewed.src} alt={$viewed.alt} draggable="false" transition:scale={{ duration: 180, start: 0.96 }} />
    <button class="x" aria-label="Close" use:tooltip={"Close (Esc)"} on:click={closeImage}><Icon name="close" /></button>
    {#if $viewed.alt}<div class="cap">{$viewed.alt}</div>{/if}
  </div>
{/if}

<style>
  .viewer {
    position: fixed;
    inset: 0;
    z-index: 1500;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: calc(var(--space) * 8) calc(var(--space) * 8);
    background: var(--scrim);
    -webkit-backdrop-filter: blur(6px);
    backdrop-filter: blur(6px);
    cursor: zoom-out;
  }
  img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    border-radius: var(--radius-lg);
    cursor: default;
  }
  .x {
    position: absolute;
    top: 12px;
    right: 12px;
    width: 32px;
    height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: var(--glass-strong-bg);
    border: var(--glass-border);
    border-radius: 50%;
    color: var(--text);
    cursor: pointer;
  }
  .x:hover {
    background: var(--panel);
  }
  .cap {
    position: absolute;
    bottom: 16px;
    left: 50%;
    max-width: 70%;
    transform: translateX(-50%);
    padding: var(--space) calc(var(--space) * 3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border-radius: 999px;
    background: var(--glass-strong-bg);
    color: var(--text);
    font-size: var(--fs-sm);
    cursor: default;
  }
</style>
