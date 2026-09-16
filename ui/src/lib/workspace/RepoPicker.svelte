<script lang="ts" context="module">
  /** One row of the picker; `key` is what `selected` holds. */
  export interface PickItem {
    key: string;
    name: string;
    sub?: string;
    badge?: string;
  }
</script>

<script lang="ts">
  export let items: PickItem[] = [];
  export let selected: string[] = [];
  export let placeholder = "Search repos";
  export let empty = "Nothing to pick here.";
  export let loading = false;

  let q = "";

  $: needle = q.trim().toLowerCase();
  $: shown = needle
    ? items.filter(
        (i) =>
          i.name.toLowerCase().includes(needle) ||
          (i.sub ?? "").toLowerCase().includes(needle),
      )
    : items;

  function toggle(key: string) {
    selected = selected.includes(key)
      ? selected.filter((k) => k !== key)
      : [...selected, key];
  }
</script>

<div class="picker">
  <input
    class="search"
    type="search"
    {placeholder}
    bind:value={q}
    on:keydown={(e) => e.stopPropagation()}
  />
  <div class="list" role="listbox" aria-multiselectable="true" tabindex="-1">
    {#if loading}
      <div class="msg">Loading…</div>
    {:else if !shown.length}
      <div class="msg">{needle ? `Nothing matches “${q}”.` : empty}</div>
    {:else}
      {#each shown as it (it.key)}
        {@const on = selected.includes(it.key)}
        <button class="row" class:on role="option" aria-selected={on} on:click={() => toggle(it.key)}>
          <span class="box" class:on>{on ? "✓" : ""}</span>
          <span class="name">{it.name}</span>
          {#if it.sub}<span class="sub">{it.sub}</span>{/if}
          {#if it.badge}<span class="badge">{it.badge}</span>{/if}
        </button>
      {/each}
    {/if}
  </div>
  <div class="foot">{selected.length} selected</div>
</div>

<style>
  .picker { display: flex; flex-direction: column; gap: 8px; min-height: 0; }
  .search {
    background: var(--input); border: 1px solid var(--line-2); border-radius: var(--radius-2);
    color: var(--text); font: inherit; font-size: 13px; padding: 8px 11px;
  }
  .search:focus { outline: none; border-color: var(--accent-line); }
  .list {
    display: flex; flex-direction: column; gap: 2px; overflow-y: auto;
    max-height: 340px; min-height: 120px;
    border: 1px solid var(--line-2); border-radius: var(--radius-3); padding: 4px;
  }
  .row {
    display: flex; align-items: center; gap: 9px; width: 100%; text-align: left;
    background: transparent; border: none; border-radius: var(--radius-2);
    color: var(--text-2); font: inherit; font-size: 12.5px; padding: 7px 9px; cursor: pointer;
  }
  .row:hover { background: var(--surface-1); color: var(--text); }
  .row.on { background: var(--accent-soft); color: var(--text); }
  .box {
    flex: none; width: 15px; height: 15px; border-radius: 4px;
    border: 1px solid var(--line-3); display: inline-flex; align-items: center;
    justify-content: center; font-size: 10px; line-height: 1;
  }
  .box.on { background: var(--accent); border-color: transparent; color: var(--accent-ink); }
  /* The name is what a person picks by, so it gets the room it needs (up to
     60 % of the row) and the remote — the secondary line — truncates first. */
  .name {
    flex: 0 0 auto; max-width: 60%; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 550;
  }
  .sub { flex: 1 1 0; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-3); font-size: 11.5px; }
  .badge {
    flex: none; font-size: 10px; color: var(--text-3); border: 1px solid var(--line-3);
    border-radius: var(--radius-pill); padding: 1px 7px;
  }
  .msg { font-size: 12px; color: var(--text-4); padding: 14px 10px; text-align: center; }
  .foot { font-size: 11.5px; color: var(--text-3); }
</style>
