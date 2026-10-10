<script lang="ts">
  import { mdHtml, richReady } from "../md";
  import { handleLinkClick } from "../links";
  import { chartPalette, themeRev, type ChartPalette } from "../theme";
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
  // Only charts read theme colours, and they share one cached palette.
  const charted = ["chart-line", "chart-bar", "histogram", "scatter"].includes(type);
  const NO_PALETTE: ChartPalette = { ink: [""], line: "", faint: "", muted: "", panel: "", text: "" };
  $: pal = charted ? chartPalette($themeRev) : NO_PALETTE;
  $: INK = pal.ink;
  $: serInk = (i: number): string => INK[i % INK.length];
  $: showLegend = plotSer.length > 1 && plotSer.some((s) => s.name);
  $: axisCommon = {
    axisLine: { lineStyle: { color: pal.line } },
    axisTick: { show: false },
    axisLabel: { color: pal.faint, fontSize: 10 },
    splitLine: { lineStyle: { color: pal.line } },
  };
  $: tipStyle = {
    backgroundColor: pal.panel,
    borderColor: pal.line,
    borderWidth: 1,
    textStyle: { color: pal.text, fontSize: 11 },
  };
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
            nameTextStyle: { color: pal.faint, fontSize: 10 },
            ...axisCommon,
          },
          yAxis: {
            type: "value",
            name: ylabel,
            nameTextStyle: { color: pal.faint, fontSize: 10 },
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
                    color: pal.muted,
                    lineStyle: { width: 1.5, type: "dashed" },
                    showSymbol: false,
                  },
                ]
              : []),
          ],
        };
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
            nameTextStyle: { color: pal.faint, fontSize: 10 },
            ...axisCommon,
          },
          yAxis: {
            type: "value",
            scale: true,
            name: ylabel,
            nameTextStyle: { color: pal.faint, fontSize: 10 },
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
  {#if bad || title}
  <div class="w-head">
    {#if bad}<span class="w-kind">widget</span>{/if}
    {#if title}<span class="w-title">{title}</span>{/if}
  </div>
  {/if}
  {#if bad}
    <div class="w-error">Couldn't render widget type "{type}" — showing source.</div>
    <pre class="w-source">{JSON.stringify(d, null, 2)?.slice(0, 3000)}</pre>
  {:else if type === "stat"}
    <div class="stat-big">{text || value}</div>
    {#if d.sub}<div class="w-sub">{d.sub}</div>{/if}
  {:else if type === "progress"}
    <div class="progress-track"><div class="progress-fill" style="width:{Math.min(100, Math.max(0, Math.round(value * 100)))}%" /></div>
    <div class="w-sub">{Math.min(100, Math.max(0, Math.round(value * 100)))}%</div>
  {:else if type === "markdown"}
    <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
    <div class="w-md" on:click={(e) => void handleLinkClick(e)}>{@html mdHtml(text, $richReady)}</div>
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
    border: 1px solid var(--bad);
    border-radius: 12px;
    background: var(--panel);
    overflow: hidden;
  }
  .w-head { display: flex; align-items: center; gap: 8px; padding: 4px 0; font-size: 11px; }
  .w-kind { font-family: var(--mono), ui-monospace, monospace; font-size: 10px; color: var(--faint); }
  .w-title { font-weight: 600; color: var(--text); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .xbar { display: flex; gap: 2px; padding: 2px 0 4px; }
  .w-error { padding: 8px 12px; font-size: 11px; color: var(--bad); }
  .w-source { margin: 0 12px; padding: 8px; font-size: 10px; overflow: auto; background: var(--bg); border-radius: 6px; }
  .w-sub { padding: 4px 0; font-size: 11px; color: var(--muted); }
  .w-empty { padding: 8px 0; font-size: 11px; font-style: italic; color: var(--muted); }
  .w-md { padding: 6px 0; font-size: 13px; }
  .w-list { margin: 4px 0; padding-left: 18px; font-size: 12.5px; line-height: 1.6; }
  .stat-big { padding: 8px 0; font-size: 22px; font-weight: 700; letter-spacing: -0.02em; color: var(--text); }
  .progress-track { margin: 8px 0 4px; height: 8px; border-radius: 4px; background: var(--line); overflow: hidden; }
  .progress-fill { height: 100%; background: var(--accent); border-radius: 4px; transition: width 300ms ease; }
  .table-wrap { overflow-x: auto; margin: 8px 0 0; border-radius: 8px; border: 1px solid var(--line); }
  .ezoom { cursor: zoom-in; border-radius: 8px; }
  .ezoom:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .legend { display: flex; flex-wrap: wrap; gap: 4px 12px; padding: 2px 0 4px; }
  .leg { display: inline-flex; align-items: center; gap: 5px; font-size: 11px; color: var(--muted); }
  .leg i { width: 8px; height: 8px; border-radius: 2px; flex: none; }
  .leg.trend-leg { color: var(--faint); }
  .trend-sw { width: 14px !important; height: 0 !important; border-radius: 0 !important; border-top: 2px dashed var(--muted); }
  .xbtn {
    background: transparent; border: none; border-radius: 4px;
    color: var(--faint); font-family: var(--mono), ui-monospace, monospace;
    font-size: 10px; padding: 2px 6px; cursor: pointer; flex: none;
  }
  .xbtn:hover { color: var(--text); background: var(--panel); }
  table { width: 100%; border-collapse: collapse; font-size: 11.5px; }
  th, td { padding: 6px 10px; border-bottom: 1px solid var(--line); text-align: left; }
  th { color: var(--muted); font-weight: 600; background: var(--panel); }
  .kanban { display: flex; gap: 8px; padding: 6px 0 0; overflow-x: auto; }
  .kcol { flex: 1; min-width: 140px; }
  .pill { display: block; margin: 4px 0; background: transparent; border: none; border-left: 2px solid var(--line); border-radius: 0; padding: 3px 8px; font-size: 11px; }
</style>
