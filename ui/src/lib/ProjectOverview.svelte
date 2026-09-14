<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { LaneView, SessionMeta } from "./api";

  export let project = "default";
  export let root = "";
  export let branch = "";
  export let lanes: LaneView[] = [];
  export let threads: SessionMeta[] = [];
  export let logos: Record<string, string> = {};

  const dispatch = createEventDispatcher<{
    openThread: { id: string };
    newThread: void;
    killRun: { id: string };
    newSubsession: { id: string };
  }>();

  interface Row {
    t: SessionMeta;
    depth: number;
  }

  function byUpdated(a: SessionMeta, b: SessionMeta): number {
    return +new Date(b.updated) - +new Date(a.updated);
  }

  function byCreated(a: SessionMeta, b: SessionMeta): number {
    return +new Date(a.created) - +new Date(b.created);
  }

  $: mine = threads.filter((t) => (t.project || "default") === project);
  $: byId = new Map(mine.map((t) => [t.id, t]));
  $: roots = mine
    .filter((t) => !t.parent_id || !byId.has(t.parent_id as string))
    .sort(byUpdated);

  /** Flatten a set of roots into indented rows (arbitrary depth). */
  function walk(list: SessionMeta[]): Row[] {
    const kids = new Map<string, SessionMeta[]>();
    for (const t of mine) {
      if (!t.parent_id) continue;
      if (!byId.has(t.parent_id)) continue;
      if (!kids.has(t.parent_id)) kids.set(t.parent_id, []);
      kids.get(t.parent_id)!.push(t);
    }
    for (const k of kids.values()) k.sort(byCreated);
    const rows: Row[] = [];
    const go = (t: SessionMeta, depth: number) => {
      rows.push({ t, depth });
      for (const c of kids.get(t.id) ?? []) go(c, depth + 1);
    };
    for (const r of list) go(r, 0);
    return rows;
  }

  $: liveRoots = roots.filter((t) => t.status === "active" || t.status === "queued");
  $: restRoots = roots
    .filter((t) => t.status !== "active" && t.status !== "queued")
    .slice(0, 30);
  $: liveRows = walk(liveRoots);
  $: restRows = walk(restRoots);
  $: liveCount = liveRoots.length;

  function provOf(t: SessionMeta): string {
    const m = t.model || "";
    if (m === "auto") return "auto";
    return m.split("/")[0] || "";
  }

  function toks(t: SessionMeta): string {
    const n = (t.tokens_in || 0) + (t.tokens_out || 0);
    if (n >= 1000) return `${(n / 1000).toFixed(1)}k tok`;
    return `${n} tok`;
  }

  function rel(iso: string): string {
    const s = Math.max(0, (Date.now() - +new Date(iso)) / 1000);
    if (s < 60) return "just now";
    if (s < 3600) return `${Math.floor(s / 60)}m ago`;
    if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
    return `${Math.floor(s / 86400)}d ago`;
  }

  function statusLabel(t: SessionMeta): string {
    if (t.status === "active") return "working";
    if (t.status === "queued") return "queued";
    if (t.status === "done") return "done";
    if (t.status === "killed") return "stopped";
    return "idle";
  }
</script>

<div class="proj-ov">
  <div class="ov-head">
    <div class="ov-title-row">
      <span class="ov-name">{project}</span>
      {#if branch}<span class="ov-branch">⎇ {branch}</span>{/if}
      {#if liveCount > 0}
        <span class="ov-live"><span class="live-dot" />{liveCount} live</span>
      {/if}
      <span class="spacer" />
      <button class="go-btn" on:click={() => dispatch("newThread")}>+ New conversation</button>
    </div>
    {#if root}<div class="ov-root" title={root}>{root}</div>{/if}
    {#if lanes.length}
      <div class="lane-chips">
        {#each lanes as l}
          <span class="lane-chip" title="mode: {l.mode}{l.model ? ` · model: ${l.model}` : ''}">
            {l.name}<span class="lane-mode">{l.mode}</span>
          </span>
        {/each}
      </div>
    {/if}
  </div>

  {#if !roots.length}
    <div class="ov-empty">
      <p>No sessions in {project} yet.</p>
      <span class="sub">Start a conversation and it will show up here with its subsessions.</span>
    </div>
  {:else}
    {#if liveRows.length}
      <div class="ov-sec">Live now</div>
      <div class="sess-list">
        {#each liveRows as { t, depth } (t.id)}
          {@const prov = provOf(t)}
          <div
            class="sess-row"
            class:sub={depth > 0}
            role="button"
            tabindex="0"
            style={depth > 0 ? `margin-left:${depth * 18}px;width:calc(100% - ${depth * 18}px);` : ""}
            on:click={() => dispatch("openThread", { id: t.id })}
            on:keydown={(e) => { if (e.key === "Enter") dispatch("openThread", { id: t.id }); }}
          >
            {#if depth > 0}<span class="branch" title="Subsession">↳</span>{/if}
            <span class="live-dot" title={statusLabel(t)} />
            <span class="sess-main">
              <span class="sess-title">{t.title || "untitled"}</span>
              <span class="sess-meta">
                {#if t.lane}{t.lane} · {/if}{t.model || "no model"} · {toks(t)} · {rel(t.updated)}
              </span>
            </span>
            {#if prov && prov !== "auto" && logos[prov]}
              <img src={logos[prov]} alt={prov} title={t.model} class="prov-logo" draggable="false" />
            {/if}
            <span class="sess-hover">
              <span
                role="button"
                tabindex="0"
                title="Stop run"
                on:click|stopPropagation={() => dispatch("killRun", { id: t.id })}
                on:keydown={(e) => e.key === "Enter" && dispatch("killRun", { id: t.id })}>✕</span
              >
              <span
                role="button"
                tabindex="0"
                title="New subsession"
                on:click|stopPropagation={() => dispatch("newSubsession", { id: t.id })}
                on:keydown={(e) => e.key === "Enter" && dispatch("newSubsession", { id: t.id })}>+</span
              >
            </span>
          </div>
        {/each}
      </div>
    {/if}

    {#if restRows.length}
      <div class="ov-sec">Recent</div>
      <div class="sess-list">
        {#each restRows as { t, depth } (t.id)}
          {@const prov = provOf(t)}
          <div
            class="sess-row"
            class:sub={depth > 0}
            role="button"
            tabindex="0"
            style={depth > 0 ? `margin-left:${depth * 18}px;width:calc(100% - ${depth * 18}px);` : ""}
            on:click={() => dispatch("openThread", { id: t.id })}
            on:keydown={(e) => { if (e.key === "Enter") dispatch("openThread", { id: t.id }); }}
          >
            {#if depth > 0}<span class="branch" title="Subsession">↳</span>{/if}
            <span class="st-dot {t.status}" title={statusLabel(t)} />
            <span class="sess-main">
              <span class="sess-title">{t.title || "untitled"}</span>
              <span class="sess-meta">
                {#if t.lane}{t.lane} · {/if}{t.model || "no model"} · {toks(t)} · {rel(t.updated)}
              </span>
            </span>
            {#if prov && prov !== "auto" && logos[prov]}
              <img src={logos[prov]} alt={prov} title={t.model} class="prov-logo" draggable="false" />
            {/if}
            <span class="sess-hover">
              <span
                role="button"
                tabindex="0"
                title="New subsession"
                on:click|stopPropagation={() => dispatch("newSubsession", { id: t.id })}
                on:keydown={(e) => e.key === "Enter" && dispatch("newSubsession", { id: t.id })}>+</span
              >
            </span>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .proj-ov {
    max-width: 860px;
    margin: 0 auto;
    padding: 28px 32px 100px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: 100%;
    box-sizing: border-box;
  }
  .ov-head {
    background: var(--surface-1);
    border: 1px solid var(--line);
    border-radius: 14px;
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .ov-title-row { display: flex; align-items: center; gap: 10px; }
  .ov-name { font-size: 20px; font-weight: 700; color: var(--text); letter-spacing: -0.3px; }
  .ov-branch { font-size: 12px; color: var(--text-3); font-family: var(--parzi-mono), ui-monospace, monospace; }
  .ov-live {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 11px; font-weight: 600; color: var(--ok);
    background: var(--ok-soft);
    border: 1px solid var(--ok-line);
    border-radius: 6px; padding: 2px 8px;
  }
  .live-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 8px var(--ok-line); }
  .spacer { flex: 1; }
  .go-btn {
    background: var(--accent-soft); border: 1px solid var(--accent-line);
    color: var(--accent-text); font: inherit; font-size: 12px; padding: 6px 12px;
    cursor: pointer; border-radius: 7px; white-space: nowrap;
  }
  .go-btn:hover { background: var(--accent-mid); }
  .ov-root {
    font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 11px;
    color: var(--text-3);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .lane-chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .lane-chip {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 11.5px; color: var(--text);
    background: var(--surface-2);
    border: 1px solid var(--line);
    border-radius: 7px; padding: 4px 9px;
  }
  .lane-mode {
    font-size: 10px; color: var(--accent);
    background: var(--accent-soft);
    border-radius: 4px; padding: 1px 5px;
  }
  .ov-sec {
    font-size: 11px; font-weight: 600; letter-spacing: 0.6px; text-transform: uppercase;
    color: var(--text-3); padding: 6px 2px 0;
  }
  .sess-list { display: flex; flex-direction: column; gap: 6px; }
  .sess-row {
    display: flex; align-items: center; gap: 10px; width: 100%; box-sizing: border-box;
    background: var(--surface-1);
    border: 1px solid var(--line);
    border-radius: 10px; padding: 10px 14px; cursor: pointer; outline: none;
  }
  .sess-row:hover { background: var(--surface-2); }
  .sess-row:focus-visible { box-shadow: 0 0 0 1.5px var(--accent); }
  .sess-row.sub { border-left: 1px solid var(--line-3); border-radius: 0 10px 10px 0; }
  .branch { flex: none; color: var(--text-4); font-size: 12px; }
  .st-dot { width: 7px; height: 7px; flex: none; border-radius: 50%; background: var(--text-4); }
  .st-dot.done { background: var(--info); }
  .st-dot.killed { background: var(--bad); }
  .sess-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .sess-title { font-size: 13.5px; font-weight: 600; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sess-meta { font-size: 11px; color: var(--text-3); font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .prov-logo { width: 15px; height: 15px; flex: none; object-fit: contain; opacity: 0.75; }
  .sess-hover { display: none; gap: 2px; color: var(--text-3); flex: none; align-items: center; }
  .sess-row:hover .sess-hover, .sess-row:focus-within .sess-hover { display: inline-flex; }
  .sess-hover span { display: inline-flex; align-items: center; padding: 3px 7px; border-radius: 5px; cursor: pointer; font-size: 12px; }
  .sess-hover span:hover { background: var(--surface-3); color: var(--text); }
  .ov-empty {
    text-align: center; padding: 48px 24px;
    background: var(--surface-1);
    border: 1px solid var(--line);
    border-radius: 14px; color: var(--text-3); font-size: 13px;
  }
  .ov-empty .sub { font-size: 11.5px; opacity: 0.65; display: block; margin-top: 4px; }
</style>
