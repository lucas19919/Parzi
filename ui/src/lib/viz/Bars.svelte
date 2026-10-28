<script lang="ts">
  // Bars from one baseline, in the accent, with a 4px round at the data end
  // only. Rows (horizontal) carry their label and value as text; columns
  // (vertical) put the value on the cap when it fits, else an axis.
  import { fmt, niceTicks, SERIES_COLORS, type Point } from "./spec";
  import { onDestroy } from "svelte";
  import { hideTip, showTip } from "./tip";
  import DataTable from "./DataTable.svelte";

  // A readout left up by a chart that went away goes with it.
  onDestroy(hideTip);

  export let items: Point[] = [];
  export let orient: "h" | "v" = "h";
  export let unit = "";
  export let height = 160;
  export let label = "";

  const INK = SERIES_COLORS[0];
  const BAR = 10;
  const TOP = 18;
  const BOTTOM = 22;
  const CH = 6.4;

  // Rows: each track's width, measured.
  let tracks: number[] = [];
  // Columns: the plot's width.
  let w = 0;
  let hover = -1;

  $: vals = items.map((p) => p.value);
  $: max = Math.max(0, ...vals);
  $: min = Math.min(0, ...vals);
  $: texts = items.map((p) => fmt(p.value, unit));
  $: aria = `${label ? `${label}: ` : ""}bar chart, ${items.map((p, i) => `${p.label} ${texts[i]}`).join(", ")}`;

  // Horizontal: a rounded end on the right, square at the baseline.
  function rowPath(len: number, h: number): string {
    if (len <= 0.5) return "";
    const r = Math.min(4, len, h / 2);
    return `M0,0H${(len - r).toFixed(1)}A${r},${r} 0 0 1 ${len.toFixed(1)},${r}V${h - r}A${r},${r} 0 0 1 ${(len - r).toFixed(1)},${h}H0Z`;
  }

  // Vertical: from y0 (baseline) to y1, rounded at y1.
  function colPath(x: number, bw: number, y0: number, y1: number): string {
    const hgt = Math.abs(y0 - y1);
    if (hgt <= 0.5) return "";
    const r = Math.min(4, bw / 2, hgt);
    const up = y1 < y0;
    const e = up ? y1 + r : y1 - r;
    const sweep = up ? 1 : 0;
    return `M${x.toFixed(1)},${y0.toFixed(1)}V${e.toFixed(1)}A${r},${r} 0 0 ${sweep} ${(x + r).toFixed(1)},${y1.toFixed(1)}H${(x + bw - r).toFixed(1)}A${r},${r} 0 0 ${sweep} ${(x + bw).toFixed(1)},${e.toFixed(1)}V${y0.toFixed(1)}Z`;
  }

  // Columns.
  $: n = items.length;
  $: band = n ? w / n : 0;
  $: bw = Math.max(2, Math.min(24, band * 0.62));
  $: capsFit = n > 0 && texts.every((t) => t.length * CH <= band - 2);
  $: ticks = capsFit ? [min, max] : niceTicks(min, max, height < 140 ? 2 : 3);
  $: lo = Math.min(min, ticks[0]);
  $: hi = Math.max(max, ticks[ticks.length - 1]) || 1;
  $: plotH = height - TOP - BOTTOM;
  $: Y = (v: number) => TOP + (1 - (v - lo) / (hi - lo || 1)) * plotH;
  $: base = Y(0);
  $: longest = Math.max(1, ...items.map((p) => p.label.length));
  $: every = Math.max(1, Math.ceil((longest * CH + 8) / Math.max(1, band)));

  function tipAt(i: number, box: DOMRect) {
    hover = i;
    showTip(box, items[i].label, [{ value: texts[i], label: "" }]);
  }

  function enterRow(e: PointerEvent, i: number) {
    const track = (e.currentTarget as HTMLElement).querySelector(".track");
    const r = track?.getBoundingClientRect();
    if (!r) return;
    const len = max > 0 ? (Math.max(0, items[i].value) / max) * r.width : 0;
    tipAt(i, new DOMRect(r.left + len - 4, r.top, 8, r.height));
  }

  function enterCol(e: PointerEvent, i: number) {
    const svg = (e.currentTarget as SVGElement).ownerSVGElement;
    const r = svg?.getBoundingClientRect();
    if (!r) return;
    const x = r.left + band * i + band / 2;
    const y = r.top + Math.min(Y(items[i].value), base);
    tipAt(i, new DOMRect(x - 4, y - 4, 8, 8));
  }

  function leave() {
    hover = -1;
    hideTip();
  }
</script>

{#if orient === "h"}
  <div class="rows" role="list" aria-label={aria}>
    {#each items as p, i}
      <div class="row" class:on={hover === i} role="listitem" on:pointerenter={(e) => enterRow(e, i)} on:pointerleave={leave}>
        <span class="label">{p.label}</span>
        <span class="track" bind:clientWidth={tracks[i]}>
          {#if tracks[i]}
            <svg width={tracks[i]} height={BAR} aria-hidden="true">
              <path d={rowPath(max > 0 ? (Math.max(0, p.value) / max) * tracks[i] : 0, BAR)} style:fill={INK} />
            </svg>
          {/if}
        </span>
        <span class="value">{texts[i]}</span>
      </div>
    {/each}
  </div>
{:else}
  <div class="cols" bind:clientWidth={w} style:height="{height}px">
    {#if w > 0}
      <svg width={w} {height} role="img" aria-label={aria}>
        <title>{aria}</title>
        {#if !capsFit}
          {#each ticks as t}
            <line class="grid" x1="0" x2={w} y1={Math.round(Y(t)) + 0.5} y2={Math.round(Y(t)) + 0.5} />
            <text class="tick" x="0" y={Y(t) - 4}>{fmt(t, unit)}</text>
          {/each}
        {/if}
        {#each items as p, i}
          {#if hover === i}<rect class="band" x={band * i + 1} y={TOP - 14} width={band - 2} height={plotH + 14} rx="4" />{/if}
          <path d={colPath(band * i + (band - bw) / 2, bw, base, Y(p.value))} style:fill={INK} />
          {#if capsFit}
            <text class="cap" x={band * i + band / 2} y={(p.value >= 0 ? Y(p.value) : base) - 5} text-anchor="middle">{texts[i]}</text>
          {/if}
          {#if i % every === 0}
            <text class="tick" x={band * i + band / 2} y={height - 6} text-anchor="middle">{p.label}</text>
          {/if}
        {/each}
        <line class="base" x1="0" x2={w} y1={Math.round(base) + 0.5} y2={Math.round(base) + 0.5} />
        {#each items as _, i}
          <rect class="hit" role="presentation" x={band * i} y="0" width={band} {height} on:pointerenter={(e) => enterCol(e, i)} on:pointerleave={leave} />
        {/each}
      </svg>
    {/if}
    <DataTable caption={aria} head={["", "Value"]} rows={items.map((p, i) => [p.label, texts[i]])} />
  </div>
{/if}

<style>
  .rows {
    display: grid;
    grid-template-columns: fit-content(40%) minmax(40px, 1fr) max-content;
    column-gap: calc(var(--space) * 3);
  }
  .row {
    display: grid;
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    align-items: center;
    height: 26px;
    padding: 0 calc(var(--space) * 2);
    margin: 0 calc(var(--space) * -2);
    border-radius: var(--radius-sm);
  }
  .row.on {
    background: var(--hover);
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--text);
  }
  .track {
    display: flex;
    align-items: center;
    min-width: 0;
    height: 100%;
  }
  .value {
    font-size: var(--fs-sm);
    color: var(--muted);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .cols {
    position: relative;
    width: 100%;
    min-width: 0;
  }
  .grid,
  .base {
    stroke: var(--line);
    stroke-width: 1;
  }
  .tick,
  .cap {
    fill: var(--muted);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
  }
  .cap {
    fill: var(--text);
  }
  .band {
    fill: var(--hover);
  }
  .hit {
    fill: transparent;
  }
</style>
