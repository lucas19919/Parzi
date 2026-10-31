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
  import { createEventDispatcher } from "svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import { tooltip } from "./tooltip";

  export let todo: unknown;
  export let working = false;
  // In the session column the card around it already has the title and count.
  export let docked = false;

  const dispatch = createEventDispatcher<{ dock: void }>();

  $: items = todoItems(todo);
  $: done = items.filter((i) => i.state === "done").length;

  let shown = true;
</script>

{#if todoOpen(todo)}
  {#if docked}
    <ul class="docked">
      {#each items as it, i (i)}
        <li class="item {it.state}">
          <span class="mark {it.state}" class:spin={working && it.state === "active"} aria-hidden="true" />
          <span class="t">{it.title}</span>
        </li>
      {/each}
    </ul>
  {:else}
    <section class="todo" aria-label="Tasks">
      <div class="bar">
        <button class="head" aria-expanded={shown} on:click={() => (shown = !shown)}>
          <span class="title t-section">Tasks</span>
          <span class="count t-hint">{done} of {items.length}</span>
          <Icon name={shown ? "chevDown" : "chevRight"} size={12} />
        </button>
        <button class="icon-btn" use:tooltip={"Move to the session column"} aria-label="Move tasks to the session column" on:click={() => dispatch("dock")}>
          <Icon name="close" size={12} />
        </button>
      </div>
      {#if shown}
        <ul transition:slide={{ duration: 160, easing: cubicOut }}>
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
{/if}

<style>
  .todo {
    margin: var(--space) 0 0;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .bar {
    display: flex;
    align-items: center;
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
    color: var(--muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .head:hover {
    color: var(--text);
  }
  .title {
    flex: 1;
  }
  .count {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  .todo ul {
    padding: 0 calc(var(--space) * 3) calc(var(--space) * 2);
  }
  ul {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .item {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    min-height: var(--h);
    color: var(--text);
    font-size: var(--fs-md);
  }
  /* In a side card the list reads as that card's lines. */
  .docked {
    gap: calc(var(--space) * 2);
  }
  .docked .item {
    gap: calc(var(--space) * 2);
    min-height: var(--h-sm);
    font-size: var(--fs-sm);
  }
  .docked .mark {
    width: 12px;
    height: 12px;
  }
  .docked .mark.done::after {
    left: 2.5px;
    top: 0.5px;
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
    width: 14px;
    height: 14px;
    flex: none;
    border: 1.5px solid var(--muted);
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
    left: 3.5px;
    top: 1.5px;
    width: 3px;
    height: 6px;
    border: solid var(--bg);
    border-width: 0 1.5px 1.5px 0;
    transform: rotate(45deg);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .mark.active.spin {
      animation: none;
    }
  }
</style>
