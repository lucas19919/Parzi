<script lang="ts">
  // Lines over a shared x: 2px strokes in the theme's series colours, a
  // hairline grid at round numbers, and a crosshair that reads every series
  // at the x under the pointer.
  import { axisFloor, fmt, niceTicks, SERIES_COLORS, type Series } from "./spec";
  import { onDestroy } from "svelte";
  import { hideTip, showTip } from "./tip";
  import DataTable from "./DataTable.svelte";

  // A readout left up by a chart that went away goes with it.
  onDestroy(hideTip);

  export let labels: string[] = [];
  export let series: Series[] = [];
  export let area = false;
  export let points = false;
  export let unit = "";
  export let height = 180;
  export let label = "";

  const TOP = 8;
  const BOTTOM = 22;
  const RIGHT = 8;
  // Inter's tabular digits at 11px.
  const CH = 6.4;

  let w = 0;
  let hover = -1;

  $: n = labels.length;
  $: all = series.flatMap((s) => s.values).filter((v): v is number => v !== null);
  $: ticks = niceTicks(axisFloor(Math.min(...all), Math.max(...all)), Math.max(...all), height < 150 ? 2 : 3);
  $: lo = ticks[0];
  $: hi = ticks[ticks.length - 1];
  $: tickText = ticks.map((t) => fmt(t, unit));
  $: left = Math.ceil(Math.max(...tickText.map((t) => t.length)) * CH) + 10;
  $: plotW = Math.max(10, w - left - RIGHT);
  $: plotH = height - TOP - BOTTOM;
  $: step = n > 1 ? plotW / (n - 1) : 0;
  $: X = (i: number) => left + i * step;
  $: Y = (v: number) => TOP + (1 - (v - lo) / (hi - lo || 1)) * plotH;
  $: paths = series.map((s) => trace(s.values, X, Y));
  $: fills = area ? series.map((s) => wash(s.values, X, Y, Y(Math.max(lo, Math.min(0, hi))))) : [];
  $: xTicks = pickLabels(n, plotW, labels);
  $: showDots = points && n <= 60;
  $: color = (i: number) => SERIES_COLORS[i % SERIES_COLORS.length];
  $: aria = `${label ? `${label}: ` : ""}line chart, ${series.length > 1 ? `${series.length} series` : "one series"} over ${n} points${all.length ? `, from ${fmt(Math.min(...all), unit)} to ${fmt(Math.max(...all), unit)}` : ""}`;

  // A series as path segments; a null breaks the line.
  function trace(vals: (number | null)[], x: (i: number) => number, y: (v: number) => number): string {
    let d = "";
    let pen = false;
    vals.forEach((v, i) => {
      if (v === null) {
        pen = false;
        return;
      }
      d += `${pen ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`;
      pen = true;
    });
    return d;
  }

  function wash(vals: (number | null)[], x: (i: number) => number, y: (v: number) => number, base: number): string {
    let d = "";
    let run: number[] = [];
    const close = () => {
      if (run.length > 1) {
        d += `M${x(run[0]).toFixed(1)},${base.toFixed(1)}`;
        for (const i of run) d += `L${x(i).toFixed(1)},${y(vals[i] as number).toFixed(1)}`;
        d += `L${x(run[run.length - 1]).toFixed(1)},${base.toFixed(1)}Z`;
      }
      run = [];
    };
    vals.forEach((v, i) => (v === null ? close() : run.push(i)));
    close();
    return d;
  }

  // As many x labels as fit, evenly spaced, the last one always shown.
  function pickLabels(count: number, width: number, text: string[]): number[] {
    if (!count) return [];
    const longest = Math.max(...text.map((t) => t.length), 1);
    const fit = Math.max(2, Math.floor(width / (longest * CH + 16)));
    if (count <= fit) return text.map((_, i) => i);
    const every = Math.ceil((count - 1) / (fit - 1));
    const out: number[] = [];
    for (let i = 0; i < count; i += every) out.push(i);
    if (count - 1 - out[out.length - 1] < every / 2) out.pop();
    out.push(count - 1);
    return out;
  }

  function move(e: PointerEvent) {
    const r = (e.currentTarget as SVGSVGElement).getBoundingClientRect();
    const i = Math.max(0, Math.min(n - 1, Math.round((e.clientX - r.left - left) / (step || 1))));
    hover = i;
    const ys = series.map((s) => s.values[i]).filter((v): v is number => v !== null).map(Y);
    const top = r.top + (ys.length ? Math.min(...ys) : TOP);
    const x = r.left + X(i);
    const rows = series
      .map((s, si) => ({ s, si }))
      .filter(({ s }) => s.values[i] !== null)
      .map(({ s, si }) => ({ value: fmt(s.values[i] as number, unit), label: series.length > 1 ? s.name : "", color: series.length > 1 ? color(si) : undefined }));
    showTip({ left: x - 4, right: x + 4, top: top - 4, bottom: top + 4 }, labels[i] ?? "", rows);
  }

  function leave() {
    hover = -1;
    hideTip();
  }
</script>

<div class="line-chart">
  {#if series.length > 1}
    <div class="legend">
      {#each series as s, si}
        <span class="key"><i style:background={color(si)} />{s.name}</span>
      {/each}
    </div>
  {/if}
  <div class="plot-wrap" bind:clientWidth={w} style:height="{height}px">
    {#if w > 0}
      <svg class="plot" width={w} {height} role="img" aria-label={aria} on:pointermove={move} on:pointerleave={leave}>
        <title>{aria}</title>
        {#each ticks as t, ti}
          <line class="grid" x1={left} x2={w - RIGHT} y1={Math.round(Y(t)) + 0.5} y2={Math.round(Y(t)) + 0.5} />
          <text class="tick" x={left - 8} y={Y(t)} text-anchor="end" dominant-baseline="middle">{tickText[ti]}</text>
        {/each}
        {#each xTicks as i}
          <text class="tick" x={X(i)} y={height - 6} text-anchor={n > 1 && i === 0 ? "start" : i === n - 1 && n > 1 ? "end" : "middle"}>{labels[i]}</text>
        {/each}
        {#each fills as d, si}<path class="wash" d={d} style:fill={color(si)} />{/each}
        {#if hover >= 0}
          <line class="cross" x1={X(hover)} x2={X(hover)} y1={TOP} y2={TOP + plotH} />
        {/if}
        {#each paths as d, si}<path class="stroke" d={d} style:stroke={color(si)} />{/each}
        {#if showDots}
          {#each series as s, si}
            {#each s.values as v, i}
              {#if v !== null}<circle class="pt" cx={X(i)} cy={Y(v)} r="4" style:fill={color(si)} />{/if}
            {/each}
          {/each}
        {/if}
        {#if hover >= 0}
          {#each series as s, si}
            {#if s.values[hover] !== null}<circle class="pt" cx={X(hover)} cy={Y(s.values[hover] ?? 0)} r="4" style:fill={color(si)} />{/if}
          {/each}
        {/if}
      </svg>
    {/if}
  </div>
  <DataTable
    caption={aria}
    head={["", ...series.map((s, si) => s.name || `Series ${si + 1}`)]}
    rows={labels.map((l, i) => [l, ...series.map((s) => (s.values[i] === null ? "" : fmt(s.values[i] ?? 0, unit)))])}
  />
</div>

<style>
  .line-chart {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 16px;
  }
  .key {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .key i {
    width: 12px;
    height: 2px;
    border-radius: 1px;
  }
  .plot-wrap {
    width: 100%;
    min-width: 0;
  }
  svg {
    display: block;
    overflow: visible;
    touch-action: pan-y;
  }
  .grid {
    stroke: var(--line);
    stroke-width: 1;
  }
  .tick {
    fill: var(--muted);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
  }
  .stroke {
    fill: none;
    stroke-width: 2;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .wash {
    fill-opacity: 0.1;
  }
  .cross {
    stroke: var(--faint);
    stroke-width: 1;
  }
  .pt {
    stroke: var(--panel);
    stroke-width: 2;
  }
</style>
