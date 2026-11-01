<script lang="ts">
  /** The face that bobs and blinks while an agent works. */
  export let size = 22;
</script>

<span class="face" style="--s: {size}px" aria-hidden="true"><span class="eye" /><span class="eye" /></span>

<style>
  .face {
    width: var(--s);
    height: var(--s);
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: calc(var(--s) * 0.18);
    box-sizing: border-box;
    border-radius: 50%;
    background: var(--selected);
    border: 1px solid color-mix(in srgb, var(--text) 16%, transparent);
    animation: bob 2.4s ease-in-out infinite;
  }
  .eye {
    width: calc(var(--s) * 0.136);
    height: calc(var(--s) * 0.205);
    border-radius: var(--radius-sm);
    background: var(--text);
    opacity: 0.85;
    animation: blink 4.4s ease-in-out infinite;
    transform-origin: center;
  }
  @keyframes bob {
    0%,
    100% {
      transform: translateY(0);
    }
    50% {
      transform: translateY(-1.5px);
    }
  }
  @keyframes blink {
    0%,
    91%,
    100% {
      transform: scaleY(1);
    }
    94.5% {
      transform: scaleY(0.12);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .face,
    .eye {
      animation: none;
    }
  }
</style>
