<script lang="ts">
  export let bg = "";
</script>


<div class="backdrop">
  {#if bg}
    <div class="art">
      <img src={bg} alt="" />
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
    top: 0;
    left: 0;
    right: 0;
    height: 34%;
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
</style>
