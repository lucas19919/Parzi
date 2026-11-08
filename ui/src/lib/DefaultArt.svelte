<script context="module" lang="ts">
  /** A tile of random grey pixels, made once. Scaled up without smoothing,
   *  each pixel turns into a square dot of the dither size. */
  let tile = "";
  function noiseTile(): string {
    if (tile || typeof document === "undefined") return tile;
    const n = 128;
    const c = document.createElement("canvas");
    c.width = c.height = n;
    const ctx = c.getContext("2d");
    if (!ctx) return "";
    const img = ctx.createImageData(n, n);
    for (let i = 0; i < img.data.length; i += 4) {
      const v = Math.random() * 255;
      img.data[i] = img.data[i + 1] = img.data[i + 2] = v;
      img.data[i + 3] = 255;
    }
    ctx.putImageData(img, 0, 0);
    tile = c.toDataURL("image/png");
    return tile;
  }
</script>

<script lang="ts">
  import { fade } from "svelte/transition";

  export let bg = "";
  const noise = noiseTile();
</script>

<div class="backdrop">
  {#if bg}
    <div class="art" transition:fade={{ duration: 200 }}>
      <img src={bg} alt="" />
      <div class="shade" />
      <div class="grain" aria-hidden="true" style:background-image={noise ? `url(${noise})` : undefined} />
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
  /* The band fades in and out on an eased curve, not a straight line: a
     linear mask leaves a visible edge and, on a light theme, milky
     mid-tones where the picture meets the page. */
  .art {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: var(--bg-height, 34%);
    overflow: hidden;
    --fade: linear-gradient(
      to bottom,
      rgb(0 0 0 / 0) 0%,
      rgb(0 0 0 / 0.08) 5%,
      rgb(0 0 0 / 0.28) 11%,
      rgb(0 0 0 / 0.6) 18%,
      rgb(0 0 0 / 0.88) 24%,
      #000 30%,
      #000 52%,
      rgb(0 0 0 / 0.86) 62%,
      rgb(0 0 0 / 0.6) 72%,
      rgb(0 0 0 / 0.32) 82%,
      rgb(0 0 0 / 0.12) 91%,
      rgb(0 0 0 / 0) 100%
    );
    -webkit-mask-image: var(--fade);
    mask-image: var(--fade);
  }
  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: center var(--bg-y, 50%);
  }
  /* On a dark theme the shade dims the picture a little and darkens the top
     behind the tabs. A light theme gets no shade: a wash of white only greys
     the picture out. Text over it carries its own halo instead (App.svelte). */
  .shade {
    position: absolute;
    inset: 0;
    background:
      linear-gradient(to bottom, color-mix(in srgb, var(--bg) 72%, transparent), transparent 55%),
      color-mix(in srgb, var(--bg) 30%, transparent);
  }
  /* On a light theme the band starts at the top edge: fading a picture in
     from white only greys its top. It still fades out into the page. */
  :global(html[data-tone="light"]) .art {
    --fade: linear-gradient(
      to bottom,
      #000 0%,
      #000 52%,
      rgb(0 0 0 / 0.86) 62%,
      rgb(0 0 0 / 0.6) 72%,
      rgb(0 0 0 / 0.32) 82%,
      rgb(0 0 0 / 0.12) 91%,
      rgb(0 0 0 / 0) 100%
    );
  }
  :global(html[data-tone="light"]) .shade {
    background: none;
  }
  /* Full window: the picture fills Home. */
  :global(html[data-art-fill="full"]) .art {
    height: 100%;
    -webkit-mask-image: none;
    mask-image: none;
  }
  :global(html[data-art-fill="full"]) .shade {
    background:
      linear-gradient(to bottom, color-mix(in srgb, var(--bg) 55%, transparent), transparent 22%),
      color-mix(in srgb, var(--bg) 38%, transparent);
  }
  :global(html[data-tone="light"][data-art-fill="full"]) .shade {
    background: none;
  }
  /* Dither, sized and strengthened in Appearance (--dither-size, --dither).
     Soft light lightens and darkens around the picture's own tones, so it
     reads as grain on both themes instead of greying a light one. */
  .grain {
    position: absolute;
    inset: 0;
    background-size: calc(var(--dither-size, 1px) * 128) calc(var(--dither-size, 1px) * 128);
    image-rendering: pixelated;
    mix-blend-mode: soft-light;
    opacity: min(1, calc(var(--dither, 0.055) * 3));
  }
</style>
