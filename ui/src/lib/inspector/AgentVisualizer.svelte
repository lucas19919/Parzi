<script lang="ts">
  import ProviderLogo from "../ProviderLogo.svelte";
  import { createEventDispatcher } from "svelte";
  import type { ChatEvent, SwarmNode } from "../api";

  /** Root thread + every subsession of the swarm the active thread belongs to. */
  export let nodes: SwarmNode[] = [];
  export let activeThreadId: string | null = null;
  /** Events of the active thread: tool trace + message vectors come from here. */
  export let events: ChatEvent[] = [];
  export let approval: { key: string; call: { id: string; name: string; args: unknown; lane: string } } | null = null;

  const dispatch = createEventDispatcher<{
    focus: { id: string };
    fork: { id: string };
    kill: { id: string };
    spawn: { id: string };
    approve: { key: string; allow: boolean };
  }>();

  const isLive = (s: string) => s === "active" || s === "queued";

  function vote(allow: boolean) {
    if (approval) dispatch("approve", { key: approval.key, allow });
  }

  function provider(model: string): string {
    return (model || "").split("/")[0] || "";
  }
  function shortModel(model: string): string {
    const parts = (model || "").split("/");
    return parts.length > 1 ? parts.slice(1).join("/") : model || "auto";
  }
  function fmtTokens(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1000) return `${(n / 1000).toFixed(n >= 10_000 ? 0 : 1)}k`;
    return String(n);
  }
  function fmtCost(c: number): string {
    if (!c) return "$0";
    return c < 0.01 ? "<$0.01" : `$${c.toFixed(2)}`;
  }
  function fmtMs(ms: number): string {
    return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.round(ms)}ms`;
  }
  function stateWord(s: string): string {
    if (s === "active") return "working";
    if (s === "queued") return "queued";
    if (s === "done") return "finished";
    if (s === "killed") return "stopped";
    return "idle";
  }

  /* ---------- tree order: App emits pre-order (parent, then children) ---------- */

  /* ---------- live tool trace (active thread) ---------- */
  $: trace = (() => {
    const results = new Map<string, { ok: boolean; ms: number }>();
    for (const e of events) if (e.kind === "tool_result") results.set(e.id, { ok: e.ok, ms: e.ms ?? 0 });
    const calls: { id: string; name: string; args: unknown; ok?: boolean; ms?: number; running: boolean }[] = [];
    for (const e of events) {
      if (e.kind !== "tool_call") continue;
      const r = results.get(e.id);
      calls.push({ id: e.id, name: e.name, args: e.args, ok: r?.ok, ms: r?.ms, running: !r });
    }
    return calls.slice(-14).reverse();
  })();

  function argSummary(args: unknown): string {
    if (!args || typeof args !== "object") return "";
    const a = args as Record<string, unknown>;
    const key = ["path", "command", "cmd", "query", "url", "title", "session_id", "id", "message", "prompt"].find((k) => typeof a[k] === "string");
    const v = key ? String(a[key]) : JSON.stringify(a);
    return v.length > 64 ? v.slice(0, 61) + "…" : v;
  }

  $: liveCount = nodes.filter((n) => isLive(n.status)).length;
  $: totalTokens = nodes.reduce((s, n) => s + n.tokens, 0);
  $: totalCost = nodes.reduce((s, n) => s + n.cost, 0);
</script>

<div class="viz">
  {#if !nodes.length}
    <div class="empty">
      <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="5" r="2.5" /><circle cx="5" cy="19" r="2.5" /><circle cx="19" cy="19" r="2.5" /><path d="M12 7.5v4M12 11.5l-5.5 5M12 11.5l5.5 5" /></svg>
      <span class="empty-title">No swarm yet</span>
      <span class="empty-sub">Open a thread. Subsessions spawned with session.spawn appear here as a live tree.</span>
    </div>
  {:else}
    <!-- Agents: one unified tree (status, cost, last tool, actions). -->
    <section class="sec">
      <div class="sec-head">
        <span>Agents</span>
        <span class="sec-meta">{nodes.length} agent{nodes.length === 1 ? "" : "s"} · {liveCount} live · {fmtTokens(totalTokens)} tok · {fmtCost(totalCost)}</span>
      </div>
      <div class="table">
        {#each nodes as n (n.id)}
          {@const indent = Math.min(n.depth || 0, 5) * 14}
          <div class="row" class:on={n.id === activeThreadId} style={indent ? `padding-left:${10 + indent}px;` : ""}>
            <span class="dot {n.status}" title={stateWord(n.status)} />
            <button class="cell name" title={`${n.lane || "default"} / ${n.title || "untitled"}`} on:click={() => dispatch("focus", { id: n.id })}>
              <span class="lane">{n.lane || "default"}</span>
              <span class="title">{n.depth > 0 ? "↳ " : ""}{n.title || "untitled"}</span>
            </button>
            <span class="cell model" title={n.model}>
              <ProviderLogo provider={provider(n.model)} size={12} muted />
              <span>{shortModel(n.model)}</span>
            </span>
            <span class="cell num">{fmtTokens(n.tokens)} · {fmtCost(n.cost)}</span>
            <span class="cell tool" title={n.tool?.name ?? ""}>
              {#if n.tool?.running}<span class="pulse-dot" />{/if}
              {n.tool?.name ?? "—"}
            </span>
            <span class="cell acts">
              {#if n.id !== activeThreadId}
                <button class="act" title="Switch to this thread" on:click={() => dispatch("focus", { id: n.id })}>Focus</button>
              {/if}
              <button class="act" title="Fork this thread" on:click={() => dispatch("fork", { id: n.id })}>Fork</button>
              {#if isLive(n.status)}
                <button class="act bad" title="Stop this run" on:click={() => dispatch("kill", { id: n.id })}>Kill</button>
              {:else}
                <button class="act" title="Spawn a subsession under this thread" on:click={() => dispatch("spawn", { id: n.id })}>Spawn</button>
              {/if}
            </span>
          </div>
        {/each}
      </div>
    </section>

    <!-- Live tool trace -->
    <section class="sec">
      <div class="sec-head">
        <span>Tool trace</span>
        <span class="sec-meta">{activeThreadId ? "active thread" : "—"}</span>
      </div>
      {#if approval}
        <div class="approval">
          <div class="appr-head">Waiting for approval · <b>{approval.call.name}</b> · lane {approval.call.lane}</div>
          <pre>{JSON.stringify(approval.call.args, null, 2)?.slice(0, 800)}</pre>
          <div class="appr-acts">
            <button class="act ok" on:click={() => vote(true)}>Approve</button>
            <button class="act bad" on:click={() => vote(false)}>Deny</button>
          </div>
        </div>
      {/if}
      {#if !trace.length}
        <div class="trace-empty">No tool calls in this thread yet.</div>
      {:else}
        <div class="trace">
          {#each trace as t (t.id)}
            <div class="tr" class:running={t.running} class:bad={!t.running && t.ok === false}>
              <span class="tr-state">
                {#if t.running}<span class="pulse-dot" />{:else if t.ok}✓{:else}✗{/if}
              </span>
              <span class="tr-name">{t.name}</span>
              <span class="tr-arg" title={argSummary(t.args)}>{argSummary(t.args)}</span>
              <span class="tr-ms">{t.running ? "running" : fmtMs(t.ms ?? 0)}</span>
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}
</div>

<style>
  .viz { display: flex; flex-direction: column; min-height: 0; height: 100%; overflow-y: auto; padding: 6px 10px 24px; gap: 12px; }
  .empty {
    flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center;
    gap: 6px; color: var(--text-4, #5d636f); padding: 24px; text-align: center;
  }
  .empty-title { font-size: 13px; color: var(--text-3, #94a3b8); font-weight: 500; }
  .empty-sub { font-size: 11.5px; max-width: 250px; line-height: 1.5; }

  .sec { display: flex; flex-direction: column; gap: 6px; }
  .sec-head {
    display: flex; align-items: center; justify-content: space-between;
    font-size: 11px; letter-spacing: 0.06em; text-transform: uppercase;
    color: var(--text-3, #94a3b8); padding: 4px 2px 0;
  }
  .sec-meta { font-family: var(--parzi-mono, monospace); text-transform: none; letter-spacing: 0; font-size: 10.5px; color: var(--text-4, #5d636f); font-variant-numeric: tabular-nums; }

  /* Status colour = state, only here. */
  .dot { width: 7px; height: 7px; border-radius: 50%; display: inline-block; background: var(--text-4, #5d636f); flex: none; }
  .dot.active { background: var(--ok, #4ade80); fill: var(--ok, #4ade80); box-shadow: 0 0 8px var(--ok-line, rgba(74,222,128,0.5)); }
  .dot.queued { background: var(--warn, #fbbf24); fill: var(--warn, #fbbf24); animation: toolpulse 1.6s ease-in-out infinite; }
  .dot.done { background: var(--accent, #7c8cff); fill: var(--accent, #7c8cff); }
  .dot.killed { background: var(--bad, #f87171); fill: var(--bad, #f87171); }
  .dot.idle { background: var(--text-4, #5d636f); fill: var(--text-4, #5d636f); }
  @keyframes toolpulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.55; } }
  .pulse-dot { width: 5px; height: 5px; border-radius: 50%; background: var(--ok, #4ade80); display: inline-block; animation: deck-pulse 2s ease-in-out infinite; flex: none; }
  @keyframes deck-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.35; } }

  .table {
    display: flex; flex-direction: column;
    background: var(--surface-1, rgba(255,255,255,0.03));
    border: 1px solid var(--line-2, rgba(255,255,255,0.07));
    border-radius: var(--radius-3, 10px); overflow: hidden;
  }
  .row {
    display: grid; grid-template-columns: 12px minmax(0, 1.4fr) minmax(0, 1fr) auto; grid-template-rows: auto auto;
    align-items: center; column-gap: 8px; row-gap: 3px; padding: 7px 10px; font-size: 12px;
    border-bottom: 1px solid var(--line-2, rgba(255,255,255,0.05));
  }
  .row:last-child { border-bottom: none; }
  .row.on { background: var(--accent-soft, rgba(124,140,255,0.08)); }
  .row .dot { grid-row: 1 / span 2; }
  .cell { min-width: 0; }
  .cell.name {
    display: flex; flex-direction: column; align-items: flex-start; gap: 0; background: transparent; border: none;
    color: inherit; font: inherit; padding: 0; cursor: pointer; text-align: left; overflow: hidden;
  }
  .cell.name .lane { font-family: var(--parzi-mono, monospace); font-size: 10px; color: var(--text-4, #5d636f); }
  .cell.name .title { color: var(--text, #f1f5f9); font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  .cell.name:hover .title { color: var(--accent-text, #c7ccff); }
  .cell.model { display: inline-flex; align-items: center; gap: 5px; color: var(--text-3, #94a3b8); font-size: 11px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .cell.num { font-family: var(--parzi-mono, monospace); font-size: 10.5px; color: var(--text-3, #94a3b8); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .cell.tool { grid-column: 2 / span 2; display: inline-flex; align-items: center; gap: 5px; font-family: var(--parzi-mono, monospace); font-size: 10.5px; color: var(--text-4, #5d636f); overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .cell.acts { grid-column: 4; display: inline-flex; gap: 3px; justify-content: flex-end; }
  .act {
    background: var(--surface-2, rgba(255,255,255,0.06)); border: none; border-radius: 4px;
    color: var(--text-3, #94a3b8); font: inherit; font-size: 10px; padding: 2px 7px; cursor: pointer;
  }
  .act:hover { color: var(--text, #fff); background: var(--surface-3, rgba(255,255,255,0.12)); }
  .act.bad:hover { background: var(--bad-soft, rgba(248,113,113,0.15)); color: var(--bad, #f87171); }
  .act.ok { background: var(--ok-soft, rgba(74,222,128,0.12)); color: var(--ok, #4ade80); }

  .approval {
    background: var(--warn-soft, rgba(251,191,36,0.1)); border: 1px solid var(--warn-line, rgba(251,191,36,0.35));
    border-radius: var(--radius-3, 10px); padding: 8px 10px; font-size: 12px; display: flex; flex-direction: column; gap: 6px;
  }
  .appr-head { color: var(--warn, #fbbf24); }
  .approval pre { margin: 0; font-size: 10.5px; max-height: 120px; overflow: auto; background: rgba(0,0,0,0.3); border-radius: 6px; padding: 6px 8px; }
  .appr-acts { display: flex; gap: 4px; }
  .trace-empty { font-size: 11.5px; color: var(--text-4, #5d636f); padding: 6px 2px; }
  .trace {
    display: flex; flex-direction: column;
    background: var(--surface-1, rgba(255,255,255,0.03));
    border: 1px solid var(--line-2, rgba(255,255,255,0.07));
    border-radius: var(--radius-3, 10px); overflow: hidden;
  }
  .tr {
    display: grid; grid-template-columns: 14px auto minmax(0, 1fr) auto; align-items: center; gap: 8px;
    padding: 5px 10px; font-size: 11px; border-bottom: 1px solid var(--line-2, rgba(255,255,255,0.05));
  }
  .tr:last-child { border-bottom: none; }
  .tr.running { background: var(--ok-soft, rgba(74,222,128,0.06)); }
  .tr.bad .tr-state { color: var(--bad, #f87171); }
  .tr-state { color: var(--ok, #4ade80); font-size: 10px; display: inline-flex; align-items: center; justify-content: center; }
  .tr-name { font-family: var(--parzi-mono, monospace); color: var(--text, #f1f5f9); font-size: 11px; white-space: nowrap; }
  .tr-arg { font-family: var(--parzi-mono, monospace); color: var(--text-4, #5d636f); font-size: 10.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tr-ms { font-family: var(--parzi-mono, monospace); color: var(--text-3, #94a3b8); font-size: 10px; white-space: nowrap; font-variant-numeric: tabular-nums; }
</style>
