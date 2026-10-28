<script lang="ts" context="module">
  import { writable } from "svelte/store";

  // Which cards are folded, kept across sessions and restarts.
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
      /* private window: folding still works for now */
    }
  });
</script>

<script lang="ts">
  // One card of the session column: a title that folds it, a short summary
  // beside the title, the details below.
  import Icon from "./Icon.svelte";

  export let id: string;
  export let title: string;
  // The summary only while folded, when the details repeat it.
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
    <span class="title">{title}</span>
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
    background: var(--glass-bg-solid);
    border: var(--glass-border);
    border-radius: var(--radius-lg);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 36px;
    padding: 0 12px 0 14px;
    background: none;
    border: none;
    border-radius: var(--radius-lg);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .head:hover {
    background: var(--hover);
  }
  /* Open: square the head's bottom into the body, so the hover fill never
     rounds off mid-card. Shut: the head is the whole card, fully round. */
  .open .head {
    border-radius: var(--radius-lg) var(--radius-lg) 0 0;
  }
  .title {
    flex: none;
    font-size: var(--fs-base);
    font-weight: 650;
    letter-spacing: -0.2px;
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
    gap: 8px;
    padding: 2px 12px 12px;
  }
</style>
