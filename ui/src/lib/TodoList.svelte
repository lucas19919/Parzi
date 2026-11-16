<script lang="ts" context="module">
  export type TodoItem = { title: string; state: "pending" | "active" | "done" };

  export function todoItems(todo: unknown): TodoItem[] {
    return (((todo ?? {}) as { items?: TodoItem[] }).items ?? []).filter((i) => i && typeof i.title === "string");
  }

  /** A list with work left. A finished one has nothing to say, so it goes away. */
  export function todoOpen(todo: unknown): boolean {
    return todoItems(todo).some((i) => i.state !== "done");
  }
</script>

<script lang="ts">
  import { SLIDE } from "./motion";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";

  export let todo: unknown;
  export let working = false;

  $: items = todoItems(todo);
  $: done = items.filter((i) => i.state === "done").length;

  let shown = true;
</script>

<!-- The session's plan: one quiet card, a title row that folds it, the list. -->
{#if todoOpen(todo)}
  <section class="todo" aria-label="Plan">
    <button class="head" aria-expanded={shown} on:click={() => (shown = !shown)}>
      <span class="title">Plan</span>
      <span class="count">{done} of {items.length}</span>
      <span class="chev" class:folded={!shown}><Icon name="chevDown" size={12} /></span>
    </button>
    {#if shown}
      <ul transition:slide={{ duration: SLIDE, easing: cubicOut }}>
        {#each items as it, i (i)}
          <li class="item {it.state}">
            <span class="mark {it.state}" class:spin={working && it.state === "active"} aria-hidden="true" />
            <span class="t">{it.title}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .todo {
    margin: var(--space) 0 0;
    background: var(--surface);
    -webkit-backdrop-filter: var(--blur);
    backdrop-filter: var(--blur);
    border-radius: var(--radius);
  }
  .head {
    width: 100%;
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    height: var(--h-field);
    padding: 0 calc(var(--space) * 3);
    background: none;
    border: none;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .head:hover {
    color: var(--text);
  }
  .title {
    font-weight: 600;
  }
  .count {
    flex: 1;
    font-variant-numeric: tabular-nums;
  }
  .chev {
    display: inline-flex;
    transition: transform var(--dur) ease;
  }
  .chev.folded {
    transform: rotate(-90deg);
  }
  ul {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0 calc(var(--space) * 3) calc(var(--space) * 2);
    list-style: none;
  }
  .item {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2.5);
    min-height: var(--h-sm);
    color: var(--text);
    font-size: var(--fs-md);
  }
  .item.done {
    color: var(--muted);
  }
  .item.active .t {
    font-weight: 600;
  }
  .t {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mark {
    position: relative;
    width: 12px;
    height: 12px;
    flex: none;
    box-sizing: border-box;
    border: 1.5px solid var(--faint);
    border-radius: 50%;
  }
  .mark.active {
    border-color: var(--accent);
  }
  .mark.active.spin {
    border-color: var(--line);
    border-top-color: var(--accent);
    animation: spin 0.9s linear infinite;
  }
  .mark.done {
    border-color: var(--ok);
    background: var(--ok);
  }
  .mark.done::after {
    content: "";
    position: absolute;
    left: 2.5px;
    top: 0.5px;
    width: 3px;
    height: 6px;
    border: solid var(--bg);
    border-width: 0 1.5px 1.5px 0;
    transform: rotate(45deg);
  }
  @media (prefers-reduced-motion: reduce) {
    .mark.active.spin {
      animation: none;
    }
  }
</style>
