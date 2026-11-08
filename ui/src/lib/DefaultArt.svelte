<script context="module" lang="ts">
  /** A tile of random grey pixels, made once. Scaled up without smoothing,
   *  each pixel turns into a square dot of the dither size. */
  let tile = "";
  export function noiseTile(): string {
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

<!-- The window's backdrop on Home. The picture, blurred and tinted toward
     the theme, is what the title bar and side column sit on, so they read
     over any image. In a band the picture turns sharp below the title bar
     and fades into the page; in full window the sharp picture is the stage
     card instead (HomeArt.svelte) and this is the surround. -->
<div class="backdrop">
  {#if bg}
    <div class="art" transition:fade={{ duration: 200 }}>
      <img class="soft" src={bg} alt="" />
      <div class="tint" />
      <img class="sharp" src={bg} alt="" />
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
  /* The band fades out into the page on an eased curve: a linear mask
     shows its edge and leaves milky mid-tones. */
  .art {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: var(--bg-height, 34%);
    overflow: hidden;
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
  /* The surround: blurred well past any detail, then tinted most of the
     way to the theme, like Windows' Mica. Text keeps its contrast; the
     window keeps the picture's colour. Scaled so the blur has no edge. */
  .soft {
    filter: blur(64px) saturate(1.4);
    transform: scale(1.2);
  }
  .tint {
    position: absolute;
    inset: 0;
    background: color-mix(in srgb, var(--bg) 70%, transparent);
  }
  /* In a band the sharp picture starts just below the title bar, growing
     out of the blur rather than out of white. */
  .sharp {
    --in: linear-gradient(to bottom, rgb(0 0 0 / 0) 0, rgb(0 0 0 / 0) 40px, rgb(0 0 0 / 0.55) 64px, #000 104px);
    -webkit-mask-image: var(--in);
    mask-image: var(--in);
  }
  /* Full window: the surround covers the window; the sharp picture is the
     stage card. */
  :global(html[data-art-fill="full"]) .art {
    height: 100%;
    -webkit-mask-image: none;
    mask-image: none;
  }
  :global(html[data-art-fill="full"]) .sharp {
    display: none;
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
