<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import * as echarts from "echarts/core";
  import { LineChart, BarChart, ScatterChart } from "echarts/charts";
  import {
    GridComponent,
    TooltipComponent,
    DataZoomComponent,
  } from "echarts/components";
  import { SVGRenderer } from "echarts/renderers";

  echarts.use([
    LineChart,
    BarChart,
    ScatterChart,
    GridComponent,
    TooltipComponent,
    DataZoomComponent,
    SVGRenderer,
  ]);

  export let option: any = null;
  export let height = 260;

  let el: HTMLDivElement | null = null;
  let chart: echarts.ECharts | null = null;
  let shown: any = null;

  export function snapshot(): string | null {
    if (!chart) return null;
    try {
      return chart.getDataURL({ type: "svg", backgroundColor: "transparent" });
    } catch {
      return null;
    }
  }

  onMount(() => {
    if (!el) return;
    chart = echarts.init(el, null, { renderer: "svg" });
    if (shown) chart.setOption(shown, true);
    const ro = new ResizeObserver(() => chart?.resize());
    ro.observe(el);
    return () => ro.disconnect();
  });

  $: if (chart && option && option !== shown) {
    shown = option;
    chart.setOption(option, true);
  }

  onDestroy(() => {
    chart?.dispose();
    chart = null;
  });
</script>

<div bind:this={el} class="echart" style={`height:${height}px`} />

<style>
  .echart {
    width: 100%;
    min-width: 0;
  }
</style>
