<script lang="ts">
  import { onDestroy, onMount } from "svelte";

  // echarts loads on first chart, off the startup path.
  type ECharts = import("echarts/core").ECharts;

  export let option: any = null;
  export let height = 260;

  let el: HTMLDivElement | null = null;
  let chart: ECharts | null = null;
  let shown: any = null;
  let failed = false;

  export function snapshot(): string | null {
    if (!chart) return null;
    try {
      return chart.getDataURL({ type: "svg", backgroundColor: "transparent" });
    } catch {
      return null;
    }
  }

  onMount(() => {
    let dead = false;
    void (async () => {
      try {
        const [core, charts, comps, renderers] = await Promise.all([
          import("echarts/core"),
          import("echarts/charts"),
          import("echarts/components"),
          import("echarts/renderers"),
        ]);
        if (dead || !el) return;
        core.use([
          charts.LineChart,
          charts.BarChart,
          charts.ScatterChart,
          comps.GridComponent,
          comps.TooltipComponent,
          comps.DataZoomComponent,
          renderers.SVGRenderer,
        ]);
        chart = core.init(el, null, { renderer: "svg" });
        if (shown) chart.setOption(shown, true);
        else if (option) {
          shown = option;
          chart.setOption(option, true);
        }
      } catch {
        if (!dead) failed = true;
      }
    })();
    const ro = new ResizeObserver(() => chart?.resize());
    if (el) ro.observe(el);
    return () => {
      dead = true;
      ro.disconnect();
    };
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

<div bind:this={el} class="echart" style={`height:${height}px`}>
  {#if failed}<span class="echart-err">Chart failed to load</span>{/if}
</div>

<style>
  .echart {
    width: 100%;
    min-width: 0;
  }
  .echart-err {
    font-size: 12px;
    color: var(--faint);
  }
</style>
