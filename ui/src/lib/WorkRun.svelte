<script lang="ts">
  import Icon from "./Icon.svelte";
  import Thinking from "./Thinking.svelte";
  import WorkGroup from "./WorkGroup.svelte";
  import type { RunPart } from "./timeline";
  import { runLine } from "./toolWords";

  /** A stretch of mixed work: one line, its groups folded under it. */
  export let blocks: RunPart[] = [];
  /** The newest thing in a running turn: open while it works, shut after. */
  export let live = false;

  let chosen: boolean | null = null;
  $: open = chosen ?? live;
  /** The groups are drawn once the run has been open, and kept for folding shut. */
  let drawn = false;
  $: if (open) drawn = true;

  $: groups = blocks.flatMap((b) => (b.kind === "group" ? [b] : []));
  $: running = groups.some((g) => g.items.some((i) => i.running));
  $: failed = groups.reduce((n, g) => n + g.items.filter((i) => !i.running && i.ok === false).length, 0);
  $: line = runLine(groups);
</script>

<div class="run">
  <button class="head" aria-expanded={open} on:click={() => (chosen = !open)}>
    <span class="chev" class:open><Icon name="chevDown" size={12} /></span>
    <span class="verb" class:shine={running}>{line}</span>
    {#if failed}<span class="bad">{failed} failed</span>{/if}
  </button>
  <div class="fold" class:open inert={!open}>
    <div class="fold-in">
      {#if drawn}
        <div class="parts">
          {#each blocks as b, i (b.key)}
            {#if b.kind === "group"}
              <WorkGroup tool={b.tool} items={b.items} live={live && i === blocks.length - 1} />
            {:else if b.kind === "thinking"}
              <Thinking text={b.text} live={live && i === blocks.length - 1} />
            {:else}
              <div class="echo">{b.text}</div>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .run {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .head {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    max-width: 100%;
    min-height: var(--h);
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .head:hover {
    color: var(--text);
  }
  .chev {
    display: inline-flex;
    flex: none;
    width: 16px;
    justify-content: center;
    color: var(--faint);
    transform: rotate(-90deg);
    transition: transform var(--dur) var(--ease-out);
  }
  .chev.open {
    transform: none;
  }
  .verb {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bad {
    flex: none;
    font-size: var(--fs-sm);
    color: var(--bad-text);
  }
  /* The groups sit on one rail under the line, as a group's steps do. */
  .parts {
    display: flex;
    flex-direction: column;
    margin-left: calc(var(--space) * 2);
    padding-left: calc(var(--space) * 4);
    border-left: 1px solid var(--line);
  }
  .echo {
    min-height: var(--h);
    display: flex;
    align-items: center;
    font-family: var(--mono);
    font-size: var(--fs-sm);
    color: var(--muted);
    overflow-wrap: anywhere;
  }
</style>
