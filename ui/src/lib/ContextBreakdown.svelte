<script lang="ts" context="module">
  type Parts = { instructions: number; notes: number; tools: number };
  const cache = new Map<string, Promise<Parts | null>>();
  function partsFor(folder: string) {
    const key = folder;
    let p = cache.get(key);
    if (!p) {
      p = api.contextParts(folder).catch(() => null);
      cache.set(key, p);
    }
    return p;
  }
</script>

<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { api } from "./api";

  export let used = 0;
  export let limit = 0;
  export let folder = "";
  export let compacting = false;
  export let streaming = false;

  const dispatch = createEventDispatcher<{ compact: void }>();

  let parts: Parts | null = null;
  let partsKey = "";
  $: if (folder !== partsKey) {
    partsKey = folder;
    const key = partsKey;
    parts = null;
    void partsFor(folder).then((p) => {
      if (key === partsKey) parts = p;
    });
  }

  $: pct = limit > 0 ? Math.min(100, Math.round((used / limit) * 100)) : 0;
  $: ours = parts ? parts.instructions + parts.tools + parts.notes : 0;
  $: rows = [
    { id: "conv", label: "Conversation", n: Math.max(0, used - ours) },
    ...(parts
      ? [
          { id: "inst", label: "Parzi instructions", n: parts.instructions },
          { id: "tools", label: "Parzi tools", n: parts.tools },
          { id: "notes", label: "Project notes", n: parts.notes },
        ]
      : []),
  ].filter((r) => r.n > 0);
  $: filled = rows.reduce((s, r) => s + r.n, 0);
  $: free = Math.max(0, limit - filled);

  function share(n: number) {
    if (limit <= 0) return "0%";
    const p = (n / limit) * 100;
    return p > 0 && p < 1 ? "<1%" : `${Math.round(p)}%`;
  }
  function tok(n: number) {
    if (n >= 1_000_000) return `${+(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${+(n / 1_000).toFixed(n >= 100_000 ? 0 : 1)}k`;
    return String(n);
  }
</script>

<div class="ctx">
  <div class="top">
    <span class="pct" class:warn={pct >= 70} class:bad={pct >= 90}>{pct}% used</span>
    <span class="of">{tok(used)} / {tok(limit)}</span>
  </div>
  <div class="bar" aria-hidden="true">
    {#each rows as r (r.id)}
      <i class="s-{r.id}" style:flex-grow={r.n} />
    {/each}
    {#if free > 0}<i class="s-free" style:flex-grow={free} />{/if}
  </div>
  <div class="rows">
    {#each rows as r (r.id)}
      <div class="row">
        <i class="dot s-{r.id}" />
        <span class="label">{r.label}</span>
        <span class="n">{tok(r.n)}</span>
        <span class="p">{share(r.n)}</span>
      </div>
    {/each}
    <div class="row">
      <i class="dot s-free" />
      <span class="label">Free</span>
      <span class="n">{tok(free)}</span>
      <span class="p">{share(free)}</span>
    </div>
  </div>
  <button class="btn sm" disabled={compacting || streaming} on:click={() => dispatch("compact")}>
    {compacting ? "Compacting…" : "Compact session"}
  </button>
</div>

<style>
  .ctx {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 3);
  }
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: calc(var(--space) * 2);
  }
  .pct {
    font-size: var(--fs-base);
    font-weight: 600;
    color: var(--text);
  }
  .pct.warn {
    color: var(--warn-text);
  }
  .pct.bad {
    color: var(--bad-text);
  }
  .of,
  .n,
  .p {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    white-space: nowrap;
  }
  .of {
    font-size: var(--fs-sm);
  }
  .bar {
    display: flex;
    align-items: stretch;
    height: 6px;
    overflow: hidden;
    border-radius: var(--radius-sm);
    background: var(--line);
    line-height: 0;
  }
  .bar i {
    display: block;
    flex: 1 1 0;
    height: 6px;
    min-width: 0;
    min-height: 0;
  }
  .s-conv {
    background: var(--accent);
  }
  .s-inst {
    background: color-mix(in srgb, var(--accent) 68%, var(--bg));
  }
  .s-tools {
    background: color-mix(in srgb, var(--accent) 46%, var(--bg));
  }
  .s-notes {
    background: color-mix(in srgb, var(--accent) 30%, var(--bg));
  }
  .s-free {
    background: var(--line);
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    margin: 0;
    padding: 0;
    background: none;
    border: none;
    overflow: visible;
  }
  .row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-height: calc(var(--space) * 6);
    margin: 0;
    padding: 0;
    border: none;
    background: none;
    font-size: var(--fs-sm);
    line-height: 1.4;
  }
  .dot {
    display: block;
    width: 8px;
    height: 8px;
    flex: none;
    margin-right: var(--space);
    border-radius: 50%;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .n {
    min-width: 5ch;
    text-align: right;
  }
  .p {
    width: calc(var(--space) * 8);
    text-align: right;
  }
  .note {
    font-size: var(--fs-sm);
    color: var(--muted);
  }
</style>
