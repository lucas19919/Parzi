<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { api, type ProviderStatus } from "./api";
  import { ageOf, checking, refreshBoard } from "./providerStore";
  import { AUTO_ROW, PROVIDER_ORDER, allRows, isUsable, nameOf, rowSub, shownOf, type PickRow } from "./providerRows";
  import { popover, placeAbove } from "./popover";

  export let value = "auto";
  export let board: ProviderStatus[] = [];

  const dispatch = createEventDispatcher<{ unavailable: { provider: string } }>();

  const FRESH_SECS = 300;

  let open = false;
  let rail = "";
  let query = "";
  let index = 0;
  let favorites: string[] = [];
  let trigger: HTMLButtonElement | null = null;
  let searchEl: HTMLInputElement | null = null;
  let style = "";

  $: rows = allRows(board);
  $: q = query.trim().toLowerCase();
  $: favRows = favorites.map((v) => rows.find((r) => r.value === v)).filter((r): r is PickRow => !!r);
  $: items = listFor(q, rail, rows, favRows);
  $: if (index >= items.length) index = 0;
  $: shown = shownOf(value, board);
  $: railStatus = board.find((b) => b.provider === rail);
  $: busy = $checking.has(rail) || $checking.has("*");

  function listFor(q: string, rail: string, rows: PickRow[], favRows: PickRow[]): PickRow[] {
    if (q) {
      return [AUTO_ROW, ...rows].filter(
        (r) => r.label.toLowerCase().includes(q) || r.value.toLowerCase().includes(q) || nameOf(r.provider).toLowerCase().includes(q),
      );
    }
    if (rail === "__auto") return [AUTO_ROW];
    if (rail === "__fav") return favRows;
    return rows.filter((r) => r.provider === rail);
  }

  function freshen(p: string) {
    if (!p || p.startsWith("__")) return;
    if (ageOf(board.find((b) => b.provider === p)) < FRESH_SECS) return;
    void refreshBoard([p]).catch(() => {});
  }

  async function focusSearch() {
    await tick();
    searchEl?.focus();
  }

  function selectRail(p: string) {
    rail = p;
    query = "";
    index = 0;
    freshen(p);
    void focusSearch();
  }

  export function show() {
    const [p] = value.split("/");
    rail =
      value === "auto" || !value
        ? "__auto"
        : PROVIDER_ORDER.includes(p) && board.some((b) => b.provider === p)
          ? p
          : (PROVIDER_ORDER.find((id) => board.some((b) => b.provider === id)) ?? "__auto");
    query = "";
    index = 0;
    open = true;
    if (trigger) style = placeAbove(trigger, 440);
    freshen(rail);
    api
      .getConfig()
      .then((c) => (favorites = c.favorite_models ?? []))
      .catch(() => {});
    void focusSearch();
  }

  function pick(row: PickRow) {
    open = false;
    if (!row.usable) {
      dispatch("unavailable", { provider: row.provider });
      return;
    }
    value = row.value;
  }

  async function toggleFav(spec: string) {
    try {
      favorites = await api.toggleFavorite(spec);
    } catch {}
  }

  function onKey(e: KeyboardEvent) {
    if (e.ctrlKey && /^[1-5]$/.test(e.key)) {
      e.preventDefault();
      const row = items[Number(e.key) - 1];
      if (row) pick(row);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      index = (index + 1) % Math.max(1, items.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      index = (index - 1 + items.length) % Math.max(1, items.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      if (items[index]) pick(items[index]);
    } else if (e.key === "Escape") {
      e.stopPropagation();
      open = false;
    }
  }
</script>

<button
  bind:this={trigger}
  class="ctl"
  class:open
  title="Model"
  aria-expanded={open}
  on:click|stopPropagation={() => (open ? (open = false) : show())}
>
  <span class="truncate">{shown.name}</span>
  <Icon name="chevDown" size={10} />
</button>

{#if open}
  <div
    class="pop"
    {style}
    use:popover={{ anchor: trigger, close: () => (open = false) }}
    transition:scale={{ duration: 150, start: 0.97, easing: cubicOut }}
  >
    <div class="search">
      <Icon name="search" size={13} />
      <input
        bind:this={searchEl}
        bind:value={query}
        on:keydown={onKey}
        placeholder={rail === "__fav" ? "Starred models" : rail === "__auto" ? "Smart Auto" : `Search ${nameOf(rail)} models`}
      />
      {#if busy}<span class="spin" title="Asking the agent" />{/if}
    </div>
    <div class="body">
      <div class="rail">
        <button class="rail-row" class:on={rail === "__auto"} title="Smart Auto" on:click={() => selectRail("__auto")}>
          <span class="auto"><Icon name="spark" /></span>
        </button>
        {#if favRows.length}
          <button class="rail-row" class:on={rail === "__fav"} title="Starred" on:click={() => selectRail("__fav")}>
            <span class="star-rail">★</span>
          </button>
        {/if}
        {#each PROVIDER_ORDER as p}
          {@const status = board.find((b) => b.provider === p)}
          {#if status}
            <button
              class="rail-row"
              class:on={rail === p}
              class:dim={!isUsable(status)}
              title={isUsable(status) ? nameOf(p) : `${nameOf(p)}: ${status.hint}`}
              on:click={() => selectRail(p)}
            >
              {#if hasMark(p)}
                <ProviderLogo provider={p} size={18} />
              {:else}
                <span class="initial">{p.slice(0, 1).toUpperCase()}</span>
              {/if}
            </button>
          {/if}
        {/each}
      </div>
      <div class="list">
        {#if !q && railStatus && !isUsable(railStatus)}
          <button
            class="hint"
            on:click={() => {
              open = false;
              dispatch("unavailable", { provider: rail });
            }}
          >
            {railStatus.hint || "Not available"}
          </button>
        {/if}
        {#each items as row, i (row.value)}
          <button class="mrow" class:on={row.value === value || i === index} on:click={() => pick(row)} on:mousemove={() => (index = i)}>
            {#if q && hasMark(row.provider)}
              <ProviderLogo provider={row.provider} size={14} />
            {/if}
            <span class="meta">
              <span class="name">{row.label}</span>
              <span class="sub">{rowSub(row, board)}</span>
            </span>
            {#if i < 5}<kbd>Ctrl+{i + 1}</kbd>{/if}
            {#if !row.usable}<span class="unavailable">unavailable</span>{/if}
            {#if row.provider !== "auto"}
              <span
                class="star"
                class:on={favorites.includes(row.value)}
                role="button"
                tabindex="0"
                title="Star model"
                on:click|stopPropagation={() => toggleFav(row.value)}
                on:keydown={(e) => e.key === "Enter" && toggleFav(row.value)}>{favorites.includes(row.value) ? "★" : "☆"}</span
              >
            {/if}
          </button>
        {:else}
          <div class="empty">{busy ? "Asking the agent…" : q ? "No matching models" : "No models. Open Settings › Providers"}</div>
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .pop {
    width: 340px;
    height: 380px;
    display: flex;
    flex-direction: column;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
    padding: 8px 10px;
    color: var(--muted);
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
  }
  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .rail {
    width: 46px;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 5px 4px;
    overflow-y: auto;
    border-right: 1px solid var(--line);
  }
  .rail-row {
    height: 32px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 7px;
    color: var(--muted);
    cursor: pointer;
  }
  .rail-row:hover {
    background: var(--line);
    color: var(--text);
  }
  .rail-row.on,
  .mrow.on {
    background: color-mix(in srgb, var(--accent) 12%, var(--line));
    color: var(--text);
  }
  .rail-row.dim {
    opacity: 0.4;
  }
  .auto {
    display: inline-flex;
    color: var(--accent);
  }
  .star-rail {
    color: var(--warn);
    font-size: 15px;
    line-height: 1;
  }
  .initial {
    width: 20px;
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 6px;
    background: var(--line);
    font-size: 10px;
    font-weight: 600;
  }
  .list {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 4px;
    overflow-y: auto;
  }
  .hint {
    margin: 2px 2px 6px;
    padding: 8px 10px;
    background: var(--line);
    border: 1px solid var(--accent);
    border-radius: 7px;
    color: var(--accent);
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .mrow {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 6px 8px;
    background: transparent;
    border: none;
    border-radius: 7px;
    color: var(--muted);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .mrow:hover {
    background: var(--line);
    color: var(--text);
  }
  .meta {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .name,
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    font-size: 11px;
    color: var(--muted);
  }
  kbd {
    flex: none;
    padding: 1px 5px;
    font-size: 10px;
    color: var(--muted);
    background: transparent;
    border: 1px solid var(--line);
    border-radius: 4px;
    opacity: 0;
  }
  .mrow:hover kbd,
  .mrow.on kbd {
    opacity: 1;
  }
  .unavailable {
    flex: none;
    font-size: 11px;
    color: var(--bad);
  }
  .star {
    flex: none;
    padding: 1px 4px;
    font-size: 13px;
    color: var(--faint);
    cursor: pointer;
    opacity: 0;
  }
  .mrow:hover .star,
  .star.on {
    opacity: 1;
  }
  .star.on {
    color: var(--warn);
  }
  .empty {
    padding: 14px;
    text-align: center;
    font-size: 12px;
    color: var(--muted);
  }
</style>
