<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import ProviderLogo from "./ProviderLogo.svelte";
  import { hasMark } from "./providerMarks";
  import { api, type ProviderStatus } from "./api";
  import { ageOf, checking, refreshBoard } from "./providerStore";
  import { PROVIDER_ORDER, allRows, isUsable, nameOf, shownOf, type PickRow } from "./providerRows";
  import { effortLabel, effortsFor } from "./providerRows";
  import { popover, placeAbove } from "./popover";
  import { tooltip } from "./tooltip";

  export let value = "";
  export let board: ProviderStatus[] = [];
  // Two-way: parent binds its effort; picking here updates the composer.
  export let effort = "medium";

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
  let favorites: string[] = [];
  let trigger: HTMLButtonElement | null = null;
  let searchEl: HTMLInputElement | null = null;
  let listEl: HTMLDivElement | null = null;
  let style = "";
  let above = true;

  $: shown = shownOf(value, board);
  $: modelEfforts = effortsFor(value, board);
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
    const starred = favorites.map((v) => rows.find((r) => r.value === v)).filter((r): r is PickRow => !!r && matches(r, q));
    if (starred.length) out.push({ key: "starred", label: "Starred", rows: starred });
    for (const id of PROVIDER_ORDER) {
      const status = board.find((b) => b.provider === id);
      if (!status || !isUsable(status)) continue;
      const mine = rows.filter((r) => r.provider === id && !favorites.includes(r.value) && matches(r, q));
      if (mine.length) out.push({ key: id, label: nameOf(id), rows: mine });
    }
    return out;
  }

  function labelOf(row: PickRow) {
    return row.value === row.provider ? "Default model" : row.label;
  }

  // Long provider strings ("Muse Spark 1.3 Contributor Free") collapse to
  // the first two non-version words ("Muse Spark") so the pill never
  // crushes its neighbours.
  function shortModel(name: string) {
    if (!value) return "Pick a model";
    if (name.length <= 24) return name;
    const skip = new Set(["contributor", "free", "preview", "latest", "thinking"]);
    const keep: string[] = [];
    const parts = name.split(/\s+/);
    for (let i = 0; i < parts.length && keep.length < 2; i++) {
      const p = parts[i];
      if (/\d/.test(p) && i > 0) continue;
      if (skip.has(p.toLowerCase())) continue;
      keep.push(p);
    }
    const short = (keep.length ? keep : parts.slice(0, 2)).join(" ");
    return short.length > 22 ? `${short.slice(0, 22)}…` : short;
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
      style = placeAbove(trigger, 280, true);
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

  function setupLabel(b: ProviderStatus) {
    return b.state === "not_installed" ? "install" : b.state === "disabled" ? "turn on" : "sign in";
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
  use:tooltip={shown.name}
  aria-haspopup="dialog"
  aria-expanded={open}
  on:click|stopPropagation={() => (open ? (open = false) : show())}
>
  <span class="truncate">{shortModel(shown.name)}</span>
  <Icon name="chevDown" size={12} />
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
      <Icon name="search" />
      <input bind:this={searchEl} bind:value={query} on:keydown={onKey} aria-label="Search models" placeholder="Search models" spellcheck="false" />
      {#if busy}<span class="spin" use:tooltip={"Checking your agents"} />{/if}
    </div>
    <div class="list" bind:this={listEl}>
      {#each groups as g (g.key)}
        <div class="head">{g.label}</div>
        <div class="rows" data-ui-list>
          {#each g.rows as row (g.key + row.value)}
            {@const i = flat.indexOf(row)}
            <button
              class="row"
              class:active={i === index}
              class:picked={row.value === value}
              on:click={() => pick(row)}
              on:mousemove={() => (index = i)}
            >
              <span class="logo">{#if hasMark(row.provider)}<ProviderLogo provider={row.provider} size={14} />{/if}</span>
              <span class="name">{labelOf(row)}</span>
              {#if row.note}<span class="note">{row.note}</span>{/if}
              <span class="tick">{#if row.value === value}<Icon name="check" />{/if}</span>
            </button>
          {/each}
        </div>
      {:else}
        <div class="empty">{busy ? "Checking your agents…" : "No matching models"}</div>
      {/each}
    </div>
    {#if modelEfforts.length > 1 || (idle.length && !q)}
      <div class="foot">
        {#if modelEfforts.length > 1}
          <div class="effort-line">
            <span class="foot-label">Effort</span>
            <div class="effort-row" role="group" aria-label="Effort" data-ui-segmented>
              {#each modelEfforts as e (e)}
                <button class="effort" class:on={effort === e} aria-pressed={effort === e} on:click={() => (effort = e)}>
                  {effortLabel(e)}
                </button>
              {/each}
            </div>
          </div>
        {/if}
        {#if idle.length && !q}
          <!-- One row for every agent not ready yet; Settings sets them up. -->
          <button class="row idle" use:tooltip={idle.map((b) => `${nameOf(b.provider)}: ${setupLabel(b)}`).join(", ")} on:click={() => setUp(idle[0].provider)}>
            <span class="logos">
              {#each idle.slice(0, 3) as b (b.provider)}
                {#if hasMark(b.provider)}<ProviderLogo provider={b.provider} size={14} />{/if}
              {/each}
            </span>
            <span class="name">Set up {idle.length === 1 ? nameOf(idle[0].provider) : `${idle.length} more agents`}</span>
            <Icon name="chevRight" size={12} />
          </button>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .model {
    flex: none;
    min-width: 0;
    max-width: 200px;
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
    transform-origin: bottom left;
  }
  .picker:not(.above) {
    transform-origin: top left;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: none;
    height: 40px;
    padding: 0 12px;
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
    padding: 4px;
  }
  .head {
    padding: 10px 8px 4px;
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--muted);
  }
  .head:first-child {
    padding-top: 6px;
  }
  .rows {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 32px;
    padding: 0 8px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
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
  .note {
    flex: none;
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .tick {
    display: inline-flex;
    width: 16px;
    flex: none;
    color: var(--accent);
  }
  .foot {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: none;
    padding: 8px 4px 4px;
    border-top: 1px solid var(--line);
  }
  .effort-line {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 4px 0 8px;
  }
  .foot-label {
    flex: 1;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .effort-row {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 999px;
    background: var(--hover);
  }
  .effort {
    height: 24px;
    padding: 0 10px;
    background: transparent;
    border: none;
    border-radius: 999px;
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .effort:hover {
    color: var(--text);
  }
  .effort.on {
    background: var(--selected);
    color: var(--text);
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
    gap: 4px;
    flex: none;
    opacity: 0.8;
  }
  .empty {
    padding: 16px;
    text-align: center;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
</style>
