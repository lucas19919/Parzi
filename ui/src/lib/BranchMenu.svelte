<script lang="ts">
  import { createEventDispatcher, onMount, tick } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import { api } from "./api";
  import { DUR } from "./motion";
  import { folderName } from "./tabs";
  import { placeAbove, popover } from "./popover";

  export let anchor: HTMLElement | null = null;
  export let folder = "";
  export let branch = "";
  export let sessionTitle = "";

  const dispatch = createEventDispatcher<{
    close: void;
    switchBranch: { branch: string };
    moveFolder: { path: string };
    pickFolder: void;
    error: { text: string };
  }>();

  let style = placeAbove(anchor, 276, true);
  let loading = true;
  let current = branch;
  let allBranches: string[] = [];
  let worktrees: { path: string; branch: string; current: boolean }[] = [];
  let query = "";
  let addingWt = false;
  let wtName = "";
  let busy = false;
  let wtInput: HTMLInputElement | null = null;

  const close = () => dispatch("close");

  onMount(async () => {
    try {
      const [b, w] = await Promise.all([
        api.gitBranches(folder).catch(() => ({ current: branch, all: [] })),
        api.gitWorktrees(folder).catch(() => []),
      ]);
      current = b.current || branch;
      allBranches = b.all ?? [];
      worktrees = w ?? [];
    } finally {
      loading = false;
    }
  });

  $: filtered = query.trim()
    ? allBranches.filter((b) => b.toLowerCase().includes(query.trim().toLowerCase()))
    : allBranches;

  function worktreeFor(b: string) {
    return worktrees.find((w) => w.branch === b);
  }

  function pickBranch(b: string) {
    if (busy) return;
    if (b === current) {
      close();
      return;
    }
    const wt = worktreeFor(b);
    if (wt && !wt.current) {
      close();
      dispatch("moveFolder", { path: wt.path });
      return;
    }
    close();
    dispatch("switchBranch", { branch: b });
  }

  async function startWorktree() {
    addingWt = true;
    wtName = sessionTitle.trim().slice(0, 32);
    await tick();
    wtInput?.focus();
    wtInput?.select();
  }

  async function createWorktree() {
    if (busy) return;
    busy = true;
    try {
      const path = await api.gitCreateWorktree(folder, current || "HEAD", wtName.trim() || "session");
      close();
      dispatch("moveFolder", { path });
    } catch (e) {
      dispatch("error", { text: String(e) });
    } finally {
      busy = false;
    }
  }
</script>

<div
  class="menu"
  role="menu"
  aria-label="Branch and worktree"
  {style}
  use:popover={{ anchor, close }}
  transition:fly={{ y: 6, duration: DUR, easing: cubicOut }}
>
  {#if loading}
    <div class="empty">Loading branches…</div>
  {:else}
    {#if worktrees.length > 1}
      <div class="sec">Worktrees</div>
      {#each worktrees as wt (wt.path)}
        <button
          class="item"
          class:on={wt.current}
          role="menuitemradio"
          aria-checked={wt.current}
          on:click={() => {
            close();
            if (!wt.current) dispatch("moveFolder", { path: wt.path });
          }}
        >
          <Icon name="folder" size={12} />
          <span class="label">
            <span class="name">{wt.branch}</span>
            <span class="sub">{folderName(wt.path)}</span>
          </span>
          {#if wt.current}<Icon name="check" size={12} />{/if}
        </button>
      {/each}
      <div class="sep" />
    {/if}

    {#if allBranches.length > 8}
      <div class="search">
        <input bind:value={query} placeholder="Filter branches…" aria-label="Filter branches" />
      </div>
    {/if}

    {#if filtered.length}
      <div class="sec">Branches</div>
      <div class="list">
        {#each filtered as b (b)}
          {@const wt = worktreeFor(b)}
          {@const isCurrent = b === current}
          <button
            class="item"
            class:on={isCurrent}
            role="menuitemradio"
            aria-checked={isCurrent}
            on:click={() => pickBranch(b)}
          >
            <Icon name="branch" size={12} />
            <span class="name mono">{b}</span>
            {#if wt && !wt.current}<span class="tag">worktree</span>{/if}
            {#if isCurrent}<Icon name="check" size={12} />{/if}
          </button>
        {/each}
      </div>
    {:else if allBranches.length === 0}
      <div class="empty">Not a git repository</div>
    {:else}
      <div class="empty">No matching branches</div>
    {/if}

    <div class="sep" />

    {#if addingWt}
      <form class="wt-form" on:submit|preventDefault={createWorktree}>
        <input
          bind:this={wtInput}
          bind:value={wtName}
          placeholder="Worktree name…"
          aria-label="New worktree name"
          disabled={busy}
        />
        <button class="btn primary" type="submit" disabled={busy}>{busy ? "…" : "Create"}</button>
      </form>
    {:else if allBranches.length > 0}
      <button class="item" role="menuitem" on:click={startWorktree}>
        <Icon name="plus" size={12} />
        <span class="name">New worktree from {current || "HEAD"}…</span>
      </button>
    {/if}

    <button
      class="item"
      role="menuitem"
      on:click={() => {
        close();
        dispatch("pickFolder");
      }}
    >
      <Icon name="folder" size={12} />
      <span class="name">Choose folder…</span>
    </button>
  {/if}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 60;
    width: 276px;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 0.5);
    padding: calc(var(--space) * 1.5);
    overflow-y: auto;
    background: var(--surface-raised);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    box-shadow: 0 10px 28px color-mix(in srgb, var(--scrim) 55%, transparent);
    -webkit-backdrop-filter: var(--blur);
    backdrop-filter: var(--blur);
    color: var(--text);
    font-size: var(--fs-sm);
  }
  .sec {
    padding: var(--space) calc(var(--space) * 2) calc(var(--space) * 0.5);
    color: var(--muted);
    font-size: var(--fs-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .list {
    max-height: 220px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 0.5);
  }
  .item {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    width: 100%;
    padding: calc(var(--space) * 1.5) calc(var(--space) * 2);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background var(--dur) ease;
  }
  .item:hover {
    background: var(--fill);
  }
  .item.on {
    background: var(--selected);
  }
  .item :global(svg) {
    flex: none;
    color: var(--muted);
  }
  .label {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    color: var(--muted);
    font-size: var(--fs-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mono {
    font-family: var(--mono);
    font-size: var(--fs-xs);
  }
  .tag {
    flex: none;
    padding: 0 calc(var(--space) * 1.5);
    background: var(--fill);
    border-radius: var(--radius-sm);
    color: var(--muted);
    font-size: var(--fs-xs);
  }
  .sep {
    height: 1px;
    margin: var(--space) 0;
    background: var(--edge);
  }
  .empty {
    padding: calc(var(--space) * 2);
    color: var(--muted);
    text-align: center;
  }
  .search {
    padding: var(--space);
  }
  .search input,
  .wt-form input {
    width: 100%;
    box-sizing: border-box;
    padding: calc(var(--space) * 1.5) calc(var(--space) * 2);
    background: var(--fill);
    border: 1px solid var(--edge);
    border-radius: var(--radius-sm);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-xs);
    outline: none;
  }
  .search input:focus,
  .wt-form input:focus {
    border-color: var(--line);
  }
  .wt-form {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 1.5);
    padding: var(--space);
  }
  .wt-form .btn {
    flex: none;
    height: var(--h-sm);
    padding: 0 calc(var(--space) * 2.5);
    font-size: var(--fs-xs);
  }
</style>
