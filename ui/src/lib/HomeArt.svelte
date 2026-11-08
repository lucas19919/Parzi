<script lang="ts">
  import { fade } from "svelte/transition";
  import { noiseTile } from "./DefaultArt.svelte";

  export let bg = "";
  const noise = noiseTile();
</script>

<!-- Home's stage in full-window mode: the sharp picture in the same card a
     session uses, over the blurred surround (DefaultArt.svelte). -->
<div class="home-art" aria-hidden="true" transition:fade={{ duration: 200 }}>
  <img src={bg} alt="" />
  <div class="grain" style:background-image={noise ? `url(${noise})` : undefined} />
</div>

<style>
  .home-art {
    position: relative;
    flex: 1;
    min-width: 0;
    margin: 0 var(--gap);
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--text) 10%, transparent);
    border-bottom: none;
    border-radius: var(--radius) var(--radius) 0 0;
    background: var(--bg);
  }
  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center var(--bg-y, 50%);
  }
  .grain {
    position: absolute;
    inset: 0;
    background-size: calc(var(--dither-size, 1px) * 128) calc(var(--dither-size, 1px) * 128);
    image-rendering: pixelated;
    mix-blend-mode: soft-light;
    opacity: min(1, calc(var(--dither, 0.055) * 3));
  }
</style>
