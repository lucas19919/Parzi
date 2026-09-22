<script lang="ts">
  import { renderMarkdown } from "../md";
  import { handleLinkClick } from "../links";
  import Zoomable from "./Zoomable.svelte";
  import EChart from "./EChart.svelte";

  export let data: any;
  const d = data && typeof data === "object" ? data : {};
  const arr = (v: unknown): any[] => (Array.isArray(v) ? v : []);
  const type: string = String(d.type ?? "stat");
  const title: string = String(d.title ?? "");
  const rows: any[][] = arr(d.rows ?? d.payload?.rows);
  const cols: string[] = arr(d.columns ?? d.payload?.columns);
  const items: any[] = arr(d.items ?? d.list ?? d.payload?.items);
  const value: number = Number(d.value ?? 0);
  const text: string = String(d.text ?? d.title ?? d.payload?.text ?? "");
  const cardOf = (col: any): any[] => (col && typeof col === "object" ? arr(col.cards) : []);
  const colName = (col: any): string =>
    String(col?.title ?? col?.id ?? "col");
  const known = ["stat", "progress", "list", "table", "chart-line", "chart-bar", "histogram", "scatter", "kanban", "markdown"];
  const bad = !known.includes(type);
  $: isChart = type === "chart-line" || type === "chart-bar";
  $: pts = Array.isArray(d.points)
    ? (d.points as number[]).map(Number).filter((n) => Number.isFinite(n)).slice(0, 200)
    : Array.isArray(d.payload?.points)
      ? (d.payload.points as number[]).map(Number).filter((n) => Number.isFinite(n)).slice(0, 200)
      : [];
  /** Multi-series form: {series:[{name, points[]}]}; falls back to one. */
  $: ser = ((): { name: string; points: number[] }[] => {
    const raw = d.series ?? d.payload?.series;
    if (!Array.isArray(raw)) return [{ name: "", points: pts }];
    const out = raw
      .filter((s: any) => s && typeof s === "object")
      .slice(0, 8)
      .map((s: any) => ({
        name: String(s.name ?? ""),
        points: (Array.isArray(s.points) ? s.points : [])
          .map(Number)
          .filter((n: number) => Number.isFinite(n))
          .slice(0, 200),
      }))
      .filter((s: { points: number[] }) => s.points.length);
    return out.length ? out : [{ name: "", points: pts }];
  })();
  $: all = plotSer.flatMap((s) => s.points);
  $: maxLen = Math.max(1, ...plotSer.map((s) => s.points.length));
  function cssVar(n: string, fb: string): string {
    try {
      return getComputedStyle(document.documentElement).getPropertyValue(n).trim() || fb;
    } catch {
      return fb;
    }
  }
  $: INK = [
    cssVar("--accent", "#7C8CFF"),
    cssVar("--ok", "#22c55e"),
    cssVar("--info", "#5eb1ff"),
    cssVar("--warn", "#f59e0b"),
    cssVar("--bad", "#ef4444"),
  ];
  $: serInk = (i: number): string => INK[i % INK.length];
  $: showLegend = plotSer.length > 1 && plotSer.some((s) => s.name);
  $: axisCommon = {
    axisLine: { lineStyle: { color: cssVar("--line-2", "#333") } },
    axisTick: { show: false },
    axisLabel: { color: cssVar("--text-4", "#999"), fontSize: 10 },
    splitLine: { lineStyle: { color: cssVar("--line-2", "#333") } },
  };
  $: tipStyle = {
    backgroundColor: cssVar("--parzi-bar", "#14141a"),
    borderColor: cssVar("--line-2", "#333"),
    borderWidth: 1,
    textStyle: { color: cssVar("--text", "#eee"), fontSize: 11 },
  };
  /** ECharts option for line/bar/histogram from the normalized series. */
  $: chartOpt = !all.length
    ? null
    : {
          animation: false,
          grid: { left: 6, right: 10, top: 12, bottom: 2, containLabel: true },
          tooltip: { trigger: "axis", ...tipStyle },
          dataZoom: [{ type: "inside", xAxisIndex: 0 }],
          xAxis: {
            type: "category",
            data: Array.from({ length: maxLen }, (_, i) => plotLabels[i] ?? String(i + 1)),
            name: xlabel,
            nameLocation: "middle",
            nameGap: 22,
            nameTextStyle: { color: cssVar("--text-4", "#999"), fontSize: 10 },
            ...axisCommon,
          },
          yAxis: {
            type: "value",
            name: ylabel,
            nameTextStyle: { color: cssVar("--text-4", "#999"), fontSize: 10 },
            ...axisCommon,
          },
          series: [
            ...plotSer.map((s, si) => ({
              name: s.name || (plotSer.length > 1 ? `series ${si + 1}` : "value"),
              type: plotType === "chart-line" ? "line" : "bar",
              data: s.points,
              color: INK[si % INK.length],
              ...(plotType === "chart-line"
                ? { showSymbol: false, symbolSize: 6, lineStyle: { width: 2 } }
                : { barMaxWidth: 26 }),
            })),
            ...(trend
              ? [
                  {
                    name: "trend",
                    type: "line",
                    data: Array.from({ length: trend.n }, (_, i) => trend.m * i + trend.b),
                    color: cssVar("--text-3", "#999"),
                    lineStyle: { width: 1.5, type: "dashed" },
                    showSymbol: false,
                  },
                ]
              : []),
          ],
        };
  /** ECharts option for scatter (both axes fit the data). */
  $: scatterOpt = scSer.some((s) => s.points.length)
      ? {
          animation: false,
          grid: { left: 6, right: 10, top: 12, bottom: 2, containLabel: true },
          tooltip: { ...tipStyle },
          dataZoom: [{ type: "inside" }, { type: "inside", orient: "vertical" }],
          xAxis: {
            type: "value",
            name: xlabel,
            nameLocation: "middle",
            nameGap: 22,
            nameTextStyle: { color: cssVar("--text-4", "#999"), fontSize: 10 },
            ...axisCommon,
          },
          yAxis: {
            type: "value",
            scale: true,
            name: ylabel,
            nameTextStyle: { color: cssVar("--text-4", "#999"), fontSize: 10 },
            ...axisCommon,
          },
          series: scSer
            .filter((s) => s.points.length)
            .map((s, si) => ({
              name: s.name || `series ${si + 1}`,
              type: "scatter",
              data: s.points,
              color: INK[si % INK.length],
              symbolSize: 7,
            })),
        }
      : null;
  $: xlabels = (() => {
    const raw = d.labels ?? d.payload?.labels;
    if (!Array.isArray(raw)) return [] as string[];
    return (raw as unknown[]).map((l) => String(l ?? "")).slice(0, 200);
  })();
  $: xlabel = String(d.xlabel ?? d.payload?.xlabel ?? "");
  $: ylabel = String(d.ylabel ?? d.payload?.ylabel ?? "");
  function fmtTick(v: number): string {
    const a = Math.abs(v);
    if (a >= 1_000_000) return `${+(v / 1_000_000).toFixed(1)}M`;
    if (a >= 10_000) return `${Math.round(v / 1000)}k`;
    if (a >= 100) return String(Math.round(v));
    if (a >= 1) return String(+v.toFixed(1));
    return String(+v.toFixed(2));
  }

  /** Histogram bins raw values into counts; renders through the bar plot. */
  function binValues(vals: number[], nb: number) {
    if (!vals.length) return { counts: [] as number[], labels: [] as string[] };
    let lo = Math.min(...vals);
    let hi = Math.max(...vals);
    if (lo === hi) {
      lo -= 0.5;
      hi += 0.5;
    }
    const k = Math.min(24, Math.max(2, nb || Math.ceil(Math.sqrt(vals.length))));
    const w = (hi - lo) / k;
    const counts = new Array(k).fill(0) as number[];
    for (const v of vals) counts[Math.min(k - 1, Math.floor((v - lo) / w))]++;
    return { counts, labels: counts.map((_, i) => fmtTick(lo + w * i)) };
  }
  $: histVals = (() => {
    const raw = d.values ?? d.payload?.values;
    if (!Array.isArray(raw)) return [] as number[];
    return raw.map(Number).filter((n: number) => Number.isFinite(n)).slice(0, 2000);
  })();
  $: histBins = Math.round(Number(d.bins ?? d.payload?.bins ?? 0)) || 0;
  $: hist = type === "histogram" ? binValues(histVals, histBins) : null;
  $: plotSer = hist ? [{ name: "", points: hist.counts }] : ser;
  $: plotLabels = hist ? hist.labels : xlabels;
  $: plotType = type === "histogram" ? "chart-bar" : type;

  /** Scatter pairs (and per-series pairs); both axes fit the data. */
  $: scSer = (() => {
    const toPairs = (v: unknown): [number, number][] => {
      if (!Array.isArray(v)) return [];
      const out: [number, number][] = [];
      for (const p of (v as unknown[]).slice(0, 200)) {
        if (Array.isArray(p) && p.length === 2) {
          const x = Number(p[0]);
          const y = Number(p[1]);
          if (Number.isFinite(x) && Number.isFinite(y)) out.push([x, y]);
        }
      }
      return out;
    };
    const raw = d.series ?? d.payload?.series;
    if (Array.isArray(raw)) {
      const out = raw
        .filter((s: any) => s && typeof s === "object")
        .slice(0, 8)
        .map((s: any) => ({ name: String(s.name ?? ""), points: toPairs(s.points) }))
        .filter((s: { points: [number, number][] }) => s.points.length);
      if (out.length) return out;
    }
    return [{ name: "", points: toPairs(d.points ?? d.payload?.points) }];
  })();

  /** Least-squares trend for the first line series (chart-line + trend). */
  $: trend =
    type === "chart-line" && (d.trend ?? d.payload?.trend) && ser[0]?.points.length > 1
      ? (() => {
          const ys = ser[0].points;
          const n = ys.length;
          let sx = 0, sy = 0, sxx = 0, sxy = 0;
          ys.forEach((y, i) => {
            sx += i;
            sy += y;
            sxx += i * i;
            sxy += i * y;
          });
          const den = n * sxx - sx * sx;
          if (!den) return null;
          const m = (n * sxy - sx * sy) / den;
          return { m, b: (sy - m * sx) / n, n };
        })()
      : null;
  $: trendEq = trend
    ? `y = ${fmtTick(trend.m)}·x ${trend.b < 0 ? "−" : "+"} ${fmtTick(Math.abs(trend.b))}`
    : "";

  type EchRef = { snapshot: () => string | null } | null;
  let echLine: EchRef = null;
  let echScatter: EchRef = null;
  function slug(s: string): string {
    const t = s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 40);
    return t || "chart";
  }
  function download(url: string, name: string) {
    const a = document.createElement("a");
    a.href = url;
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 2000);
  }
  function dlCsv() {
    if (type === "scatter") {
      const lines = ["series,x,y"];
      for (const s of scSer) {
        for (const p of s.points) lines.push([s.name || "value", String(p[0]), String(p[1])].join(","));
      }
      const blob = new Blob([lines.join("\n")], { type: "text/csv" });
      download(URL.createObjectURL(blob), `${slug(title)}.csv`);
      return;
    }
    const head = ["x", ...plotSer.map((s) => s.name || "value")];
    const lines = [head.join(",")];
    for (let i = 0; i < maxLen; i++) {
      lines.push([plotLabels[i] ?? String(i + 1), ...plotSer.map((s) => (i < s.points.length ? String(s.points[i]) : ""))].join(","));
    }
    const blob = new Blob([lines.join("\n")], { type: "text/csv" });
    download(URL.createObjectURL(blob), `${slug(title)}.csv`);
  }
  function dlSvg() {
    const url = (type === "scatter" ? echScatter : echLine)?.snapshot();
    if (url) download(url, `${slug(title)}.svg`);
  }
  $: shownRows = rows.slice(0, 50);
  $: truncated = rows.length - shownRows.length;
  $: kanbanCols = arr(d.columns);
</script>

<Zoomable enabled={isChart} let:open let:toggle>
<div class="widget-card" class:bad>
  <div class="w-head">
    <span class="w-kind">{bad ? "widget" : type}</span>
    {#if title}<span class="w-title">{title}</span>{/if}
  </div>
  {#if bad}
    <div class="w-error">Couldn't render widget type "{type}" — showing source.</div>
    <pre class="w-source">{JSON.stringify(d, null, 2)?.slice(0, 3000)}</pre>
  {:else if type === "stat" || type === undefined}
    <div class="stat-big">{text || value}</div>
    {#if d.sub}<div class="w-sub">{d.sub}</div>{/if}
  {:else if type === "progress"}
    <div class="progress-track"><div class="progress-fill" style="width:{Math.min(100, Math.max(0, Math.round(value * 100)))}%" /></div>
    <div class="w-sub">{Math.min(100, Math.max(0, Math.round(value * 100)))}%</div>
  {:else if type === "markdown"}
    <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
    <div class="w-md" on:click={(e) => void handleLinkClick(e)}>{@html renderMarkdown(text)}</div>
  {:else if type === "list"}
    {#if !items.length}
      <div class="w-empty">empty list</div>
    {:else}
      <ul class="w-list">{#each items.slice(0, 50) as it}<li>{typeof it === "string" ? it : JSON.stringify(it)}</li>{/each}</ul>
    {/if}
  {:else if type === "table"}
    {#if !rows.length}
      <div class="w-empty">empty table</div>
    {:else}
      <div class="table-wrap">
      <table>
        {#if cols.length}<tr>{#each cols as c}<th>{c}</th>{/each}</tr>{/if}
        {#each shownRows as r}<tr>{#each Array.isArray(r) ? r : [r] as c}<td>{String(c)}</td>{/each}</tr>{/each}
      </table>
      </div>
      {#if truncated > 0}<div class="w-sub">{truncated} more rows truncated (max 50)</div>{/if}
    {/if}
  {:else if type === "chart-line" || type === "chart-bar" || type === "histogram"}
    {#if !chartOpt}
      <div class="w-empty">no data points</div>
    {:else}
      {#if showLegend}
        <div class="legend">
          {#each plotSer as s, si}
            {#if s.name}<span class="leg"><i style={`background:${serInk(si)}`} />{s.name}</span>{/if}
          {/each}
          {#if trend}<span class="leg trend-leg"><i class="trend-sw" />{trendEq}</span>{/if}
        </div>
      {:else if trend}
        <div class="legend"><span class="leg trend-leg"><i class="trend-sw" />{trendEq}</span></div>
      {/if}
      {#if open}
        <div class="xbar">
          <button class="xbtn" on:click={dlCsv} title="Download CSV">csv</button>
          <button class="xbtn" on:click={dlSvg} title="Download SVG">svg</button>
        </div>
      {/if}
      <div class="ezoom" role="button" tabindex="0" aria-label="Zoom chart"
        on:click={() => toggle()} on:keydown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggle(); } }}>
        <EChart bind:this={echLine} option={chartOpt} height={open ? 420 : 260} />
      </div>
    {/if}
  {:else if type === "scatter"}
    {#if !scatterOpt}
      <div class="w-empty">no data points</div>
    {:else}
      {#if scSer.length > 1 && scSer.some((s) => s.name)}
        <div class="legend">
          {#each scSer as s, si}
            {#if s.name}<span class="leg"><i style={`background:${serInk(si)}`} />{s.name}</span>{/if}
          {/each}
        </div>
      {/if}
      {#if open}
        <div class="xbar">
          <button class="xbtn" on:click={dlCsv} title="Download CSV">csv</button>
          <button class="xbtn" on:click={dlSvg} title="Download SVG">svg</button>
        </div>
      {/if}
      <div class="ezoom" role="button" tabindex="0" aria-label="Zoom chart"
        on:click={() => toggle()} on:keydown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggle(); } }}>
        <EChart bind:this={echScatter} option={scatterOpt} height={open ? 420 : 260} />
      </div>
    {/if}
  {:else if type === "kanban"}
    {#if !kanbanCols.length}
      <div class="w-empty">no columns</div>
    {:else}
      <div class="kanban">
        {#each kanbanCols.slice(0, 8) as col}
          <div class="kcol">
            <div class="w-sub">{colName(col)}</div>
            {#each cardOf(col).slice(0, 20) as card}<div class="pill">{typeof card === "string" ? card : JSON.stringify(card)}</div>{/each}
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>
</Zoomable>

<style>
  .widget-card {
    background: transparent;
    border: none;
    border-radius: 0;
    overflow: visible;
    padding-bottom: 6px;
  }
  .widget-card.bad {
    border: 1px solid var(--bad-line);
    border-radius: 12px;
    background: var(--surface-1);
    overflow: hidden;
  }
  .w-head { display: flex; align-items: center; gap: 8px; padding: 4px 0; font-size: 11px; }
  .w-kind { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10px; color: var(--text-4); }
  .w-title { font-weight: 600; color: var(--text); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .xbar { display: flex; gap: 2px; padding: 2px 0 4px; }
  .w-error { padding: 8px 12px; font-size: 11px; color: var(--bad); }
  .w-source { margin: 0 12px; padding: 8px; font-size: 10px; overflow: auto; background: var(--input); border-radius: 6px; }
  .w-sub { padding: 4px 0; font-size: 11px; color: var(--text-3); }
  .w-empty { padding: 8px 0; font-size: 11px; font-style: italic; color: var(--text-3); }
  .w-md { padding: 6px 0; font-size: 13px; }
  .w-list { margin: 4px 0; padding-left: 18px; font-size: 12.5px; line-height: 1.6; }
  .stat-big { padding: 8px 0; font-size: 22px; font-weight: 700; color: var(--text); }
  .progress-track { margin: 8px 0 4px; height: 8px; border-radius: 4px; background: var(--surface-3); overflow: hidden; }
  .progress-fill { height: 100%; background: var(--accent); border-radius: 4px; transition: width 300ms ease; }
  .table-wrap { overflow-x: auto; margin: 8px 0 0; border-radius: 8px; border: 1px solid var(--line-2); }
  .ezoom { cursor: zoom-in; border-radius: 8px; }
  .ezoom:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .legend { display: flex; flex-wrap: wrap; gap: 4px 12px; padding: 2px 0 4px; }
  .leg { display: inline-flex; align-items: center; gap: 5px; font-size: 11px; color: var(--text-3); }
  .leg i { width: 8px; height: 8px; border-radius: 2px; flex: none; }
  .leg.trend-leg { color: var(--text-4); }
  .trend-sw { width: 14px !important; height: 0 !important; border-radius: 0 !important; border-top: 2px dashed var(--text-3); }
  .xbtn {
    background: transparent; border: none; border-radius: 4px;
    color: var(--text-4); font-family: var(--parzi-mono), ui-monospace, monospace;
    font-size: 10px; padding: 2px 6px; cursor: pointer; flex: none;
  }
  .xbtn:hover { color: var(--text); background: var(--surface-1); }
  table { width: 100%; border-collapse: collapse; font-size: 11.5px; }
  th, td { padding: 6px 10px; border-bottom: 1px solid var(--line-2); text-align: left; }
  th { color: var(--text-3); font-weight: 600; background: var(--surface-1); }
  .kanban { display: flex; gap: 8px; padding: 6px 0 0; overflow-x: auto; }
  .kcol { flex: 1; min-width: 140px; }
  .pill { display: block; margin: 4px 0; background: transparent; border: none; border-left: 2px solid var(--line-3); border-radius: 0; padding: 3px 8px; font-size: 11px; }
</style>
