<script lang="ts" context="module">
  // What Parzi adds to a context depends on the lane and the folder only:
  // asked once per pair.
  type Parts = { instructions: number; notes: number; tools: number };
  const cache = new Map<string, Promise<Parts | null>>();
  function partsFor(lane: string, folder: string) {
    const key = `${lane}\u0000${folder}`;
    let p = cache.get(key);
    if (!p) {
      p = api.contextParts(lane, folder).catch(() => null);
      cache.set(key, p);
    }
    return p;
  }
</script>

<script lang="ts">
  // How full a session's context is and what fills it: the conversation,
  // and what Parzi adds (its instructions, the tools it offers, the
  // project's notes). Parzi's parts are estimates; the total is the agent's.
  import { createEventDispatcher } from "svelte";
  import { api } from "./api";

  export let used = 0;
  export let limit = 0;
  export let lane = "";
  export let folder = "";
  export let compacting = false;
  export let streaming = false;
  // The agent's name when it compacts on its own (no Compact here).
  export let selfCompacts = "";

  const dispatch = createEventDispatcher<{ compact: void }>();

  let parts: Parts | null = null;
  let partsKey = "";
  $: if (`${lane}\u0000${folder}` !== partsKey) {
    partsKey = `${lane}\u0000${folder}`;
    const key = partsKey;
    parts = null;
    void partsFor(lane, folder).then((p) => {
      if (key === partsKey) parts = p;
    });
  }

  $: pct = limit > 0 ? Math.min(100, Math.round((used / limit) * 100)) : 0;
  // Parzi's parts are counted inside what the agent reports; whatever is
  // left is the conversation.
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
      <i class="s-{r.id}" style:width="{limit > 0 ? (r.n / limit) * 100 : 0}%" />
    {/each}
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
  {#if selfCompacts}
    <div class="note">{selfCompacts} compacts on its own.</div>
  {:else}
    <button class="compact" disabled={compacting || streaming} on:click={() => dispatch("compact")}>
      {compacting ? "Compacting…" : "Compact session"}
    </button>
  {/if}
</div>

<style>
  .ctx {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .top {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }
  .pct {
    font-size: var(--fs-base);
    font-weight: 600;
    color: var(--text);
  }
  .pct.warn {
    color: var(--warn);
  }
  .pct.bad {
    color: var(--bad);
  }
  .of,
  .n,
  .p {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .of {
    font-size: var(--fs-sm);
  }
  .bar {
    display: flex;
    gap: 2px;
    height: 6px;
    overflow: hidden;
    border-radius: 3px;
    background: var(--line);
  }
  .bar i {
    display: block;
    height: 100%;
  }
  /* One hue, fading by part, so the bar stays quiet. */
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
    gap: 2px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 22px;
    font-size: var(--fs-sm);
  }
  .dot {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 2px;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
  }
  .p {
    width: 32px;
    text-align: right;
  }
  .note {
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .compact {
    width: 100%;
    padding: 6px;
    background: var(--hover);
    border: var(--glass-hairline);
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    font-weight: 600;
    cursor: pointer;
  }
  .compact:hover:not(:disabled) {
    background: var(--selected);
  }
  .compact:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
