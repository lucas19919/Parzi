<script lang="ts">
  export let data: any;
  const rawNodes: any[] = Array.isArray(data?.nodes) ? data.nodes : [];
  const rawEdges: any[] = Array.isArray(data?.edges) ? data.edges : [];
  // Normalize so malformed payloads degrade to the error card, never throw.
  const nodes: { id: string; label: string; color: string }[] = rawNodes
    .filter((n) => n && (typeof n.id === "string" || typeof n.id === "number") && String(n.id).trim())
    .map((n) => ({
      id: String(n.id),
      label: typeof n.label === "string" || typeof n.label === "number" ? String(n.label) : "",
      color: typeof n.color === "string" ? n.color.toLowerCase() : "",
    }));
  const edges: { from: string; to: string; label: string }[] = rawEdges
    .filter((e) => e && (typeof e.from === "string" || typeof e.from === "number") && (typeof e.to === "string" || typeof e.to === "number"))
    .map((e) => ({
      from: String(e.from),
      to: String(e.to),
      label: typeof e.label === "string" || typeof e.label === "number" ? String(e.label) : "",
    }));
  const title: string = String(data?.title ?? "");
  const vertical = String(data?.direction ?? "LR").toUpperCase() === "TB";
  const ids = new Set(nodes.map((n) => n.id));
  const dupes = ids.size !== nodes.length;
  const dangling = edges.some((e) => !ids.has(e.from) || !ids.has(e.to));
  const bad =
    !nodes.length || nodes.length > 200 || edges.length > 400 || dupes || dangling;

  const INK: Record<string, string> = {
    accent: "var(--accent)",
    ok: "var(--ok)",
    warn: "var(--warn)",
    bad: "var(--bad)",
    info: "var(--info)",
  };
  const inkOf = (c: string): string | null => INK[c] ?? null;
  const short = (s: string, n: number): string =>
    s.length > n ? s.slice(0, n - 1) + "…" : s;

  // Depth via BFS from roots; cycle-safe with visited set. Adjacency is
  // built once so layout stays linear in nodes + edges.
  const depth = new Map<string, number>();
  const incoming = new Map<string, number>();
  const adj = new Map<string, typeof edges>();
  for (const n of nodes) incoming.set(n.id, 0);
  for (const e of edges) {
    incoming.set(e.to, (incoming.get(e.to) ?? 0) + 1);
    const list = adj.get(e.from) ?? [];
    list.push(e);
    adj.set(e.from, list);
  }
  const queue: string[] = nodes.filter((n) => (incoming.get(n.id) ?? 0) === 0).map((n) => n.id);
  if (!queue.length && nodes.length) queue.push(nodes[0].id);
  for (const q of queue) depth.set(q, 0);
  const visit = [...queue];
  const seen = new Set<string>();
  while (visit.length) {
    const cur = visit.shift()!;
    if (seen.has(cur)) continue;
    seen.add(cur);
    for (const e of adj.get(cur) ?? []) {
      const d = (depth.get(cur) ?? 0) + 1;
      if (d > (depth.get(e.to) ?? -1)) depth.set(e.to, d);
      visit.push(e.to);
    }
  }
  const cols = new Map<number, typeof nodes>();
  for (const n of nodes) {
    const d = depth.get(n.id) ?? 0;
    if (!cols.has(d)) cols.set(d, []);
    cols.get(d)!.push(n);
  }
  const CW = 170, CH = 54, GX = 70, GY = 18;
  const lanes = [...cols.entries()].sort((a, b) => a[0] - b[0]);
  const maxRows = Math.max(1, ...lanes.map(([, c]) => c.length));
  const W = vertical ? maxRows * (CW + GX) + 10 : Math.max(1, lanes.length * (CW + GX)) + 10;
  const H = vertical ? Math.max(1, lanes.length * (CH + GY)) + 10 : maxRows * (CH + GY) + 10;
  const pos = new Map<string, { x: number; y: number }>();
  lanes.forEach(([d, list], ci) => {
    list.forEach((n, ri) => {
      pos.set(
        n.id,
        vertical
          ? { x: ri * (CW + GX) + 8, y: ci * (CH + GY) + 8 }
          : { x: ci * (CW + GX) + 8, y: ri * (CH + GY) + 8 },
      );
    });
  });

  // Inspect: zoom buttons + drag pan around a transformed group.
  let k = 1, px = 0, py = 0;
  let panning = false, sx = 0, sy = 0, ox = 0, oy = 0;
  function zoom(f: number) {
    k = Math.min(2.5, Math.max(0.5, +(k * f).toFixed(2)));
  }
  function resetView() {
    k = 1;
    px = 0;
    py = 0;
  }
  function panStart(e: PointerEvent) {
    if (e.button !== 0) return;
    panning = true;
    sx = e.clientX;
    sy = e.clientY;
    ox = px;
    oy = py;
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
  }
  function panMove(e: PointerEvent) {
    if (!panning) return;
    px = ox + (e.clientX - sx);
    py = oy + (e.clientY - sy);
  }
  function panEnd() {
    panning = false;
  }
  /** Edge endpoints on node borders, in flow direction. */
  function ends(a: { x: number; y: number }, b: { x: number; y: number }) {
    return vertical
      ? { x1: a.x + CW / 2, y1: a.y + CH, x2: b.x + CW / 2, y2: b.y }
      : { x1: a.x + CW, y1: a.y + CH / 2, x2: b.x, y2: b.y + CH / 2 };
  }
</script>

<div class="widget-card" class:bad>
  <div class="w-head">
    <span class="w-kind">diagram</span>
    {#if title}<span class="w-title">{title}</span>{/if}
    <span class="w-meta">{nodes.length} nodes · {edges.length} edges</span>
  </div>
  {#if bad}
    <div class="w-error">
      {#if !nodes.length}Empty diagram — needs at least 1 node.
      {:else if nodes.length > 200}Too many nodes ({nodes.length}/200 max).
      {:else if edges.length > 400}Too many edges ({edges.length}/400 max).
      {:else if dupes}Duplicate node ids.
      {:else}Edges point at unknown nodes.{/if}
      Showing source.
    </div>
    <pre class="w-source">{JSON.stringify(data, null, 2)?.slice(0, 3000)}</pre>
  {:else}
    <div class="dz-tools" role="toolbar" aria-label="Diagram view">
      <button class="dz-btn" on:click={() => zoom(1.25)} title="Zoom in">+</button>
      <button class="dz-btn" on:click={() => zoom(0.8)} title="Zoom out">−</button>
      <button class="dz-btn wide" on:click={resetView} title="Reset view">reset</button>
    </div>
    <svg width="100%" viewBox="0 0 {W} {H}" role="img"
      on:pointerdown={panStart} on:pointermove={panMove} on:pointerup={panEnd} on:pointerleave={panEnd}>
      <defs>
        <marker id="dz-arrow" viewBox="0 0 10 10" refX="8" refY="5"
          markerWidth="7" markerHeight="7" orient="auto-start-reverse">
          <path d="M0 0L10 5L0 10z" fill="var(--text-3)" />
        </marker>
      </defs>
      <g transform="translate({px} {py}) scale({k})">
      {#each edges as e}
        {@const a = pos.get(e.from)}
        {@const b = pos.get(e.to)}
        {#if a && b}
          {@const p = ends(a, b)}
          <line x1={p.x1} y1={p.y1} x2={p.x2} y2={p.y2}
            stroke="var(--line)" stroke-width="2" marker-end="url(#dz-arrow)" />
          {#if e.label}
            <text x={(p.x1 + p.x2) / 2} y={(p.y1 + p.y2) / 2 - 5} fill="var(--text-3)"
              font-size="10" text-anchor="middle" class="edge-label">{short(e.label, 28)}<title>{e.label}</title></text>
          {/if}
        {/if}
      {/each}
      {#each nodes as n}
        {@const p = pos.get(n.id)}
        {#if p}
          {@const ink = inkOf(n.color)}
          <rect x={p.x} y={p.y} width={CW} height={CH} rx="10"
            fill="var(--surface-1)" stroke={ink ?? "var(--line)"} stroke-width={ink ? 2 : 1.5} />
          {#if ink}
            <rect x={p.x} y={p.y + 10} width={3} height={CH - 20} rx="1.5" fill={ink} />
          {/if}
          <text x={p.x + (ink ? 16 : 12)} y={p.y + CH / 2 + 5} fill="var(--text)" font-size="13">
            {short(n.label || n.id, 20)}<title>{n.label || n.id}</title>
          </text>
        {/if}
      {/each}
      </g>
    </svg>
  {/if}
</div>

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
  .w-title { font-weight: 600; color: var(--text); font-size: 12px; }
  .w-meta { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10px; color: var(--text-3); margin-left: auto; }
  .dz-tools { display: flex; gap: 4px; padding: 6px 0 2px; }
  .dz-btn {
    background: transparent; border: 1px solid transparent; border-radius: 5px;
    color: var(--text-4); font: inherit; font-size: 11px; line-height: 1;
    min-width: 22px; height: 20px; padding: 0 5px; cursor: pointer;
  }
  .dz-btn:hover { color: var(--text); background: var(--surface-1); }
  .dz-btn.wide { padding: 0 8px; }
  svg { cursor: grab; touch-action: none; }
  svg:active { cursor: grabbing; }
  .edge-label { paint-order: stroke; stroke: var(--stage); stroke-width: 4px; }
  .w-error { padding: 8px 12px; font-size: 11px; color: var(--bad); }
  .w-source { margin: 0 12px; padding: 8px; font-size: 10px; overflow: auto; background: var(--input); border-radius: 6px; }
</style>
