<script lang="ts">
  export let data: any;
  const nodes: { id: string; label?: string }[] = Array.isArray(data?.nodes) ? data.nodes : [];
  const edges: { from: string; to: string; label?: string }[] = Array.isArray(data?.edges) ? data.edges : [];
  const title: string = String(data?.title ?? "");
  const bad = !nodes.length || nodes.length > 200 || edges.length > 400;

  // Depth via BFS from roots; cycle-safe with visited set.
  const depth = new Map<string, number>();
  const incoming = new Map<string, number>();
  for (const n of nodes) incoming.set(n.id, 0);
  for (const e of edges) incoming.set(e.to, (incoming.get(e.to) ?? 0) + 1);
  const queue: string[] = nodes.filter((n) => (incoming.get(n.id) ?? 0) === 0).map((n) => n.id);
  if (!queue.length && nodes.length) queue.push(nodes[0].id);
  for (const q of queue) depth.set(q, 0);
  const visit = [...queue];
  const seen = new Set<string>();
  while (visit.length) {
    const cur = visit.shift()!;
    if (seen.has(cur)) continue;
    seen.add(cur);
    for (const e of edges.filter((x) => x.from === cur)) {
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
  const maxRows = Math.max(1, ...[...cols.values()].map((c) => c.length));
  const W = Math.max(1, cols.size * (CW + GX));
  const H = maxRows * (CH + GY) + 10;
  const pos = new Map<string, { x: number; y: number }>();
  [...cols.entries()].sort((a, b) => a[0] - b[0]).forEach(([d, list], ci) => {
    list.forEach((n, ri) => pos.set(n.id, { x: ci * (CW + GX) + 8, y: ri * (CH + GY) + 8 }));
  });
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
      {:else}Too many edges ({edges.length}/400 max).{/if}
      Showing source.
    </div>
    <pre class="w-source">{JSON.stringify(data, null, 2)?.slice(0, 3000)}</pre>
  {:else}
    <svg width="100%" viewBox="0 0 {W} {H}" role="img">
      {#each edges as e}
        {@const a = pos.get(e.from)}
        {@const b = pos.get(e.to)}
        {#if a && b}
          <line x1={a.x + CW} y1={a.y + CH / 2} x2={b.x} y2={b.y + CH / 2}
            stroke="var(--line)" stroke-width="2" />
          {#if e.label}<text x={(a.x + CW + b.x) / 2} y={(a.y + b.y) / 2 - 4} fill="var(--text-3)" font-size="10" text-anchor="middle">{e.label.slice(0, 24)}</text>{/if}
        {/if}
      {/each}
      {#each nodes as n}
        {@const p = pos.get(n.id)}
        {#if p}
          <rect x={p.x} y={p.y} width={CW} height={CH} rx="10"
            fill="var(--surface-1)" stroke="var(--line)" stroke-width="1.5" />
          <text x={p.x + 12} y={p.y + CH / 2 + 5} fill="var(--text)" font-size="13">
            {(n.label ?? n.id).slice(0, 22)}
          </text>
        {/if}
      {/each}
    </svg>
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
  .w-title { font-weight: 600; color: var(--text); font-size: 12px; }
  .w-meta { font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10px; color: var(--text-3); margin-left: auto; }
  .w-error { padding: 8px 12px; font-size: 11px; color: var(--bad); }
  .w-source { margin: 0 12px; padding: 8px; font-size: 10px; overflow: auto; background: var(--input); border-radius: 6px; }
</style>
