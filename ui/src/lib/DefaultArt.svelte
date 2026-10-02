<script lang="ts">
  export let bg = "";
  export let blurred = false;
</script>

<div class="art-backdrop" class:blurred>
  {#if bg}
    <img src={bg} alt="" class="art-img" />
  {:else}
    <!-- Clean, solid matte dark background (no gradients, zero slop) -->
    <div class="solid-dark-bg" />
  {/if}
</div>

<style>
  .art-backdrop {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 0;
    overflow: hidden;
    background: #0d0d0f;
  }
  .solid-dark-bg {
    position: absolute;
    inset: 0;
    background: #0d0d0f;
  }
  .art-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0.95;
    transition: filter 400ms ease, opacity 400ms ease;
  }
  .art-backdrop.blurred .art-img {
    filter: blur(24px);
    opacity: 0.15;
  }
</style>
