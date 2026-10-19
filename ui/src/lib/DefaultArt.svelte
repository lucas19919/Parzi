<script lang="ts">
  export let bg = "";
</script>

<!-- Behind everything, pages included: a page covers all but the top bar,
     which keeps the window's own background. -->
<div class="backdrop">
  {#if bg}
    <!-- The picture, its shade and the dither reach --bg-height down the
         window and fade into the stage colour there. -->
    <div class="art">
      <img src={bg} alt="" />
      <div class="grain" />
      <div class="shade" />
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
    inset: 0 0 auto;
    height: var(--bg-height, 100%);
    overflow: hidden;
    /* No fade at full height; the shorter the picture, the longer its fade. */
    --fade: calc((100% - var(--bg-height, 100%)) * 0.8);
    -webkit-mask-image: linear-gradient(to bottom, #000 calc(100% - var(--fade)), transparent);
    mask-image: linear-gradient(to bottom, #000 calc(100% - var(--fade)), transparent);
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
    /* The dim is always black: a light stage must darken the picture,
       never veil it white. */
    /* A scrim under the top bar: tabs and window buttons stay readable
       over a bright picture, whatever the dim. */
    background:
      linear-gradient(to bottom, rgba(0, 0, 0, 0.45), transparent 80px),
      radial-gradient(ellipse at center, transparent 45%, rgba(0, 0, 0, var(--vignette)) 100%),
      color-mix(in srgb, #000 calc(var(--bg-dim) * 100%), transparent);
  }
  /* The dither lies over the whole picture and fades out with it. */
  .grain {
    position: absolute;
    inset: 0;
    background: url("/dither-bayer.png");
    background-size: var(--grain-size, 32px) var(--grain-size, 32px);
    image-rendering: pixelated;
    opacity: var(--grain, 0.16);
  }
</style>
