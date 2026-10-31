<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import type { ProviderStatus } from "./api";
  import { ageOf, checking, refreshBoard } from "./providerStore";
  import { PROVIDER_ORDER, allRows, chipOf, isUsable, nameOf, shownOf, type PickRow } from "./providerRows";
  import { recentModels } from "./tabs";
  import { popover, placeAbove } from "./popover";
  import { tooltip } from "./tooltip";

  export let value = "";
  export let board: ProviderStatus[] = [];

  const dispatch = createEventDispatcher<{ unavailable: { provider: string } }>();

  const FRESH_SECS = 300;
  const RETRY_SECS = 60;

  interface Group {
    key: string;
    label: string;
    rows: PickRow[];
  }

  let open = false;
  let query = "";
  let index = 0;
  let recent: string[] = [];
  let trigger: HTMLButtonElement | null = null;
  let searchEl: HTMLInputElement | null = null;
  let listEl: HTMLDivElement | null = null;
  let style = "";
  let above = true;

  $: shown = shownOf(value, board);
  $: q = query.trim().toLowerCase();
  $: groups = groupsFor(board, recent, q);
  $: flat = groups.flatMap((g) => g.rows);
  $: offsets = groups.map((_, gi) => groups.slice(0, gi).reduce((n, g) => n + g.rows.length, 0));
  $: if (index >= flat.length) index = 0;
  $: idle = board.filter((b) => !isUsable(b) && PROVIDER_ORDER.includes(b.provider));
  $: busy = $checking.size > 0;

  function matches(row: PickRow, q: string) {
    return !q || `${row.label} ${row.value} ${nameOf(row.provider)}`.toLowerCase().includes(q);
  }

  function groupsFor(board: ProviderStatus[], recent: string[], q: string): Group[] {
    const rows = allRows(board).filter((r) => r.usable && matches(r, q));
    const out: Group[] = [];
    const used = q ? [] : recent.map((v) => rows.find((r) => r.value === v)).filter((r): r is PickRow => !!r);
    if (used.length) out.push({ key: "recent", label: "Recent", rows: used });
    for (const id of PROVIDER_ORDER) {
      const mine = rows.filter((r) => r.provider === id);
      if (mine.length) out.push({ key: id, label: nameOf(id), rows: mine });
    }
    const extra: string[] = [];
    for (const row of rows) {
      if (!PROVIDER_ORDER.includes(row.provider) && !extra.includes(row.provider)) extra.push(row.provider);
    }
    for (const id of extra) {
      const mine = rows.filter((r) => r.provider === id);
      if (mine.length) out.push({ key: id, label: nameOf(id), rows: mine });
    }
    return out;
  }

  function labelOf(row: PickRow) {
    return row.value === row.provider ? "Default model" : row.label;
  }

  $: chip = value ? chipOf(shown.name) : { title: "Pick a model", version: "" };

  async function focusSearch() {
    await tick();
    searchEl?.focus();
    listEl?.querySelector(".row.picked")?.scrollIntoView({ block: "nearest" });
  }

  export function show() {
    query = "";
    open = true;
    if (trigger) {
      style = placeAbove(trigger, 280, true);
      above = style.includes("bottom:");
    }
    index = 0;
    const current = value.split("/")[0];
    const stale = board
      .filter((b) => isUsable(b) && (ageOf(b) >= (b.provider === current ? FRESH_SECS : Infinity) || (!b.models.length && ageOf(b) >= RETRY_SECS)))
      .map((b) => b.provider);
    if (stale.length) void refreshBoard(stale).catch(() => {});
    recent = recentModels();
    void focusSearch();
  }

  function pick(row: PickRow) {
    open = false;
    value = row.value;
  }

  function setupLabel(b: ProviderStatus) {
    switch (b.state) {
      case "not_installed":
        return "install";
      case "disabled":
        return "turn on";
      case "error":
        return "check failed";
      case "unchecked":
        return "checking";
      default:
        return "sign in";
    }
  }

  function setUp(provider: string) {
    open = false;
    dispatch("unavailable", { provider });
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      index = (index + step + flat.length) % Math.max(1, flat.length);
      void tick().then(() => listEl?.querySelector(".row.active")?.scrollIntoView({ block: "nearest" }));
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      if (flat[index]) pick(flat[index]);
    } else if (e.key === "Escape") {
      e.stopPropagation();
      open = false;
    }
  }
</script>

<button
  bind:this={trigger}
  class="model"
  class:open
  use:tooltip={shown.name}
  aria-haspopup="dialog"
  aria-expanded={open}
  on:click|stopPropagation={() => (open ? (open = false) : show())}
>
  {#if hasMark(shown.provider)}<ProviderLogo provider={shown.provider} size={14} mono />{/if}
  <span class="truncate">{chip.title}</span>
  {#if chip.version}<span class="ver">{chip.version}</span>{/if}
</button>

{#if open}
  <div
    class="picker"
    class:above
    role="dialog"
    aria-label="Model"
    {style}
    use:popover={{ anchor: trigger, close: () => (open = false) }}
    transition:fly={{ y: above ? 6 : -6, duration: 140, easing: cubicOut }}
  >
    <div class="search">
      <Icon name="search" size={12} />
      <input bind:this={searchEl} bind:value={query} on:keydown={onKey} aria-label="Search models" placeholder="Search models" spellcheck="false" />
      {#if busy}<span class="spin" use:tooltip={"Checking your agents"} />{/if}
    </div>
    <div class="list" bind:this={listEl} data-ui-list>
      {#each groups as g, gi (g.key)}
        <div class="head t-group">{g.label}</div>
        {#each g.rows as row, j (g.key + row.value)}
          {@const i = offsets[gi] + j}
          <button
            class="row"
            class:active={i === index}
            class:picked={row.value === value}
            on:click={() => pick(row)}
            on:mousemove={() => (index = i)}
          >
            <span class="logo">{#if hasMark(row.provider)}<ProviderLogo provider={row.provider} size={14} mono />{/if}</span>
            <span class="name">{labelOf(row)}</span>
            <span class="tick">{#if row.value === value}<Icon name="check" />{/if}</span>
          </button>
        {/each}
      {:else}
        {#if !idle.length}
          <div class="empty">{busy ? "Checking your agents…" : q ? "No matching models" : "No agent is ready yet"}</div>
        {/if}
      {/each}
      {#if idle.length}
        <button class="row idle" use:tooltip={idle.map((b) => `${nameOf(b.provider)}: ${setupLabel(b)}`).join(", ")} on:click={() => setUp(idle[0].provider)}>
          <span class="logos">
          {#each idle.slice(0, 3) as b (b.provider)}
            {#if hasMark(b.provider)}<ProviderLogo provider={b.provider} size={14} mono />{/if}
          {/each}
          </span>
          <span class="name">Set up {idle.length === 1 ? nameOf(idle[0].provider) : `${idle.length} more agents`}</span>
          <Icon name="chevRight" size={12} />
        </button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .model {
    flex: none;
    min-width: 0;
    max-width: 200px;
    gap: var(--space);
  }
  .ver {
    flex: none;
    color: var(--muted);
    font-size: var(--fs-md);
    font-variant-numeric: tabular-nums;
  }
  .model .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .picker {
    width: 280px;
    display: flex;
    flex-direction: column;
    gap: var(--space);
    padding: var(--space);
    transform-origin: bottom left;
  }
  .picker:not(.above) {
    transform-origin: top left;
  }
  .search {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    flex: none;
    height: 32px;
    padding: 0 calc(var(--space) * 2);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: none;
    outline: none;
    box-shadow: none !important;
    color: var(--text);
    font-size: var(--fs-md);
  }
  .spin {
    width: 12px;
    height: 12px;
    flex: none;
    border-radius: 50%;
    border: 2px solid var(--line);
    border-top-color: var(--muted);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .list {
    flex: 1;
    min-height: 0;
    max-height: 360px;
    overflow-y: auto;
  }
  .head {
    padding: calc(var(--space) * 2) calc(var(--space) * 2) var(--space);
              }
  .row {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    width: 100%;
    height: 32px;
    padding: 0 calc(var(--space) * 2);
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .row.picked {
    background: var(--selected);
  }
  .row.active {
    background: var(--hover);
  }
  .logo {
    display: inline-flex;
    width: 14px;
    flex: none;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tick {
    display: inline-flex;
    width: 16px;
    flex: none;
    color: var(--accent);
  }
  .row.idle {
    color: var(--muted);
  }
  .row.idle:hover {
    background: var(--hover);
    color: var(--text);
  }
  .logos {
    display: inline-flex;
    gap: var(--space);
    flex: none;
    opacity: 0.8;
  }
  .empty {
    padding: calc(var(--space) * 4);
    text-align: center;
    font-size: var(--fs-md);
    color: var(--muted);
  }
</style>
