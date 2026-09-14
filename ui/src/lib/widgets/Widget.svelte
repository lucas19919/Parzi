<script lang="ts">
  import { renderMarkdown } from "../md";

  export let data: any;
  const d = data ?? {};
  const type: string = String(d.type ?? "stat");
  const title: string = String(d.title ?? "");
  const rows: any[][] = d.rows ?? d.payload?.rows ?? [];
  const cols: string[] = d.columns ?? d.payload?.columns ?? [];
  const items: any[] = d.items ?? d.list ?? d.payload?.items ?? [];
  const value: number = Number(d.value ?? 0);
  const text: string = String(d.text ?? d.title ?? d.payload?.text ?? "");
  const known = ["stat", "progress", "list", "table", "chart-line", "chart-bar", "kanban", "markdown"];
  const bad = !known.includes(type);
  $: pts = Array.isArray(d.points)
    ? (d.points as number[]).map(Number).filter((n) => Number.isFinite(n)).slice(0, 200)
    : Array.isArray(d.payload?.points)
      ? (d.payload.points as number[]).map(Number).filter((n) => Number.isFinite(n)).slice(0, 200)
      : [];
  $: pmax = pts.length ? Math.max(1, ...pts) : 1;
  $: shownRows = rows.slice(0, 50);
  $: truncated = rows.length - shownRows.length;
  $: kanbanCols = Array.isArray(d.columns) ? d.columns : [];
</script>

<div class="widget-card" class:bad>
  <div class="w-head">
    <span class="w-kind">{bad ? "widget" : type}</span>
    {#if title}<span class="w-title">{title}</span>{/if}
    {#if type === "table" && rows.length}<span class="w-meta">{rows.length} rows</span>{/if}
    {#if (type === "chart-line" || type === "chart-bar") && pts.length}<span class="w-meta">{pts.length} pts</span>{/if}
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
    {#if !pts.length}
      <div class="w-empty">no data points</div>
    {:else}
      <svg width="100%" height="90" viewBox="0 0 300 90" preserveAspectRatio="none" role="img">
        {#if type === "chart-line"}
          <polyline
            fill="none" stroke="var(--accent)" stroke-width="2"
            points={pts.map((p, i) => `${(i / Math.max(1, pts.length - 1)) * 300},${90 - (p / pmax) * 80}`).join(" ")} />
        {:else}
          {#each pts as p, i}
            <rect x={i * (300 / Math.max(1, pts.length)) + 2} y={90 - (p / pmax) * 80}
              width={Math.max(2, 300 / Math.max(1, pts.length) - 4)} height={(p / pmax) * 80}
              fill="var(--accent)" rx="2" />
          {/each}
        {/if}
      </svg>
    {/if}
  {:else if type === "kanban"}
    {#if !kanbanCols.length}
      <div class="w-empty">no columns</div>
    {:else}
      <div class="kanban">
        {#each kanbanCols.slice(0, 8) as col}
          <div class="kcol">
            <div class="w-sub">{col.title ?? col.id ?? "col"}</div>
            {#each (col.cards ?? []).slice(0, 20) as card}<div class="pill">{typeof card === "string" ? card : JSON.stringify(card)}</div>{/each}
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .widget-card {
    background: var(--surface-1);
    border: 1px solid var(--line-2);
    border-radius: 12px;
    overflow: hidden;
    padding-bottom: 10px;
  }
  .widget-card.bad { border-color: var(--bad-line); }
  .w-head { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-bottom: 1px solid var(--line-2); font-size: 11px; }
  .w-kind { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10px; color: var(--accent); background: var(--accent-soft); border-radius: 4px; padding: 2px 6px; }
  .w-title { font-weight: 600; color: var(--text); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .w-meta { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10px; color: var(--text-3); margin-left: auto; }
  .w-error { padding: 8px 12px; font-size: 11px; color: var(--bad); }
  .w-source { margin: 0 12px; padding: 8px; font-size: 10px; overflow: auto; background: var(--input); border-radius: 6px; }
  .w-sub { padding: 4px 12px; font-size: 11px; color: var(--text-3); }
  .w-empty { padding: 12px; font-size: 11px; font-style: italic; color: var(--text-3); }
  .w-md { padding: 10px 12px; font-size: 13px; }
  .w-list { margin: 6px 12px; padding-left: 18px; font-size: 12.5px; line-height: 1.6; }
  .stat-big { padding: 12px; font-size: 22px; font-weight: 700; color: var(--text); }
  .progress-track { margin: 12px 12px 4px; height: 8px; border-radius: 4px; background: var(--surface-3); overflow: hidden; }
  .progress-fill { height: 100%; background: var(--accent); border-radius: 4px; transition: width 300ms ease; }
  .table-wrap { overflow-x: auto; margin: 8px 12px 0; border-radius: 8px; border: 1px solid var(--line-2); }
  table { width: 100%; border-collapse: collapse; font-size: 11.5px; }
  th, td { padding: 6px 10px; border-bottom: 1px solid var(--line-2); text-align: left; }
  th { color: var(--text-3); font-weight: 600; background: var(--surface-1); }
  .kanban { display: flex; gap: 8px; padding: 10px 12px 0; overflow-x: auto; }
  .kcol { flex: 1; min-width: 140px; }
  .pill { display: block; margin: 4px 0; background: var(--surface-2); border: 1px solid var(--line-2); border-radius: 6px; padding: 5px 8px; font-size: 11px; }
</style>
