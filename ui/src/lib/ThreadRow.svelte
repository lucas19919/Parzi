<script lang="ts">
  import { createEventDispatcher, onDestroy, onMount } from "svelte";
  import type { SessionMeta } from "./api";

  export let t: SessionMeta;
  export let depth = 0;
  export let hasKids = false;
  export let kidCount = 0;
  export let shut = false;
  export let active = false;
  export let renaming = false;
  export let renameDraft = "";
  export let logos: Record<string, string> = {};
  /** Git branch of the thread's project (dim suffix, T3-style). */
  export let branch = "";
  /** Set when the row is shown outside its workspace group (filter results). */
  export let showProject = "";

  const dispatch = createEventDispatcher<{
    select: { id: string };
    toggleTree: { id: string };
    togglePin: { id: string };
    startRename: { id: string };
    commitRename: void;
    cancelRename: void;
    killRun: { id: string };
    newSubsession: { id: string };
    fork: { id: string };
    deleteRequest: { id: string };
    contextMenu: { id: string; x: number; y: number };
  }>();

  /** Provider key for the row logo (`provider/model` → provider). */
  function provOf(m: SessionMeta): string {
    const model = m.model || "";
    if (model === "auto") return "auto";
    return model.split("/")[0] || "";
  }

  $: prov = provOf(t);

  /** Short model label, always shown bottom-right (e.g. `provider/family` → `family`). */
  $: modelLabel = (() => {
    const m = (t.model || "auto").trim() || "auto";
    if (m === "auto") return "auto";
    const short = m.split("/").pop() || m;
    return short.length > 16 ? short.slice(0, 16) + "…" : short;
  })();

  // Live "Working 25h 21m" timer: re-render every 30s so the elapsed label
  // ticks without waiting for a thread-list refresh.
  let now = Date.now();
  let ticker: ReturnType<typeof setInterval> | null = null;
  onMount(() => {
    if (t.status === "active") {
      ticker = setInterval(() => (now = Date.now()), 30000);
    }
  });
  onDestroy(() => {
    if (ticker) clearInterval(ticker);
  });

  /** Elapsed since creation: "1m", "2h", "3d". */
  function dur(): string {
    const ms = Math.max(0, now - +new Date(t.created || t.updated));
    const m = Math.floor(ms / 60000);
    if (m < 1) return "now";
    if (m < 60) return `${m}m`;
    const h = Math.floor(m / 60);
    if (h < 48) return `${h}h`;
    return `${Math.floor(h / 24)}d`;
  }

  /** Relative updated time for idle rows (compact: no "ago"). */
  function rel(): string {
    const s = Math.max(0, (Date.now() - +new Date(t.updated)) / 1000);
    if (s < 60) return "now";
    if (s < 3600) return `${Math.floor(s / 60)}m`;
    if (s < 86400) return `${Math.floor(s / 3600)}h`;
    return `${Math.floor(s / 86400)}d`;
  }

  /** Dim suffix: lane first, branch when it adds info. Never the project — the group header already says it. */
  $: subLine = [t.lane || "", branch || ""].filter(Boolean).filter((v, i, a) => a.indexOf(v) === i).join(" · ");

  function autofocus(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  function onRowKey(e: KeyboardEvent) {
    if ((e.target as HTMLElement).closest("[data-act]")) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      dispatch("select", { id: t.id });
    }
  }

  function onRenameKey(e: KeyboardEvent) {
    if (e.key === "Enter") dispatch("commitRename");
    else if (e.key === "Escape") dispatch("cancelRename");
    e.stopPropagation();
  }

  function onContext(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    dispatch("contextMenu", { id: t.id, x: e.clientX, y: e.clientY });
  }
</script>

<div
  class="t3"
  class:on={active}
  class:sub={depth > 0}
  class:live={t.status === "active"}
  role="button"
  tabindex="0"
  id="thread-{t.id}"
  title={t.title || "untitled"}
  style={depth > 0 ? `margin-left:${depth * 14}px;width:calc(100% - ${depth * 14}px);` : ""}
  on:click={() => dispatch("select", { id: t.id })}
  on:keydown={onRowKey}
  on:contextmenu={onContext}
>
  <div class="t3-title-row">
    {#if hasKids}
      <span
        class="disclosure"
        data-act
        role="button"
        tabindex="0"
        title={shut ? "Expand subsessions" : "Collapse subsessions"}
        on:click|stopPropagation={() => dispatch("toggleTree", { id: t.id })}
        on:keydown={(e) => { if (e.key === "Enter") dispatch("toggleTree", { id: t.id }); e.stopPropagation(); }}
      >{shut ? "▸" : "▾"}</span>
    {:else if depth > 0}
      <span class="branch" title="Subsession">↳</span>
    {/if}
    {#if renaming}
      <input
        class="rename"
        data-act
        bind:value={renameDraft}
        use:autofocus
        on:click|stopPropagation
        on:blur={() => dispatch("commitRename")}
        on:keydown={onRenameKey}
      />
    {:else}
      <span class="t3-title">{t.title || "untitled"}</span>
    {/if}
    {#if shut}<span class="kid-badge" title="{kidCount} subsession(s)">{kidCount}</span>{/if}
  </div>
  <div class="t3-meta">
    {#if t.status === "active"}
      <span class="spin" title="working" />
      <span class="st-working">{dur()}</span>
    {:else if t.status === "queued"}
      <span class="st-queue">Queued</span>
    {:else}
      <span class="st-rel">{rel()}</span>
    {/if}
    {#if showProject}<span class="st-proj" title="Workspace">{showProject}</span>{/if}
    {#if subLine}<span class="st-lane">{subLine}</span>{/if}
    <span class="meta-spacer" />
    {#if prov === "auto"}
      <span class="st-auto" title="Smart Auto">⚡</span>
    {:else if prov && logos[prov]}
      <img src={logos[prov]} alt={prov} title={t.model} class="prov-logo" draggable="false" />
    {/if}
    <span class="st-model" title={t.model || "auto"}>{modelLabel}</span>
    {#if t.pinned}<span class="st-pin" title="Pinned">★</span>{/if}
  </div>
  <span class="t3-hover" data-act>
    {#if t.status === "active" || t.status === "queued"}
      <span
        role="button"
        tabindex="0"
        title="Stop run"
        class="stop"
        on:click|stopPropagation={() => dispatch("killRun", { id: t.id })}
        on:keydown={(e) => e.key === "Enter" && dispatch("killRun", { id: t.id })}>✕</span
      >
    {/if}
    <span
      role="button"
      tabindex="0"
      title={t.pinned ? "Unpin" : "Pin"}
      on:click|stopPropagation={() => dispatch("togglePin", { id: t.id })}
      on:keydown={(e) => e.key === "Enter" && dispatch("togglePin", { id: t.id })}>{t.pinned ? "★" : "☆"}</span
    >
    <span
      role="button"
      tabindex="0"
      title="Rename"
      on:click|stopPropagation={() => dispatch("startRename", { id: t.id })}
      on:keydown={(e) => e.key === "Enter" && dispatch("startRename", { id: t.id })}>✎</span
    >
    <span
      role="button"
      tabindex="0"
      title="Delete thread"
      class="del"
      on:click|stopPropagation={() => dispatch("deleteRequest", { id: t.id })}
      on:keydown={(e) => e.key === "Enter" && dispatch("deleteRequest", { id: t.id })}>🗑</span
    >
  </span>
</div>

<style>
  /* Session row: title over a dim meta line. The workspace group header
     already names the project, so the row never repeats it. */
  .t3 {
    position: relative;
    display: flex; flex-direction: column; gap: 1px; width: 100%;
    box-sizing: border-box;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 9px;
    color: var(--text-2);
    font: inherit; padding: 6px 9px 6px 10px;
    cursor: pointer; text-align: left; outline: none;
  }
  .t3:hover { background: var(--surface-2); }
  .t3.on {
    background: var(--surface-3);
    border-color: var(--line-3);
    box-shadow: inset 2px 0 0 var(--accent);
    color: var(--text);
  }
  .t3:focus-visible { box-shadow: 0 0 0 1.5px var(--accent); }
  .t3.on:focus-visible { box-shadow: inset 2px 0 0 var(--accent), 0 0 0 1.5px var(--accent); }
  .t3.sub {
    border-left: 1px solid var(--line-3);
    border-radius: 0 9px 9px 0;
  }
  .t3-title-row { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .t3-title {
    flex: 1; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-size: 13px; color: var(--text-2);
  }
  .t3.on .t3-title { color: var(--text); font-weight: 550; }
  .t3.live .t3-title { color: var(--text); }
  .t3-meta {
    display: flex; align-items: center; gap: 6px;
    font-size: 11px; line-height: 1.4; color: var(--text-3);
    min-width: 0; padding-left: 2px;
  }
  .t3-title-row:has(.disclosure) + .t3-meta,
  .t3-title-row:has(.branch) + .t3-meta { padding-left: 18px; }
  .meta-spacer { flex: 1; }
  .spin {
    flex: none;
    width: 9px; height: 9px; border-radius: 50%;
    border: 1.6px solid var(--info-line);
    border-top-color: var(--info);
    animation: t3rot 0.9s linear infinite;
  }
  @keyframes t3rot { to { transform: rotate(360deg); } }
  .st-working { flex: none; color: var(--info); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .st-queue { flex: none; color: var(--warn); white-space: nowrap; }
  .st-rel { flex: none; color: var(--text-4); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .st-proj {
    flex: none; max-width: 90px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    color: var(--text-3); border: 1px solid var(--line-2); border-radius: 5px; padding: 0 5px;
  }
  .st-lane {
    flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    color: var(--text-4); font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10.5px;
  }
  .st-auto { flex: none; color: var(--text-3); font-size: 11px; line-height: 1; }
  .st-model {
    flex: none; max-width: 110px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    color: var(--text-3); font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10.5px;
  }
  .st-pin { flex: none; color: var(--text-3); font-size: 10px; }
  .prov-logo { width: 13px; height: 13px; flex: none; object-fit: contain; opacity: 0.8; }
  .disclosure {
    flex: none; width: 16px; margin-left: -4px; text-align: center;
    color: var(--text-3); font-size: 10px; cursor: pointer; border-radius: 4px;
  }
  .disclosure:hover { color: var(--text); background: var(--surface-3); }
  .branch { flex: none; color: var(--text-4); font-size: 11px; width: 12px; margin-left: -2px; text-align: center; }
  .kid-badge {
    flex: none; font-size: 10px; color: var(--text-3);
    border: 1px solid var(--line-3);
    border-radius: 5px; padding: 0 5px; font-variant-numeric: tabular-nums;
  }
  .t3-hover {
    display: none; position: absolute; top: 5px; right: 5px; z-index: 2;
    gap: 1px; align-items: center;
    background: var(--menu);
    border: 1px solid var(--line-2);
    border-radius: 7px; padding: 1px 2px;
    color: var(--text-3);
  }
  .t3:hover .t3-hover, .t3:focus-within .t3-hover { display: inline-flex; }
  .t3-hover span {
    display: inline-flex; align-items: center; height: 20px; padding: 0 6px;
    border-radius: 5px; cursor: pointer; font-size: 11px; line-height: 1;
  }
  .t3-hover span:hover { background: var(--surface-3); color: var(--text); }
  .t3-hover .stop:hover { background: var(--bad-soft); color: var(--bad); }
  .t3-hover .del:hover { background: var(--bad-soft); color: var(--bad); }
  .rename {
    flex: 1; min-width: 0; background: var(--surface-2);
    border: 1px solid var(--accent); border-radius: 6px; color: var(--text);
    font: inherit; font-size: 13px; padding: 1px 6px; outline: none;
  }
</style>
