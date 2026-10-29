<script lang="ts">
  export let bg = "";
</script>

<div class="backdrop">
  {#if bg}
    <div class="art">
      <img src={bg} alt="" />
      <div class="shade" />
      <div class="grain" aria-hidden="true" />
    </div>
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
  }
  .art {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: var(--bg-height, 34%);
    overflow: hidden;

    -webkit-mask-image: linear-gradient(to bottom, transparent 0, black 28%, black 55%, transparent 100%);
    mask-image: linear-gradient(to bottom, transparent 0, black 28%, black 55%, transparent 100%);
  }
  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center var(--bg-y, 50%);
  }
  .shade {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(to bottom, color-mix(in srgb, var(--bg) 72%, transparent), transparent 55%),
      color-mix(in srgb, var(--bg) 30%, transparent);
  }
  .grain {
    position: absolute;
    inset: 0;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='140' height='140'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.85' numOctaves='2' stitchTiles='stitch'/%3E%3CfeColorMatrix type='saturate' values='0'/%3E%3C/filter%3E%3Crect width='140' height='140' filter='url(%23n)'/%3E%3C/svg%3E");
    background-size: 140px 140px;
    opacity: 0.055;
  }
</style>
