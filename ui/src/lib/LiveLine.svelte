<script lang="ts">
  // What an agent is doing right now, as one line under the thread: its
  // face, the step it is on, how long the turn has run and roughly how
  // much it has written.
  import { onDestroy } from "svelte";

  export let text = "";
  // Rough tokens written this turn (characters / 4).
  export let tokens = 0;

  const t0 = Date.now();
  let now = t0;
  const timer = window.setInterval(() => (now = Date.now()), 1000);
  onDestroy(() => window.clearInterval(timer));

  $: secs = Math.floor((now - t0) / 1000);
  $: clock = secs < 60 ? `${secs}s` : `${Math.floor(secs / 60)}m ${String(secs % 60).padStart(2, "0")}s`;
  $: written = tokens >= 1000 ? `${(tokens / 1000).toFixed(1)}k tokens` : tokens >= 50 ? `${tokens} tokens` : "";
</script>

<div class="live" role="status" aria-live="polite">
  <span class="face" aria-hidden="true"><span class="eye" /><span class="eye" /></span>
  <span class="what">{text}</span>
  <span class="meta">{clock}{written ? ` · ${written}` : ""}</span>
</div>

<style>
  .live {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    min-height: 28px;
    font-size: var(--fs-md);
  }
  .what {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
    background: linear-gradient(90deg, var(--muted) 0%, var(--text) 40%, var(--muted) 80%);
    background-size: 200% 100%;
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    animation: shimmer 2.4s linear infinite;
  }
  .meta {
    flex: none;
    color: var(--muted);
    font-size: var(--fs-sm);
    font-variant-numeric: tabular-nums;
  }
  /* The face: a soft round with two eyes that blink now and then. */
  .face {
    width: 22px;
    height: 22px;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space);
    border-radius: 50%;
    background: color-mix(in srgb, var(--text) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--text) 16%, transparent);
    animation: bob 2.4s ease-in-out infinite;
  }
  .eye {
    width: 3px;
    height: 4.5px;
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
  @keyframes shimmer {
    from {
      background-position: 100% 0;
    }
    to {
      background-position: -100% 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .face,
    .eye,
    .what {
      animation: none;
    }
    .what {
      -webkit-text-fill-color: currentColor;
      background: none;
    }
  }
</style>
