<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { api, type ProviderStatus } from "./api";
  import { ageOf, checking, refreshBoard } from "./providerStore";
  import { AUTO_ROW, PROVIDER_ORDER, allRows, isUsable, nameOf, shownOf, stateLabel, type PickRow } from "./providerRows";
  import { popover, placeAbove } from "./popover";

  export let value = "auto";
  export let board: ProviderStatus[] = [];

  const dispatch = createEventDispatcher<{ unavailable: { provider: string } }>();

  const FRESH_SECS = 300;
  const RETRY_SECS = 60;

  interface Group {
    key: string;
    label: string;
    provider: string;
    note: string;
    rows: PickRow[];
  }

  let open = false;
  let query = "";
  let index = 0;
  let favorites: string[] = [];
  let trigger: HTMLButtonElement | null = null;
  let searchEl: HTMLInputElement | null = null;
  let listEl: HTMLDivElement | null = null;
  let style = "";
  let above = true;

  $: shown = shownOf(value, board);
  $: q = query.trim().toLowerCase();
  $: groups = groupsFor(board, favorites, q);
  $: flat = groups.flatMap((g) => g.rows);
  $: if (index >= flat.length) index = 0;
  $: idle = board.filter((b) => !isUsable(b) && PROVIDER_ORDER.includes(b.provider));
  $: busy = $checking.size > 0;

  function matches(row: PickRow, q: string) {
    return !q || `${row.label} ${row.value} ${nameOf(row.provider)}`.toLowerCase().includes(q);
  }

  function groupsFor(board: ProviderStatus[], favorites: string[], q: string): Group[] {
    const rows = allRows(board).filter((r) => r.usable);
    const out: Group[] = [];
    if (matches(AUTO_ROW, q)) out.push({ key: "auto", label: "", provider: "", note: "", rows: [AUTO_ROW] });
    const starred = favorites.map((v) => rows.find((r) => r.value === v)).filter((r): r is PickRow => !!r && matches(r, q));
    if (starred.length) out.push({ key: "starred", label: "Starred", provider: "", note: "", rows: starred });
    for (const id of PROVIDER_ORDER) {
      const status = board.find((b) => b.provider === id);
      if (!status || !isUsable(status)) continue;
      const mine = rows.filter((r) => r.provider === id && matches(r, q));
      if (mine.length) out.push({ key: id, label: nameOf(id), provider: id, note: stateLabel(status), rows: mine });
    }
    return out;
  }

  function labelOf(row: PickRow) {
    return row.provider !== "auto" && row.value === row.provider ? "Default model" : row.label;
  }

  async function focusSearch() {
    await tick();
    searchEl?.focus();
    listEl?.querySelector(".row.picked")?.scrollIntoView({ block: "nearest" });
  }

  export function show() {
    query = "";
    open = true;
    if (trigger) {
      style = placeAbove(trigger, 300);
      above = style.includes("bottom:");
    }
    index = 0;
    const current = value.split("/")[0];
    const stale = board
      .filter((b) => isUsable(b) && (ageOf(b) >= (b.provider === current ? FRESH_SECS : Infinity) || (!b.models.length && ageOf(b) >= RETRY_SECS)))
      .map((b) => b.provider);
    if (stale.length) void refreshBoard(stale).catch(() => {});
    api
      .getConfig()
      .then((c) => (favorites = c.favorite_models ?? []))
      .catch(() => {});
    void focusSearch();
  }

  function pick(row: PickRow) {
    open = false;
    value = row.value;
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
  class="ctl model"
  class:open
  title={shown.name}
  aria-expanded={open}
  on:click|stopPropagation={() => (open ? (open = false) : show())}
>
  <span class="truncate">{shown.name}</span>
  <Icon name="chevDown" size={10} />
</button>

{#if open}
  <div
    class="picker"
    class:above
    {style}
    use:popover={{ anchor: trigger, close: () => (open = false) }}
    transition:fly={{ y: above ? 6 : -6, duration: 140, easing: cubicOut }}
  >
    <div class="search">
      <Icon name="search" size={13} />
      <input bind:this={searchEl} bind:value={query} on:keydown={onKey} placeholder="Search models" spellcheck="false" />
      {#if busy}<span class="spin" title="Checking your agents" />{/if}
    </div>
    <div class="list" bind:this={listEl}>
      {#each groups as g (g.key)}
        {#if g.label}
          <div class="head">
            {#if g.provider && hasMark(g.provider)}<ProviderLogo provider={g.provider} size={13} />{/if}
            <span>{g.label}</span>
            {#if g.note}<span class="note">{g.note}</span>{/if}
          </div>
        {/if}
        {#each g.rows as row (g.key + row.value)}
          {@const i = flat.indexOf(row)}
          <button
            class="row"
            class:active={i === index}
            class:picked={row.value === value}
            on:click={() => pick(row)}
            on:mousemove={() => (index = i)}
          >
            {#if row.provider === "auto"}<span class="spark"><Icon name="spark" size={14} /></span>{/if}
            <span class="name">{labelOf(row)}</span>
            {#if row.provider === "auto"}<span class="note">First ready agent</span>{:else if row.note}<span class="note">{row.note}</span>{/if}
            {#if row.value === value}<span class="tick"><Icon name="check" size={13} stroke={2} /></span>{/if}
          </button>
        {/each}
      {:else}
        <div class="empty">{busy ? "Checking your agents…" : "No matching models"}</div>
      {/each}
      {#if idle.length && !q}
        <div class="head">Not set up</div>
        {#each idle as b (b.provider)}
          <button class="row idle" on:click={() => setUp(b.provider)}>
            {#if hasMark(b.provider)}<ProviderLogo provider={b.provider} size={13} />{/if}
            <span class="name">{nameOf(b.provider)}</span>
            <span class="note">{b.state === "not_installed" ? "Install" : "Sign in"}</span>
          </button>
        {/each}
      {/if}
    </div>
  </div>
{/if}

<style>
  .model {
    flex-shrink: 0;
    max-width: 280px;
  }
  .picker {
    width: 300px;
    display: flex;
    flex-direction: column;
    transform-origin: bottom left;
  }
  .picker:not(.above) {
    transform-origin: top left;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
    padding: 9px 12px;
    color: var(--faint);
    border-bottom: 1px solid var(--line);
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: none;
    outline: none;
    box-shadow: none !important;
    color: var(--text);
    font-size: 13px;
  }
  .search input::placeholder {
    color: var(--faint);
  }
  .spin {
    width: 11px;
    height: 11px;
    flex: none;
    border-radius: 50%;
    border: 1.6px solid var(--line);
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
    max-height: 420px;
    overflow-y: auto;
    padding: 4px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 10px 8px 4px;
    font-size: 11px;
    font-weight: 600;
    color: var(--faint);
  }
  .head .note {
    margin-left: auto;
    font-weight: 400;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 32px;
    padding: 0 8px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .row.active {
    background: color-mix(in srgb, var(--text) 7%, transparent);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .note {
    flex: none;
    font-size: 11.5px;
    color: var(--faint);
  }
  .spark {
    display: inline-flex;
    color: var(--accent);
  }
  .tick {
    display: inline-flex;
    color: var(--accent);
  }
  .row.idle {
    color: var(--muted);
  }
  .row.idle .note {
    color: var(--accent);
  }
  .empty {
    padding: 16px;
    text-align: center;
    font-size: 12px;
    color: var(--faint);
  }
</style>
