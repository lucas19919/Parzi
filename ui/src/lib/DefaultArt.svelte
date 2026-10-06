<script lang="ts">
  export let bg = "";
  export let blurred = false;
</script>

<div class="backdrop" class:blurred>
  {#if bg}
    <img src={bg} alt="" />
    <div class="grain" />
    <div class="shade" />
  {/if}
</div>

<style>
  .backdrop {
    position: absolute;
    inset: 0;
    z-index: 0;
    overflow: hidden;
    pointer-events: none;
    background: var(--bg);
    transition:
      transform 750ms cubic-bezier(0.16, 1, 0.3, 1),
      opacity 650ms ease,
      filter 650ms ease;
  }
  .backdrop.blurred {
    transform: translateY(-32px) scale(1.03);
    opacity: 0;
    filter: blur(16px);
  }
  img {
    position: absolute;
    inset: calc(var(--bg-blur) * -2);
    width: calc(100% + var(--bg-blur) * 4);
    height: calc(100% + var(--bg-blur) * 4);
    object-fit: cover;
    filter: blur(var(--bg-blur)) saturate(1.06);
  }
  .shade {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(ellipse at center, transparent 45%, rgba(0, 0, 0, var(--vignette)) 100%),
      color-mix(in srgb, var(--bg) calc(var(--bg-dim) * 100%), transparent);
  }
  .grain {
    position: absolute;
    inset: 0;
    background: url("/dither-bayer.png");
    background-size: 32px 32px;
    image-rendering: pixelated;
    mix-blend-mode: overlay;
    opacity: 0.16;
  }
</style>
