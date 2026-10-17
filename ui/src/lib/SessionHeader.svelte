<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import ContextBreakdown from "./ContextBreakdown.svelte";
  import type { IconName } from "./icons";
  import { tooltip } from "./tooltip";

  export let title = "";
  export let branch = "";
  export let lane = "";
  export let folder = "";
  export let canAct = false;
  export let agentCount = 0;
  export let panelOpen = false;
  export let contextUsed = 0;
  export let contextLimit = 0;
  export let compacting = false;
  export let streaming = false;
  export let costUsd = 0;
  // Empty when the agent compacts on its own: that agent's name, else "".
  export let selfCompacts = "";

  const dispatch = createEventDispatcher<{ rename: { title: string }; fork: void; copyId: void; delete: void; plan: void; back: void; compact: void }>();

  type Action = "rename" | "fork" | "copyId" | "delete";
  const ITEMS: { id: Action; label: string; icon: IconName }[] = [
    { id: "rename", label: "Rename", icon: "pencil" },
    { id: "fork", label: "Fork", icon: "fork" },
    { id: "copyId", label: "Copy session ID", icon: "copy" },
    { id: "delete", label: "Delete", icon: "trash" },
  ];

  let open = false;
  let menu: HTMLDivElement | null = null;
  let editing = false;
  let draft = "";
  let field: HTMLInputElement | null = null;

  const RING = 2 * Math.PI * 6;
  $: contextPct = contextLimit > 0 ? Math.min(100, Math.round((contextUsed / contextLimit) * 100)) : 0;
  $: agents = agentCount > 99 ? "99+" : String(agentCount);
  let ctxOpen = false;

  async function run(id: Action) {
    open = false;
    if (id === "rename") {
      draft = title;
      editing = true;
      await tick();
      field?.select();
    } else if (id === "fork") dispatch("fork");
    else if (id === "copyId") dispatch("copyId");
    else dispatch("delete");
  }

  function commit() {
    if (!editing) return;
    editing = false;
    const next = draft.trim();
    if (next && next !== title) dispatch("rename", { title: next });
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter") commit();
    else if (e.key === "Escape") {
      e.stopPropagation();
      editing = false;
    }
  }

  onMount(() => {
    // One popup at a time; a click anywhere else closes it.
    const onDoc = (e: MouseEvent) => {
      if (menu && !menu.contains(e.target as Node)) open = ctxOpen = false;
    };
    // Capture phase, so Escape closes a popup before App reads it as "stop the run".
    const onEsc = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !(open || ctxOpen)) return;
      e.preventDefault();
      open = ctxOpen = false;
    };
    document.addEventListener("click", onDoc);
    window.addEventListener("keydown", onEsc, true);
    return () => {
      document.removeEventListener("click", onDoc);
      window.removeEventListener("keydown", onEsc, true);
    };
  });
</script>

<div class="head">
  <div class="left">
    {#if editing}
      <input bind:this={field} bind:value={draft} on:keydown={onKey} on:blur={commit} aria-label="Session title" />
    {:else}
      <h2>{title || "New session"}</h2>
    {/if}
    {#if branch}
      <span class="branch"><Icon name="branch" size={12} />{branch}</span>
    {/if}
    {#if lane === "work" || lane === "build"}
      {@const work = lane === "work"}
      <span class="lane" class:worklane={work}>{work ? "Work" : "Build"}</span>
    {/if}
  </div>
  {#if canAct}
    <div class="menu" bind:this={menu}>
      {#if contextLimit > 0}
        <button
          class="icon-btn ctx"
          class:warn={contextPct >= 70}
          class:bad={contextPct >= 90}
          disabled={compacting || streaming}
          aria-haspopup="dialog"
          aria-expanded={ctxOpen}
          use:tooltip={compacting ? "Compacting…" : `Context ${contextPct}% full. Click for breakdown.`}
          on:click={() => {
            ctxOpen = !ctxOpen;
            open = false;
          }}
        >
          <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
            <circle class="track" cx="8" cy="8" r="6" />
            <circle class="fill" cx="8" cy="8" r="6" stroke-dasharray={RING} stroke-dashoffset={compacting ? RING * 0.7 : RING * (1 - contextPct / 100)} />
          </svg>
        </button>
      {/if}
      <!-- The badge is visible text, so the name has to include it. -->
      <button
        class="icon-btn"
        class:on={panelOpen}
        aria-pressed={panelOpen}
        aria-label={agentCount > 0 ? `Plan, ${agents} agents running` : "Plan"}
        use:tooltip={"Plan (Ctrl+J)"}
        on:click={() => dispatch("plan")}
      >
        <Icon name="panel" />
        {#if agentCount > 0}<span class="count">{agents}</span>{/if}
      </button>
      <button class="icon-btn" aria-haspopup="menu" aria-expanded={open} use:tooltip={"Session actions"} on:click={() => {
          open = !open;
          ctxOpen = false;
        }}>
        <Icon name="more" />
      </button>
      {#if open}
        <div class="dropdown" role="menu" data-ui-list>
          {#each ITEMS as item (item.id)}
            {#if item.id === "delete"}<div class="sep" />{/if}
            <button class="item" class:danger={item.id === "delete"} role="menuitem" on:click={() => run(item.id)}>
              <Icon name={item.icon} />
              <span>{item.label}</span>
            </button>
          {/each}
        </div>
      {/if}
      {#if ctxOpen && contextLimit > 0}
        <div class="dropdown ctx-pop" role="dialog" aria-label="Context">
          <div class="ctx-head"><span>Context</span>{#if costUsd > 0}<span class="ctx-cost">${costUsd.toFixed(2)}</span>{/if}</div>
          <ContextBreakdown
            used={contextUsed}
            limit={contextLimit}
            {lane}
            {folder}
            {compacting}
            {streaming}
            {selfCompacts}
            on:compact={() => {
              ctxOpen = false;
              dispatch("compact");
            }}
          />
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .head {
    height: 42px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 14px 0 20px;
    border-bottom: 1px solid var(--line);
    user-select: none;
  }
  .left {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex: 1;
  }
  h2,
  input {
    margin: 0;
    min-width: 0;
    font-size: var(--fs-base);
    font-weight: 600;
    letter-spacing: -0.2px;
    color: var(--text);
  }
  h2 {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  input {
    flex: 1;
    max-width: 420px;
    padding: 2px 6px;
    background: var(--bg);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
  }
  .branch {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .lane {
    flex: none;
    padding: 1px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text) 8%, transparent);
    color: var(--muted);
    font-size: var(--fs-xs);
    font-weight: 600;
    letter-spacing: 0.2px;
  }
  .lane.worklane {
    background: color-mix(in srgb, var(--work) 18%, transparent);
    color: var(--work);
  }
  .menu {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .icon-btn {
    position: relative;
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .icon-btn:hover {
    background: var(--hover);
    color: var(--text);
  }
  .icon-btn.on {
    background: var(--selected);
    color: var(--text);
  }
  .count {
    position: absolute;
    top: 0;
    right: -1px;
    min-width: 14px;
    height: 14px;
    padding: 0 3px;
    border-radius: 999px;
    background: var(--ok);
    color: var(--on-ok);
    font-size: var(--fs-xs);
    font-weight: 700;
    line-height: 14px;
    text-align: center;
  }
  .dropdown {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    width: 180px;
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    z-index: 500;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .item:hover {
    background: var(--line);
  }
  .item.danger:hover {
    background: transparent;
    color: var(--bad);
  }
  .sep {
    height: 1px;
    margin: 3px 0;
    background: var(--line);
  }
  .ctx { --ctx: var(--faint); }
  .ctx:disabled { opacity: 0.55; cursor: default; }
  .ctx svg { transform: rotate(-90deg); }
  .ctx .track { fill: none; stroke: var(--line); stroke-width: 2; }
  .ctx .fill { fill: none; stroke: var(--ctx); stroke-width: 2; stroke-linecap: round; }
  .ctx.warn { --ctx: var(--warn); }
  .ctx.bad { --ctx: var(--bad); }
  .ctx-pop { padding: 12px 14px 14px; width: 280px; gap: 10px; }
  .ctx-head { display: flex; justify-content: space-between; font-size: var(--fs-sm); font-weight: 600; color: var(--muted); }
  .ctx-cost { font-weight: 400; font-variant-numeric: tabular-nums; }
</style>
