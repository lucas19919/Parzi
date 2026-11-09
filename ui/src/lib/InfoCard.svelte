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

  // Cards that start shut are shut the first time only; after that the person's choice stands.
  const SEEN = "parzi.info.seen";
  const seen = new Set<string>(
    (() => {
      try {
        const v = JSON.parse(localStorage.getItem(SEEN) ?? "[]");
        return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
      } catch {
        return [];
      }
    })(),
  );
  function startShutOnce(id: string) {
    if (seen.has(id)) return;
    seen.add(id);
    try {
      localStorage.setItem(SEEN, JSON.stringify([...seen]));
    } catch {
    }
    folded.update((s) => new Set(s).add(id));
  }

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
  /** Closed until the person opens it once. */
  export let startShut = false;
  if (startShut) startShutOnce(id);

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
