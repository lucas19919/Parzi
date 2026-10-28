<script lang="ts">
  // A distribution: raw values bucketed into round-edged bins (spec.ts),
  // drawn as contiguous columns with a dashed mean. Hover a bin for its
  // range, count and share.
  import { fmt, type HistBin } from "./spec";
  import { hideTip, showTip } from "./tip";
  import DataTable from "./DataTable.svelte";
  import { onDestroy } from "svelte";

  export let bins: HistBin[] = [];
  export let total = 0;
  export let mean = 0;
  export let unit = "";
  export let height = 160;
  export let label = "";

  const TOP = 18;
  const BOTTOM = 22;
  const CH = 6.4;

  let w = 0;
  let hover = -1;

  $: counts = bins.map((b) => b.count);
  $: max = Math.max(1, ...counts);
  $: texts = bins.map((b) => `${fmt(b.lo, unit)}–${fmt(b.hi, unit)}`);
  $: aria = `${label ? `${label}: ` : ""}histogram of ${total} values in ${bins.length} bins`;
  $: plotH = height - TOP - BOTTOM;
  $: Y = (c: number) => TOP + (1 - c / max) * plotH;
  $: band = bins.length ? w / bins.length : 0;
  $: lo = bins.length ? bins[0].lo : 0;
  $: hi = bins.length ? bins[bins.length - 1].hi : 1;
  $: meanX = bins.length ? Math.max(0, Math.min(w, ((mean - lo) / (hi - lo || 1)) * w)) : 0;
  // Edge labels that fit: first, last, and every kth in between.
  $: every = Math.max(1, Math.ceil(bins.length / Math.max(1, Math.floor(w / 64))));
  $: edges = bins.length ? [...bins.map((b) => b.lo), bins[bins.length - 1].hi] : [];

  function tipAt(i: number, box: DOMRect) {
    hover = i;
    const pct = total ? Math.round((bins[i].count / total) * 100) : 0;
    showTip(box, texts[i], [
      { value: String(bins[i].count), label: `${pct}% of ${total}` },
    ]);
  }

  function enter(e: PointerEvent, i: number) {
    const svg = (e.currentTarget as SVGElement).ownerSVGElement;
    const r = svg?.getBoundingClientRect();
    if (!r) return;
    const x = r.left + band * i + band / 2;
    const y = r.top + Y(bins[i].count);
    tipAt(i, new DOMRect(x - 4, y - 4, 8, 8));
  }

  function leave() {
    hover = -1;
    hideTip();
  }

  onDestroy(hideTip);
</script>

<div class="hist" bind:clientWidth={w} style:height="{height}px">
  {#if w > 0 && bins.length}
    <svg width={w} {height} role="img" aria-label={aria}>
      <title>{aria}</title>
      {#each bins as b, i}
        {#if hover === i}<rect class="band" x={band * i + 0.5} y={TOP - 14} width={band - 1} height={plotH + 14} rx="3" />{/if}
        {#if b.count > 0}
          <rect
            class="bar"
            x={(band * i + 0.75).toFixed(1)}
            y={Y(b.count).toFixed(1)}
            width={Math.max(1, band - 1.5).toFixed(1)}
            height={Math.max(1, TOP + plotH - Y(b.count)).toFixed(1)}
            rx="2"
          />
        {:else}
          <line class="zero" x1={(band * i + 1).toFixed(1)} x2={(band * (i + 1) - 1).toFixed(1)} y1={TOP + plotH + 0.5} y2={TOP + plotH + 0.5} />
        {/if}
      {/each}
      {#if total > 0}
        <line class="mean" x1={meanX.toFixed(1)} x2={meanX.toFixed(1)} y1={TOP - 14} y2={TOP + plotH} />
        <text class="mean-tag" x={Math.min(meanX + 4, w - 34)} y={TOP - 4}>mean</text>
      {/if}
      <line class="base" x1="0" x2={w} y1={TOP + plotH + 0.5} y2={TOP + plotH + 0.5} />
      {#each edges as e, i}
        {#if i % every === 0 || i === edges.length - 1}
          <text class="tick" x={Math.max(20, Math.min(w - 20, (i / (edges.length - 1 || 1)) * w))} y={height - 6} text-anchor="middle">{fmt(e, unit)}</text>
        {/if}
      {/each}
      {#each bins as _, i}
        <rect class="hit" x={band * i} y="0" width={band} {height} on:pointerenter={(e) => enter(e, i)} on:pointerleave={leave} />
      {/each}
    </svg>
  {/if}
  <DataTable caption={aria} head={["Range", "Count"]} rows={bins.map((b, i) => [texts[i], String(b.count)])} />
</div>

<style>
  .hist {
    position: relative;
    width: 100%;
    min-width: 0;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .bar {
    fill: var(--accent);
  }
  .base,
  .zero {
    stroke: var(--line);
    stroke-width: 1;
  }
  .mean {
    stroke: var(--work);
    stroke-width: 1;
    stroke-dasharray: 3 3;
  }
  .mean-tag {
    fill: var(--work);
    font-size: var(--fs-xs);
  }
  .tick {
    fill: var(--muted);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
  }
  .band {
    fill: var(--hover);
  }
  .hit {
    fill: transparent;
  }
</style>
