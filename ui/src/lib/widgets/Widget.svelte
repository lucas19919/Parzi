<script lang="ts">
  import { renderMarkdown } from "../md";
  import Zoomable from "./Zoomable.svelte";

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
  const known = ["stat", "progress", "list", "table", "chart-line", "chart-bar", "kanban", "markdown"];
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
  $: all = ser.flatMap((s) => s.points);
  $: maxLen = Math.max(1, ...ser.map((s) => s.points.length));
  $: SER_INK = ["var(--accent)", "var(--ok)", "var(--info)", "var(--warn)", "var(--bad)"];
  $: serInk = (i: number): string => SER_INK[i % SER_INK.length];
  $: showLegend = ser.length > 1 && ser.some((s) => s.name);
  $: xlabels = (() => {
    const raw = d.labels ?? d.payload?.labels;
    if (!Array.isArray(raw)) return [] as string[];
    return (raw as unknown[]).map((l) => String(l ?? "")).slice(0, 200);
  })();
  $: xlabel = String(d.xlabel ?? d.payload?.xlabel ?? "");
  $: ylabel = String(d.ylabel ?? d.payload?.ylabel ?? "");
  // Plot geometry with margins for ticks and axis labels. Zero is always
  // in range so bars grow from a true baseline and negatives stay on-canvas.
  $: plo = all.length
    ? (() => {
        const lo = Math.min(0, ...all);
        const hi = Math.max(0, ...all);
        const span = hi - lo || 1;
        const W = 320, H = 124, L = 36, R = 8, T = 8, B = 20;
        const y = (v: number) => T + (1 - (v - lo) / span) * (H - T - B);
        const x = (i: number) => L + (maxLen === 1 ? (W - L - R) / 2 : (i / (maxLen - 1)) * (W - L - R));
        const bw = (W - L - R) / Math.max(1, maxLen);
        const ticks = [0, 1, 2, 3].map((i) => lo + (span * i) / 3);
        return { lo, hi, span, W, H, L, R, T, B, y, x, bw, ticks };
      })()
    : null;
  $: fmtTick = (v: number): string => {
    const a = Math.abs(v);
    if (a >= 1_000_000) return `${+(v / 1_000_000).toFixed(1)}M`;
    if (a >= 10_000) return `${Math.round(v / 1000)}k`;
    if (a >= 100) return String(Math.round(v));
    if (a >= 1) return String(+v.toFixed(1));
    return String(+v.toFixed(2));
  };
  $: xshown = maxLen
    ? ser[0].points.map((_, i) => i).filter((i) => i % Math.ceil(maxLen / 6) === 0 || i === maxLen - 1)
    : [];
  $: xtext = (i: number): string =>
    (xlabels[i] ?? String(i + 1)).slice(0, 10);

  let plotEl: SVGSVGElement | null = null;
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
    const head = ["x", ...ser.map((s) => s.name || "value")];
    const lines = [head.join(",")];
    for (let i = 0; i < maxLen; i++) {
      lines.push([xlabels[i] ?? String(i + 1), ...ser.map((s) => (i < s.points.length ? String(s.points[i]) : ""))].join(","));
    }
    const blob = new Blob([lines.join("\n")], { type: "text/csv" });
    download(URL.createObjectURL(blob), `${slug(title)}.csv`);
  }
  function dlSvg() {
    if (!plotEl) return;
    const clone = plotEl.cloneNode(true) as SVGSVGElement;
    clone.setAttribute("xmlns", "http://www.w3.org/2000/svg");
    const blob = new Blob([new XMLSerializer().serializeToString(clone)], { type: "image/svg+xml" });
    download(URL.createObjectURL(blob), `${slug(title)}.svg`);
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
    <div class="w-md">{@html renderMarkdown(text)}</div>
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
  {:else if type === "chart-line" || type === "chart-bar"}
    {#if !all.length || !plo}
      <div class="w-empty">no data points</div>
    {:else}
      {#if showLegend}
        <div class="legend">
          {#each ser as s, si}
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
      <svg bind:this={plotEl} width="100%" viewBox="0 0 {plo.W} {plo.H}" preserveAspectRatio="xMidYMid meet" role="img" class="plot" class:zoomed={open}
        aria-label="Zoom chart" on:click={() => toggle()}>
        {#each plo.ticks as t}
          <line x1={plo.L} y1={plo.y(t)} x2={plo.W - plo.R} y2={plo.y(t)}
            stroke="var(--line-2)" stroke-width="1" />
          <text x={plo.L - 5} y={plo.y(t) + 3.5} fill="var(--text-4)" font-size="9" text-anchor="end">{fmtTick(t)}</text>
        {/each}
        {#if plo.lo < 0 && plo.hi > 0}
          <line x1={plo.L} y1={plo.y(0)} x2={plo.W - plo.R} y2={plo.y(0)}
            stroke="var(--line-3)" stroke-width="1" />
        {/if}
        {#if type === "chart-line"}
          {#each ser as s, si}
            <polyline
              fill="none" stroke={serInk(si)} stroke-width="2"
              points={s.points.map((p, i) => `${plo.x(i)},${plo.y(p)}`).join(" ")} />
            {#each s.points as p, i}
              <circle cx={plo.x(i)} cy={plo.y(p)} r="2.5" fill={serInk(si)} class="dot">
                <title>{s.name ? `${s.name} · ` : ""}{xtext(i)}: {p}</title>
              </circle>
            {/each}
          {/each}
        {:else}
          {#each ser as s, si}
            {#each s.points as p, i}
              {@const n = ser.length}
              {@const w = plo.bw / n}
              {@const bx = plo.L + i * plo.bw + si * w}
              {@const by = plo.y(Math.max(0, p))}
              {@const bh = Math.abs(plo.y(p) - plo.y(0))}
              <rect x={bx + 1} y={by} width={Math.max(2, w - 2)} height={Math.max(2, bh)}
                fill={serInk(si)} rx="2" class="bar">
                <title>{s.name ? `${s.name} · ` : ""}{xtext(i)}: {p}</title>
              </rect>
            {/each}
          {/each}
        {/if}
        {#each xshown as i}
          <text x={type === "chart-line" ? plo.x(i) : plo.L + i * plo.bw + plo.bw / 2} y={plo.H - 16}
            fill="var(--text-4)" font-size="9" text-anchor="middle">{xtext(i)}</text>
        {/each}
        {#if ylabel}<text x="10" y={plo.T + 30} fill="var(--text-4)" font-size="9" text-anchor="middle"
          transform="rotate(-90 10 {plo.T + 30})">{ylabel.slice(0, 24)}</text>{/if}
        {#if xlabel}<text x={(plo.L + plo.W - plo.R) / 2} y={plo.H - 4} fill="var(--text-4)"
          font-size="9" text-anchor="middle">{xlabel.slice(0, 32)}</text>{/if}
      </svg>
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
  .plot .dot, .plot .bar { transition: opacity 120ms ease; }
  .plot .dot:hover, .plot .bar:hover { opacity: 0.75; }
  .plot { cursor: zoom-in; }
  .plot.zoomed { cursor: zoom-out; }
  .legend { display: flex; flex-wrap: wrap; gap: 4px 12px; padding: 2px 0 4px; }
  .leg { display: inline-flex; align-items: center; gap: 5px; font-size: 11px; color: var(--text-3); }
  .leg i { width: 8px; height: 8px; border-radius: 2px; flex: none; }
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
