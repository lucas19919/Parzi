<script lang="ts">
  export let bg = "";
  export let blurred = false;
</script>

<div class="art-backdrop" class:blurred>
  {#if bg}
    <img src={bg} alt="" class="art-img" />
  {:else}
    <!-- Clean matte dark background with ambient minimal geometric art -->
    <div class="solid-dark-bg" />
    <div class="ambient-art">
      <svg width="280" height="280" viewBox="0 0 200 200" fill="none" stroke="currentColor" class="art-glyph">
        <circle cx="100" cy="100" r="80" stroke-dasharray="4 4" />
        <circle cx="100" cy="100" r="50" />
        <circle cx="100" cy="100" r="20" />
      </svg>
    </div>
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
    transition:
      transform 750ms cubic-bezier(0.16, 1, 0.3, 1),
      opacity 650ms ease,
      filter 650ms ease;
  }
  .solid-dark-bg {
    position: absolute;
    inset: 0;
    background: #0d0d0f;
  }
  .ambient-art {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #27272a;
    opacity: 0.45;
    transition: transform 750ms cubic-bezier(0.16, 1, 0.3, 1), opacity 650ms ease;
  }
  .art-glyph {
    stroke-width: 0.6;
  }
  .art-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0.95;
    transition:
      filter 650ms ease,
      opacity 650ms ease,
      transform 750ms cubic-bezier(0.16, 1, 0.3, 1);
  }
  .art-backdrop.blurred {
    transform: translateY(-32px) scale(1.03);
    opacity: 0;
    filter: blur(16px);
  }
  .art-backdrop.blurred .art-img {
    opacity: 0;
  }
  .art-backdrop.blurred .ambient-art {
    transform: translateY(-24px) scale(1.05);
    opacity: 0;
  }
</style>
