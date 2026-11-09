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
  import { onDestroy } from "svelte";
  import { fade } from "svelte/transition";

  export let bg = "";
  const noise = noiseTile();

  /* Leaving Home, the picture fades out by a CSS transition rather than a
     Svelte one. Opening a session can hold the page for a moment while the
     thread renders; a Svelte fade is timed from the click and would be all
     but over by the first frame, so the picture would vanish. A CSS
     transition starts from the first frame that shows the change. */
  let shown = bg;
  let leaving = false;
  let fallback: ReturnType<typeof setTimeout> | undefined;
  $: follow(bg);
  function follow(next: string) {
    clearTimeout(fallback);
    if (next) {
      shown = next;
      leaving = false;
    } else if (shown) {
      leaving = true;
      fallback = setTimeout(gone, 1500);
    }
  }
  function gone() {
    if (!leaving) return;
    shown = "";
    leaving = false;
  }
  onDestroy(() => clearTimeout(fallback));
</script>

<div class="backdrop">
  {#if shown}
    <div
      class="art"
      class:leaving
      in:fade={{ duration: 220 }}
      on:transitionend={(e) => e.target === e.currentTarget && gone()}
    >
      <img src={shown} alt="" />
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
  .art {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: var(--bg-height, 34%);
    overflow: hidden;

    -webkit-mask-image: linear-gradient(to bottom, transparent 0, black 28%, black 55%, transparent 100%);
    mask-image: linear-gradient(to bottom, transparent 0, black 28%, black 55%, transparent 100%);
    /* An even curve: the app's sharp ease-out would clear the picture in
       the first few frames, which reads as a cut. */
    transition: opacity 520ms var(--ease-even);
  }
  .art.leaving {
    opacity: 0;
  }
  @media (prefers-reduced-motion: reduce) {
    .art {
      transition-duration: 1ms;
    }
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
  /* Full window: the picture fills Home, with an even shade so the tabs,
     the composer and the cards stay readable over it. */
  :global(html[data-art-fill="full"]) .art {
    height: 100%;
    -webkit-mask-image: none;
    mask-image: none;
  }
  /* A light theme gets no veil: a wash of white only fogs the picture.
     Text over it reads by its own colour instead (App.svelte), and the
     dither blends into the picture's tones rather than greying it. */
  :global(html[data-tone="light"]) .shade {
    background: none;
  }
  :global(html[data-tone="light"]) .grain {
    mix-blend-mode: soft-light;
    opacity: min(1, calc(var(--dither, 0.055) * 3));
  }
  :global(html:not([data-tone="light"])[data-art-fill="full"]) .shade {
    background:
      linear-gradient(to bottom, color-mix(in srgb, var(--bg) 55%, transparent), transparent 22%),
      color-mix(in srgb, var(--bg) 38%, transparent);
  }
  /* Dither, sized and strengthened in Appearance (--dither-size, --dither). */
  .grain {
    position: absolute;
    inset: 0;
    background-size: calc(var(--dither-size, 1px) * 128) calc(var(--dither-size, 1px) * 128);
    image-rendering: pixelated;
    opacity: var(--dither, 0.055);
  }
</style>
