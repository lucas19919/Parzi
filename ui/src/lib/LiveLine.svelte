<script lang="ts">
  import { onDestroy } from "svelte";
  import ThinkingFace from "./ThinkingFace.svelte";

  export let text = "";
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
  <ThinkingFace />
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
  @keyframes shimmer {
    from {
      background-position: 100% 0;
    }
    to {
      background-position: -100% 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .what {
      animation: none;
    }
    .what {
      -webkit-text-fill-color: currentColor;
      background: none;
    }
  }
</style>
