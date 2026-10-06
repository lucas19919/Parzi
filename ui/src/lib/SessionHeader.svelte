<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  export let title = "";
  export let branch = "";
  export let lane = "";
  export let canAct = false;
  export let agentCount = 0;
  export let panelOpen = false;

  const dispatch = createEventDispatcher<{ rename: { title: string }; fork: void; copyId: void; delete: void; agents: void }>();

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
    const onDoc = (e: MouseEvent) => {
      if (menu && !menu.contains(e.target as Node)) open = false;
    };
    document.addEventListener("click", onDoc);
    return () => document.removeEventListener("click", onDoc);
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
      <span class="branch"><Icon name="branch" size={11} />{branch}</span>
    {/if}
    {#if lane === "research" || lane === "code"}
      <span class="lane" class:research={lane === "research"}>{lane === "research" ? "Research" : "Code"}</span>
    {/if}
  </div>
  {#if canAct}
    <div class="menu" bind:this={menu}>
      <button class="icon-btn" class:on={panelOpen} title="Subagents (Ctrl+Shift+A)" on:click={() => dispatch("agents")}>
        <Icon name="bot" size={14} />
        {#if agentCount > 0}<span class="count">{agentCount > 99 ? "99+" : agentCount}</span>{/if}
      </button>
      <button class="icon-btn" title="Session actions" aria-expanded={open} on:click={() => (open = !open)}>
        <Icon name="more" stroke={3} />
      </button>
      <button class="icon-btn" title="Session actions" aria-expanded={open} on:click={() => (open = !open)}>
        <Icon name="more" stroke={3} />
      </button>
      {#if open}
        <div class="dropdown" role="menu">
          {#each ITEMS as item (item.id)}
            {#if item.id === "delete"}<div class="sep" />{/if}
            <button class="item" class:danger={item.id === "delete"} role="menuitem" on:click={() => run(item.id)}>
              <Icon name={item.icon} size={13} />
              <span>{item.label}</span>
            </button>
          {/each}
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
    padding: 0 12px 0 20px;
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
    font-size: 14.5px;
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
    font-size: 11px;
    color: var(--muted);
  }
  .lane {
    flex: none;
    padding: 1px 7px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text) 8%, transparent);
    color: var(--muted);
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.2px;
  }
  .lane.research {
    background: color-mix(in srgb, #e8b64c 18%, transparent);
    color: #e8b64c;
  }
  .menu {
    position: relative;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .icon-btn {
    position: relative;
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: var(--muted);
    cursor: pointer;
  }
  .icon-btn:hover {
    background: var(--line);
    color: var(--text);
  }
  .icon-btn.on {
    background: var(--line);
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
    color: #06110a;
    font-size: 9px;
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
    border-radius: var(--radius);
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
    border-radius: 5px;
    color: var(--text);
    font-size: 12px;
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
</style>
