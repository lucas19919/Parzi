<script lang="ts">
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
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
    deleteThread: { id: string };
    contextMenu: { id: string; x: number; y: number };
  }>();

  /** Provider key for the row logo (`provider/model` → provider). */
  function provOf(m: SessionMeta): string {
    const model = m.model || "";
    if (model === "auto") return "auto";
    return model.split("/")[0] || "";
  }

  $: prov = provOf(t);

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

  /** Dim suffix: the git branch, when it adds info. The lane is internal
      runtime plumbing — never shown. Never the project — the group
      header already says it. */
  $: subLine = (branch || "").trim();

  /** Crisp 13px stroke icons for disclosure, branch marks and the hover rail. */
  const P = {
    chevR: "M9 18l6-6-6-6",
    chevD: "M6 9l6 6 6-6",
    sub: "M15 10l5 5-5 5M4 4v7a4 4 0 0 0 4 4h12",
    pin: "M12 17v5M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1z",
    pencil: "M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z",
    trash: "M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6",
    x: "M18 6 6 18M6 6l12 12",
  };

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

  /** Rail delete arms in place: first click asks, second deletes. No popup. */
  let armDelete = false;
  let armTimer: ReturnType<typeof setTimeout> | null = null;
  function disarm() {
    armDelete = false;
    if (armTimer) {
      clearTimeout(armTimer);
      armTimer = null;
    }
  }
  onDestroy(disarm);
  function onDel(e: Event) {
    e.stopPropagation();
    if (armDelete) {
      disarm();
      dispatch("deleteThread", { id: t.id });
      return;
    }
    armDelete = true;
    if (armTimer) clearTimeout(armTimer);
    armTimer = setTimeout(disarm, 3000);
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
      ><Icon d={shut ? P.chevR : P.chevD} size={12} /></span>
    {:else if depth > 0}
      <span class="branch" title="Subsession"><Icon d={P.sub} size={12} /></span>
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
      <span class="st-auto" title="Smart Auto"><Icon d="M12 3l1.9 5.6L19.5 10l-5.6 1.9L12 17.5l-1.9-5.6L4.5 10l5.6-1.4Z" size={12} /></span>
    {:else if hasMark(prov)}
      <span class="prov-logo" title={t.model}><ProviderLogo provider={prov} size={13} muted /></span>
    {/if}
    {#if t.pinned}<span class="st-pin" title="Pinned"><Icon d={P.pin} size={10} /></span>{/if}
  </div>
  <span class="t3-hover" data-act>
    {#if t.status === "active" || t.status === "queued"}
      <span
        role="button"
        tabindex="0"
        title="Stop run"
        class="stop"
        on:click|stopPropagation={() => dispatch("killRun", { id: t.id })}
        on:keydown={(e) => e.key === "Enter" && dispatch("killRun", { id: t.id })}><Icon d={P.x} size={13} /></span
      >
    {/if}
    <span
      role="button"
      tabindex="0"
      title={t.pinned ? "Unpin" : "Pin"}
      class:is-pinned={t.pinned}
      on:click|stopPropagation={() => dispatch("togglePin", { id: t.id })}
      on:keydown={(e) => e.key === "Enter" && dispatch("togglePin", { id: t.id })}><Icon d={P.pin} size={13} /></span
    >
    <span
      role="button"
      tabindex="0"
      title="Rename"
      on:click|stopPropagation={() => dispatch("startRename", { id: t.id })}
      on:keydown={(e) => e.key === "Enter" && dispatch("startRename", { id: t.id })}><Icon d={P.pencil} size={13} /></span
    >
    <span
      role="button"
      tabindex="0"
      title={armDelete ? "Click again to delete this thread" : "Delete thread"}
      class="del"
      class:armed={armDelete}
      on:click|stopPropagation={onDel}
      on:keydown={(e) => e.key === "Enter" && onDel(e)}>{#if armDelete}<span class="arm-text">sure?</span>{:else}<Icon d={P.trash} size={13} />{/if}</span
    >
  </span>
</div>

<style>
  /* Session row: title over a dim meta line. The workspace group header
     already names the project, so the row never repeats it. */
  .t3 {
    position: relative;
    display: flex; flex-direction: column; gap: 3px; width: 100%;
    box-sizing: border-box;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 10px;
    color: var(--text-2);
    font: inherit; padding: 10px 12px 10px 13px;
    cursor: pointer; text-align: left; outline: none;
  }
  .t3:hover { background: var(--surface-2); }
  .t3.on {
    background: var(--surface-3);
    box-shadow: inset 2.5px 0 0 var(--accent);
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
    color: var(--text-4);
  }
  .st-lane {
    flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    color: var(--text-4); font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 10.5px;
  }
  .st-auto { display: none; }
  .st-pin { display: inline-flex; flex: none; color: var(--text-3); }
  /* Row marks stay monochrome and quiet: visible on hover, focus or
     selection; full colour lives in the Omnibar, Titlebar and Inspector. */
  .prov-logo { display: none; }
  .t3:hover .prov-logo, .t3:focus-within .prov-logo, .t3.on .prov-logo { display: inline-flex; flex: none; color: var(--text-3); }
  .t3:hover .st-auto, .t3:focus-within .st-auto, .t3.on .st-auto { display: inline-flex; flex: none; color: var(--accent-text); }
  .prov-logo :global(svg) { filter: grayscale(1); opacity: 0.75; }
  .disclosure {
    flex: none; width: 16px; margin-left: -4px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-3); cursor: pointer; border-radius: 4px;
  }
  .disclosure:hover { color: var(--text); background: var(--surface-3); }
  .branch {
    flex: none; width: 12px; margin-left: -2px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-4);
  }
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
    display: inline-flex; align-items: center; justify-content: center;
    height: 20px; min-width: 24px; padding: 0 5px;
    border-radius: 5px; cursor: pointer; line-height: 1;
  }
  .t3-hover span:hover { background: var(--surface-3); color: var(--text); }
  .t3-hover span.is-pinned { color: var(--accent-text); }
  .t3-hover .del.armed, .t3-hover .del.armed:hover { background: var(--bad); color: #fff; }
  .arm-text { font-size: 11px; font-weight: 600; white-space: nowrap; }
  .t3-hover .stop:hover { background: var(--bad-soft); color: var(--bad); }
  .t3-hover .del:hover { background: var(--bad-soft); color: var(--bad); }
  .rename {
    flex: 1; min-width: 0; background: var(--surface-2);
    border: 1px solid var(--accent); border-radius: 6px; color: var(--text);
    font: inherit; font-size: 13px; padding: 1px 6px; outline: none;
  }
</style>
