<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";

  export let todo: unknown;
  export let working = false;

  type Item = { title: string; state: "pending" | "active" | "done" };
  $: items = (((todo ?? {}) as { items?: Item[] }).items ?? []).filter((i) => i && typeof i.title === "string");
  $: done = items.filter((i) => i.state === "done").length;
  $: finished = items.length > 0 && done === items.length;

  let open: boolean | null = null;
  // Done plans collapse away even if they were opened mid-run; reopening to review still works.
  $: if (finished) open = false;
  $: shown = open ?? !finished;
</script>

{#if items.length}
  <section class="todo" aria-label="Tasks">
    <button class="head" aria-expanded={shown} on:click={() => (open = !shown)}>
      <Icon name={shown ? "chevDown" : "chevRight"} size={12} />
      <span class="title t-section">Tasks</span>
      <span class="count">{done} of {items.length}</span>
    </button>
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

<style>
  .todo {
    margin: var(--space) 0 0;
    padding: calc(var(--space) * 2) calc(var(--space) * 2);
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .head {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    width: 100%;
    padding: var(--space) calc(var(--space) * 2) calc(var(--space) * 2);
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
    flex: 1;
  }
  .count {
    flex: none;
    font-variant-numeric: tabular-nums;
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
    min-height: 30px;
    padding: var(--space) calc(var(--space) * 2);
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
