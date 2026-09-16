<script lang="ts">
  /**
   * Live tools as a horizontal ribbon. When prose starts, it folds into a
   * capsule: tool count, files touched, timer.
   */
  export let tools: { id: string; name: string; label: string; running: boolean; ok?: boolean; ms?: number }[] = [];
  export let prose = false;

  let started = 0;
  let now = Date.now();
  let tick: ReturnType<typeof setInterval> | null = null;
  let open = false;

  $: running = tools.some((t) => t.running);
  $: if (running && !started) started = Date.now();
  $: if (!tools.length) {
    started = 0;
    open = false;
  }

  $: {
    if (running && !tick) {
      tick = setInterval(() => (now = Date.now()), 250);
    } else if (!running && tick) {
      clearInterval(tick);
      tick = null;
    }
  }

  $: files = tools.filter((t) => t.name.startsWith("fs.") || /write|edit/.test(t.name)).length;
  $: fails = tools.filter((t) => !t.running && t.ok === false).length;
  $: elapsed = started ? Math.max(0, Math.round((now - started) / 1000)) : 0;
  $: capsule = `${tools.length} tool${tools.length === 1 ? "" : "s"} · ${files} file${files === 1 ? "" : "s"} · ${elapsed}s`;
  $: folded = prose && !open;
</script>

{#if tools.length}
  {#if folded}
    <button class="cap" on:click={() => (open = true)} title="Show tools">
      <span class="dot" class:live={running} />
      <span>{capsule}{#if fails} · {fails} failed{/if}</span>
    </button>
  {:else}
    <div class="ribbon">
      {#each tools as t (t.id)}
        <span class="chip" class:run={t.running} class:bad={!t.running && t.ok === false}>
          <span class="nm">{t.label || t.name}</span>
          {#if t.running}<span class="tm">{elapsed}s</span>{/if}
        </span>
      {/each}
      {#if prose}
        <button class="fold" on:click={() => (open = false)}>Hide</button>
      {/if}
    </div>
  {/if}
{/if}

<style>
  .ribbon {
    display: flex; align-items: center; gap: 6px; overflow-x: auto;
    padding: 2px 0; mask-image: linear-gradient(90deg, #000 92%, transparent);
  }
  .chip {
    flex: none; display: inline-flex; align-items: center; gap: 6px;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: var(--radius-pill); padding: 4px 10px;
    font-size: 11.5px; color: var(--text-2);
  }
  .chip.run { border-color: var(--accent-line); color: var(--text); }
  .chip.bad { border-color: var(--bad-line); color: var(--bad); }
  .nm { max-width: 160px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tm { font-variant-numeric: tabular-nums; color: var(--text-4); }
  .cap {
    display: inline-flex; align-items: center; gap: 8px; align-self: flex-start;
    background: var(--surface-1); border: 1px solid var(--line-2);
    border-radius: var(--radius-pill); padding: 5px 12px;
    color: var(--text-2); font: inherit; font-size: 12px; cursor: pointer;
  }
  .cap:hover { color: var(--text); border-color: var(--line-3); }
  .dot {
    width: 7px; height: 7px; border-radius: 50%; background: var(--text-4); flex: none;
  }
  .dot.live {
    background: var(--accent); box-shadow: 0 0 8px var(--accent-glow);
    animation: pulse 1.6s var(--ease-spring) infinite;
  }
  .fold {
    flex: none; background: none; border: none; color: var(--text-4);
    font: inherit; font-size: 11px; cursor: pointer;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.45; }
  }
  @media (prefers-reduced-motion: reduce) {
    .dot.live { animation: none; }
  }
</style>
