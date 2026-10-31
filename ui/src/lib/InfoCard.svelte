<script lang="ts" context="module">
  import { writable } from "svelte/store";

  const KEY = "parzi.info.folded";
  function load(): string[] {
    try {
      const v = JSON.parse(localStorage.getItem(KEY) ?? "[]");
      return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
    } catch {
      return [];
    }
  }
  const folded = writable<Set<string>>(new Set(load()));
  folded.subscribe((s) => {
    try {
      localStorage.setItem(KEY, JSON.stringify([...s]));
    } catch {
    }
  });

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
  <button class="head" aria-expanded={open} on:click={toggle}>
    <span class="title t-section">{title}</span>
    <span class="aside">{#if !open || !foldedAside}<slot name="aside" />{/if}</span>
    <span class="chev"><Icon name="chevDown" size={12} /></span>
  </button>
  {#if open}
    <div class="body"><slot /></div>
  {/if}
</section>

<style>
  .card {
    flex: none;
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .head {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    width: 100%;
    height: var(--h-row);
    padding: 0 calc(var(--space) * 3);
    background: none;
    border: none;
    border-radius: var(--radius);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .head:hover {
    background: var(--hover);
  }
  .open .head {
    border-radius: var(--radius) var(--radius) 0 0;
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
