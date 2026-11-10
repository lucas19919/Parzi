<script lang="ts" context="module">
  import { writable } from "svelte/store";
  import { readStrings, writeJSON } from "./storage";

  const KEY = "parzi.info.folded";
  const folded = writable<Set<string>>(new Set(readStrings(KEY)));
  folded.subscribe((s) => writeJSON(KEY, [...s]));

  /** Opening something in the sidebar always expands its card. */
  export function unfold(id: string) {
    folded.update((s) => {
      if (!s.has(id)) return s;
      const next = new Set(s);
      next.delete(id);
      return next;
    });
  }
</script>

<script lang="ts">
  import Icon from "./Icon.svelte";

  export let id: string;
  export let title: string;
  export let foldedAside = false;

  $: open = !$folded.has(id);
  function toggle() {
    folded.update((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }
</script>

<section class="card" class:open>
  <div class="bar">
    <button class="head" aria-expanded={open} on:click={toggle}>
      <span class="title t-section">{title}</span>
      <span class="aside">{#if !open || !foldedAside}<slot name="aside" />{/if}</span>
      <span class="chev"><Icon name="chevDown" size={12} /></span>
    </button>
    {#if open && $$slots.actions}<span class="actions"><slot name="actions" /></span>{/if}
  </div>
  {#if open}
    <div class="body"><slot /></div>
  {/if}
</section>

<style>
  .card {
    flex: none;
    background: color-mix(in srgb, var(--panel) var(--card-fill), transparent);
    border: 1px solid color-mix(in srgb, var(--line) var(--card-edge), transparent);
    -webkit-backdrop-filter: var(--card-blur);
    backdrop-filter: var(--card-blur);
    border-radius: var(--radius);
  }
  .bar {
    display: flex;
    align-items: center;
    border-radius: var(--radius);
  }
  .bar:hover {
    background: var(--hover);
  }
  .open .bar {
    border-radius: var(--radius) var(--radius) 0 0;
  }
  .actions {
    flex: none;
    display: inline-flex;
    gap: var(--space);
    padding-right: var(--space);
  }
  /* A button in the header nests in the card's corner. */
  .actions :global(.icon-btn) {
    border-radius: var(--radius-in);
  }
  .head {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    height: var(--h-row);
    padding: 0 calc(var(--space) * 3);
    background: none;
    border: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .title {
    flex: none;
  }
  .aside {
    flex: 1;
    min-width: 0;
    display: flex;
    justify-content: flex-end;
    overflow: hidden;
    white-space: nowrap;
    font-size: var(--fs-sm);
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .chev {
    display: inline-flex;
    flex: none;
    color: var(--muted);
    transform: rotate(-90deg);
    transition: transform 160ms var(--ease-out);
  }
  .open .chev {
    transform: none;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding: 0 calc(var(--space) * 3) calc(var(--space) * 3);
  }
</style>
