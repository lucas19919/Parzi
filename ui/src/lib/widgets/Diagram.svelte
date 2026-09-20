<script lang="ts">
  import Icon from "../Icon.svelte";
  import Zoomable from "./Zoomable.svelte";
  export let data: any;
  const rawNodes: any[] = Array.isArray(data?.nodes) ? data.nodes : [];
  const rawEdges: any[] = Array.isArray(data?.edges) ? data.edges : [];
  // Normalize so malformed payloads degrade to the error card, never throw.
  const nodes: { id: string; label: string; color: string; shape: string; sub: string }[] = rawNodes
    .filter((n) => n && (typeof n.id === "string" || typeof n.id === "number") && String(n.id).trim())
    .map((n) => ({
      id: String(n.id),
      label: typeof n.label === "string" || typeof n.label === "number" ? String(n.label) : "",
      color: typeof n.color === "string" ? n.color.toLowerCase() : "",
      shape: typeof n.shape === "string" ? n.shape.toLowerCase() : "",
      sub: typeof n.sub === "string" || typeof n.sub === "number" ? String(n.sub) : "",
    }));
  const edges: { from: string; to: string; label: string; color: string; style: string }[] = rawEdges
    .filter((e) => e && (typeof e.from === "string" || typeof e.from === "number") && (typeof e.to === "string" || typeof e.to === "number"))
    .map((e) => ({
      from: String(e.from),
      to: String(e.to),
      label: typeof e.label === "string" || typeof e.label === "number" ? String(e.label) : "",
      color: typeof e.color === "string" ? e.color.toLowerCase() : "",
      style: typeof e.style === "string" ? e.style.toLowerCase() : "",
    }));
  const groups: { id: string; label: string; nodes: string[] }[] = Array.isArray(data?.groups)
    ? data.groups
        .filter((g: any) => g && (typeof g.id === "string" || typeof g.id === "number"))
        .slice(0, 20)
        .map((g: any) => ({
          id: String(g.id),
          label: typeof g.label === "string" || typeof g.label === "number" ? String(g.label) : "",
          nodes: Array.isArray(g.nodes) ? g.nodes.map((m: any) => String(m ?? "")) : [],
        }))
    : [];
  const title: string = String(data?.title ?? "");
  const vertical = String(data?.direction ?? "LR").toUpperCase() === "TB";
  const isSeq = String(data?.layout ?? "flow").toLowerCase() === "sequence";
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
  // Keep group members adjacent inside their lane so containers stay tight.
  const groupOf = new Map<string, number>();
  groups.forEach((g, gi) => {
    for (const m of g.nodes) if (!groupOf.has(m)) groupOf.set(m, gi);
  });
  for (const list of cols.values()) {
    const order = new Map(list.map((n, i) => [n.id, i]));
    list.sort(
      (a, b) =>
        (groupOf.get(a.id) ?? 1e9) - (groupOf.get(b.id) ?? 1e9) ||
        (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0),
    );
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

  // Group containers from placed members.
  $: boxes = groups
    .map((g) => {
      const pts = g.nodes
        .map((m) => pos.get(m))
        .filter((p): p is { x: number; y: number } => !!p);
      if (!pts.length) return null;
      const x0 = Math.min(...pts.map((p) => p.x)) - 10;
      const y0 = Math.min(...pts.map((p) => p.y)) - 10;
      const x1 = Math.max(...pts.map((p) => p.x + CW)) + 10;
      const y1 = Math.max(...pts.map((p) => p.y + CH)) + 10;
      return { id: g.id, label: g.label || g.id, x0, y0, x1, y1 };
    })
    .filter((b): b is { id: string; label: string; x0: number; y0: number; x1: number; y1: number } => !!b);

  const MARKS: [string, string][] = [
    ["line", "var(--text-3)"],
    ["accent", "var(--accent)"],
    ["ok", "var(--ok)"],
    ["warn", "var(--warn)"],
    ["bad", "var(--bad)"],
    ["info", "var(--info)"],
  ];
  const edgeInk = (c: string): string => (INK[c] ? c : "line");
  const edgeDash = (s: string): string =>
    s === "dashed" ? "6 4" : s === "dotted" ? "2 4" : "";
  const edgeWidth = (s: string): number => (s === "thick" ? 3.5 : 2);

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

  // Sequence geometry: participants in given order, messages top to bottom
  // in edge order. Self-messages become a short loop.
  $: seq = isSeq
    ? (() => {
        const PW = 150, BW = 130, PH = 34, TOP = 8, Y0 = 66, STEP = 30, BOT = 14;
        const W = Math.max(1, nodes.length * PW) + 20;
        const H = Y0 + Math.max(1, edges.length) * STEP + BOT;
        const cx = (i: number) => 8 + i * PW + PW / 2;
        const bx = (i: number) => 8 + i * PW + (PW - BW) / 2;
        const idx = new Map(nodes.map((n, i) => [n.id, i]));
        return { PW, BW, PH, TOP, Y0, STEP, BOT, W, H, cx, bx, idx };
      })()
    : null;
</script>

<Zoomable enabled let:open let:toggle>
<div class="widget-card" class:bad>
  <div class="w-head">
    <span class="w-kind">diagram</span>
    {#if title}<span class="w-title">{title}</span>{/if}
    <button class="dz-btn zoom" on:click={toggle} title={open ? "Close zoomed view" : "Zoom diagram"}>
      <Icon d="M8 3H3v5M16 3h5v5M8 21H3v-5M16 21h5v-5" size={11} />
    </button>
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
    <svg width="100%" viewBox={isSeq && seq ? `0 0 ${seq.W} ${seq.H}` : `0 0 ${W} ${H}`} role="img"
      on:pointerdown={panStart} on:pointermove={panMove} on:pointerup={panEnd} on:pointerleave={panEnd}>
      <defs>
        {#each MARKS as [id, fill]}
          <marker id={`dz-${id}`} viewBox="0 0 10 10" refX="8" refY="5"
            markerWidth="7" markerHeight="7" orient="auto-start-reverse">
            <path d="M0 0L10 5L0 10z" fill={fill} />
          </marker>
        {/each}
      </defs>
      <g transform="translate({px} {py}) scale({k})">
      {#if isSeq && seq}
        {#each nodes as n, ni}
          <rect x={seq.bx(ni)} y={seq.TOP} width={seq.BW} height={seq.PH} rx="10"
            fill="var(--surface-1)" stroke={inkOf(n.color) ?? "var(--line)"} stroke-width={inkOf(n.color) ? 2 : 1.5}>
            <title>{n.label || n.id}</title>
          </rect>
          <text x={seq.cx(ni)} y={seq.TOP + seq.PH / 2 + 5} fill="var(--text)" font-size="12" text-anchor="middle">
            {short(n.label || n.id, 18)}
          </text>
          <line x1={seq.cx(ni)} y1={seq.TOP + seq.PH + 6} x2={seq.cx(ni)} y2={seq.H - 8}
            stroke="var(--line-2)" stroke-width="1" stroke-dasharray="4 4" />
        {/each}
        {#each edges as e, ei}
          {@const a = seq.idx.get(e.from) ?? 0}
          {@const b = seq.idx.get(e.to) ?? 0}
          {@const y = seq.Y0 + ei * seq.STEP}
          {#if a === b}
            <line x1={seq.cx(a)} y1={y} x2={seq.cx(a) + 26} y2={y}
              stroke="var(--line)" stroke-width="2" />
            <line x1={seq.cx(a) + 26} y1={y} x2={seq.cx(a) + 26} y2={y + 12}
              stroke="var(--line)" stroke-width="2" />
            <line x1={seq.cx(a) + 26} y1={y + 12} x2={seq.cx(a)} y2={y + 12}
              stroke="var(--line)" stroke-width="2" marker-end="url(#dz-line)" />
            {#if e.label}
              <text x={seq.cx(a) + 30} y={y + 6} fill="var(--text-3)" font-size="10">{short(e.label, 24)}<title>{e.label}</title></text>
            {/if}
          {:else}
            <line x1={seq.cx(a)} y1={y} x2={seq.cx(b)} y2={y}
              stroke={INK[e.color] ?? "var(--line)"} stroke-width={edgeWidth(e.style)}
              stroke-dasharray={edgeDash(e.style) || undefined} marker-end={`url(#dz-${edgeInk(e.color)})`} />
            {#if e.label}
              <text x={(seq.cx(a) + seq.cx(b)) / 2} y={y - 5} fill="var(--text-3)" font-size="10"
                text-anchor="middle" class="edge-label">{short(e.label, 30)}<title>{e.label}</title></text>
            {/if}
          {/if}
        {/each}
      {:else}
      {#each boxes as bx (bx.id)}
        <rect x={bx.x0} y={bx.y0} width={bx.x1 - bx.x0} height={bx.y1 - bx.y0} rx="12"
          fill="transparent" stroke="var(--line-2)" stroke-width="1" stroke-dasharray="5 4" />
        <text x={bx.x0 + 8} y={bx.y0 - 6} fill="var(--text-4)" font-size="11">{short(bx.label, 32)}</text>
      {/each}
      {#each edges as e}
        {@const a = pos.get(e.from)}
        {@const b = pos.get(e.to)}
        {#if a && b}
          {@const p = ends(a, b)}
          <line x1={p.x1} y1={p.y1} x2={p.x2} y2={p.y2}
            stroke={INK[e.color] ?? "var(--line)"} stroke-width={edgeWidth(e.style)}
            stroke-dasharray={edgeDash(e.style) || undefined} marker-end={`url(#dz-${edgeInk(e.color)})`} />
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
          {@const stroke = ink ?? "var(--line)"}
          {#if n.shape === "diamond"}
            <polygon
              points={`${p.x + CW / 2},${p.y} ${p.x + CW},${p.y + CH / 2} ${p.x + CW / 2},${p.y + CH} ${p.x},${p.y + CH / 2}`}
              fill="var(--surface-1)" {stroke} stroke-width={ink ? 2 : 1.5} stroke-linejoin="round" />
            <text x={p.x + CW / 2} y={p.y + CH / 2 + 4} fill="var(--text)" font-size="12" text-anchor="middle">
              {short(n.label || n.id, 14)}<title>{n.label || n.id}</title>
            </text>
          {:else if n.shape === "db"}
            <path
              d={`M ${p.x} ${p.y + 10} L ${p.x} ${p.y + CH - 10} A ${CW / 2} 10 0 0 0 ${p.x + CW} ${p.y + CH - 10} L ${p.x + CW} ${p.y + 10}`}
              fill="var(--surface-1)" {stroke} stroke-width={ink ? 2 : 1.5} />
            <ellipse cx={p.x + CW / 2} cy={p.y + 10} rx={CW / 2} ry={10}
              fill="none" {stroke} stroke-width={ink ? 2 : 1.5} />
            <text x={p.x + CW / 2} y={p.y + CH / 2 + 8} fill="var(--text)" font-size="12" text-anchor="middle">
              {short(n.label || n.id, 18)}<title>{n.label || n.id}</title>
            </text>
          {:else if n.shape === "actor"}
            {@const cx = p.x + 26}
            <circle cx={cx} cy={p.y + 13} r={7} fill="none" stroke={stroke} stroke-width={ink ? 2 : 1.5} />
            <line x1={cx} y1={p.y + 20} x2={cx} y2={p.y + 37} stroke={stroke} stroke-width={ink ? 2 : 1.5} />
            <line x1={cx - 10} y1={p.y + 25} x2={cx + 10} y2={p.y + 25} stroke={stroke} stroke-width={ink ? 2 : 1.5} />
            <line x1={cx} y1={p.y + 37} x2={cx - 8} y2={p.y + 48} stroke={stroke} stroke-width={ink ? 2 : 1.5} />
            <line x1={cx} y1={p.y + 37} x2={cx + 8} y2={p.y + 48} stroke={stroke} stroke-width={ink ? 2 : 1.5} />
            <text x={p.x + 44} y={p.y + CH / 2 + 4} fill="var(--text)" font-size="12">
              {short(n.label || n.id, 16)}<title>{n.label || n.id}</title>
            </text>
          {:else}
            <rect x={p.x} y={p.y} width={CW} height={CH} rx="10"
              fill="var(--surface-1)" stroke={stroke} stroke-width={ink ? 2 : 1.5} />
            {#if ink}
              <rect x={p.x} y={p.y + 10} width={3} height={CH - 20} rx="1.5" fill={ink} />
            {/if}
            {#if n.sub}
              <text x={p.x + (ink ? 16 : 12)} y={p.y + CH / 2 - 1} fill="var(--text)" font-size="12">
                {short(n.label || n.id, 20)}<title>{n.label || n.id}</title>
              </text>
              <text x={p.x + (ink ? 16 : 12)} y={p.y + CH / 2 + 13} fill="var(--text-4)" font-size="10">
                {short(n.sub, 24)}<title>{n.sub}</title>
              </text>
            {:else}
              <text x={p.x + (ink ? 16 : 12)} y={p.y + CH / 2 + 5} fill="var(--text)" font-size="13">
                {short(n.label || n.id, 20)}<title>{n.label || n.id}</title>
              </text>
            {/if}
          {/if}
        {/if}
      {/each}
      {/if}
      </g>
    </svg>
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
  .w-title { font-weight: 600; color: var(--text); font-size: 12px; }
  .dz-tools { display: flex; gap: 4px; padding: 6px 0 2px; }
  .dz-btn {
    background: transparent; border: 1px solid transparent; border-radius: 5px;
    color: var(--text-4); font: inherit; font-size: 11px; line-height: 1;
    min-width: 22px; height: 20px; padding: 0 5px; cursor: pointer;
  }
  .dz-btn:hover { color: var(--text); background: var(--surface-1); }
  .dz-btn.wide { padding: 0 8px; }
  .dz-btn.zoom { display: inline-flex; align-items: center; justify-content: center; }
  svg { cursor: grab; touch-action: none; }
  svg:active { cursor: grabbing; }
  .edge-label { paint-order: stroke; stroke: var(--stage); stroke-width: 4px; }
  .w-error { padding: 8px 12px; font-size: 11px; color: var(--bad); }
  .w-source { margin: 0 12px; padding: 8px; font-size: 10px; overflow: auto; background: var(--input); border-radius: 6px; }
</style>
