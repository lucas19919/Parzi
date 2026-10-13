<script lang="ts">
  // A small trend line: the run in the quiet grey, where it ends in the
  // accent. Hover reads a point.
  import { fmt } from "./spec";
  import { onDestroy } from "svelte";
  import { hideTip, showTip } from "./tip";
  import DataTable from "./DataTable.svelte";

  // A readout left up by a chart that went away goes with it.
  onDestroy(hideTip);

  export let values: number[] = [];
  export let height = 24;
  export let area = false;
  export let unit = "";
  export let label = "";

  const PAD = 4;
  let w = 0;
  let hover = -1;

  $: lo = Math.min(...values);
  $: hi = Math.max(...values);
  $: step = values.length > 1 ? (w - PAD * 2) / (values.length - 1) : 0;
  $: xs = values.map((_, i) => PAD + i * step);
  $: ys = values.map((v) => (hi === lo ? height / 2 : PAD + (1 - (v - lo) / (hi - lo)) * (height - PAD * 2)));
  $: line = xs.map((x, i) => `${i ? "L" : "M"}${x.toFixed(1)},${ys[i].toFixed(1)}`).join("");
  $: fill = area && xs.length ? `${line}L${xs[xs.length - 1].toFixed(1)},${height}L${PAD},${height}Z` : "";
  $: last = values.length - 1;
  $: aria = `${label ? `${label}: ` : ""}trend of ${values.length} values, last ${values.length ? fmt(values[last], unit) : ""}`;

  function move(e: PointerEvent) {
    const svg = e.currentTarget as SVGSVGElement;
    const r = svg.getBoundingClientRect();
    const i = Math.max(0, Math.min(last, Math.round((e.clientX - r.left - PAD) / (step || 1))));
    hover = i;
    const x = r.left + xs[i];
    const y = r.top + ys[i];
    showTip({ left: x - 4, right: x + 4, top: y - 4, bottom: y + 4 }, label, [{ value: fmt(values[i], unit), label: `${i + 1} of ${values.length}` }]);
  }

  function leave() {
    hover = -1;
    hideTip();
  }
</script>

<div class="spark" bind:clientWidth={w} style:height="{height}px">
  {#if w > 0 && values.length > 1}
    <svg width={w} {height} role="img" aria-label={aria} on:pointermove={move} on:pointerleave={leave}>
      <title>{aria}</title>
      {#if fill}<path class="area" d={fill} />{/if}
      <path class="run" d={line} />
      {#if hover >= 0}<circle class="dot" cx={xs[hover]} cy={ys[hover]} r="3" />{/if}
      <circle class="end" cx={xs[last]} cy={ys[last]} r="3" />
    </svg>
  {/if}
  <DataTable caption={aria} head={["Point", "Value"]} rows={values.map((v, i) => [String(i + 1), fmt(v, unit)])} />
</div>

<style>
  .spark {
    position: relative;
    width: 100%;
    min-width: 40px;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .run {
    fill: none;
    stroke: var(--muted);
    stroke-width: 1.5;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .area {
    fill: var(--muted);
    fill-opacity: 0.1;
  }
  .end {
    fill: var(--accent);
  }
  .dot {
    fill: var(--text);
    stroke: var(--panel);
    stroke-width: 2;
  }
</style>
