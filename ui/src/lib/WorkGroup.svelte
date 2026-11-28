<script lang="ts">
  import { duration } from "./format";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";
  import { fenced, mdHtml, richReady } from "./md";
  import type { ToolItem } from "./timeline";
  import { groupLine, type ToolKind } from "./toolWords";
  import { tooltip } from "./tooltip";

  /** Steps of one kind in a row: "Read 5 files", opened into a tree. */
  export let tool: ToolKind;
  export let items: ToolItem[] = [];
  /** The newest thing in a running turn: open while it works. */
  export let live = false;

  const ICON: Record<ToolKind, IconName> = {
    read: "file",
    edit: "pencil",
    search: "search",
    list: "folder",
    run: "terminal",
    web: "globe",
    browser: "browser",
    helper: "bot",
    plan: "plan",
    other: "wrench",
  };

  /** What the person chose; until then it opens while live and closes after. */
  let chosen: boolean | null = null;
  let shown = new Set<string>();

  $: running = items.some((i) => i.running);
  $: failed = items.filter((i) => !i.running && i.ok === false).length;
  $: one = items.length === 1 ? items[0] : null;
  $: open = chosen ?? live;
  /** The tree is drawn once it has been open, and kept for folding shut. */
  let drawn = false;
  $: if (open) drawn = true;
  $: line = groupLine(tool, items, running);

  function toggle(id: string) {
    shown = new Set(shown);
    if (shown.has(id)) shown.delete(id);
    else shown.add(id);
  }
</script>

<div class="group">
  {#if one}
    <button class="head" disabled={!one.output} aria-expanded={one.output ? shown.has(one.id) : undefined} on:click={() => toggle(one.id)}>
      <span class="ico"><Icon name={ICON[tool]} size={14} /></span>
      <span class="verb" class:shine={one.running}>{one.running ? one.active : one.done}</span>
      {#if one.target}<span class="chip" class:code={one.code} use:tooltip={one.full !== one.target ? one.full : ""}>{one.target}</span>{/if}
      {#if !one.running && one.ok === false}<span class="bad">Failed</span>{:else if one.ms && one.ms >= 1000}<span class="ms">{duration(one.ms)}</span>{/if}
    </button>
    {#if one.output && shown.has(one.id)}
      <div class="output solo">{@html mdHtml(fenced(one.output.slice(0, 6000), "text"), $richReady)}</div>
    {/if}
  {:else}
    <button class="head" aria-expanded={open} on:click={() => (chosen = !open)}>
      <span class="chev" class:open><Icon name="chevDown" size={12} /></span>
      <span class="verb" class:shine={running}>{line}</span>
      {#if failed}<span class="bad">{failed} failed</span>{/if}
    </button>
    <!-- Folds by its rows' height in CSS: nothing to finish if the turn moves on mid-fold. -->
    <div class="fold" class:open inert={!open}>
      <div class="fold-in">
      {#if drawn}
      <div class="tree" data-ui-list>
        {#each items as it (it.id)}
          <div class="branch">
            <button class="item" disabled={!it.output} aria-expanded={it.output ? shown.has(it.id) : undefined} on:click={() => toggle(it.id)}>
              <span class="ico"><Icon name={ICON[it.kind]} size={14} /></span>
              <span class="verb" class:shine={it.running}>{it.running ? it.active : it.done}</span>
              {#if it.target}<span class="chip" class:code={it.code} use:tooltip={it.full !== it.target ? it.full : ""}>{it.target}</span>{/if}
              {#if !it.running && it.ok === false}<span class="bad">Failed</span>{:else if it.ms && it.ms >= 1000}<span class="ms">{duration(it.ms)}</span>{/if}
            </button>
            {#if it.output && shown.has(it.id)}
              <div class="output">{@html mdHtml(fenced(it.output.slice(0, 6000), "text"), $richReady)}</div>
            {/if}
          </div>
        {/each}
      </div>
      {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .group {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .head,
  .item {
    display: flex;
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
  .head {
    align-self: flex-start;
  }
  .head:disabled,
  .item:disabled {
    cursor: default;
  }
  .head:hover:not(:disabled),
  .item:hover:not(:disabled) {
    color: var(--text);
  }
  .ico,
  .chev {
    display: inline-flex;
    flex: none;
    width: 16px;
    justify-content: center;
    color: var(--faint);
  }
  .chev {
    transform: rotate(-90deg);
    transition: transform var(--dur) var(--ease-out);
  }
  .chev.open {
    transform: none;
  }
  .verb {
    flex: none;
    white-space: nowrap;
  }
  .chip {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0 calc(var(--space) * 1.5);
    line-height: 20px;
    background: var(--fill);
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: var(--fs-sm);
  }
  .chip.code {
    font-family: var(--mono);
    font-size: var(--fs-xs);
  }
  .ms {
    flex: none;
    font-size: var(--fs-xs);
    color: var(--faint);
    font-variant-numeric: tabular-nums;
    opacity: 0;
    transition: opacity var(--dur) ease;
  }
  .head:hover .ms,
  .item:hover .ms {
    opacity: 1;
  }
  .bad {
    flex: none;
    font-size: var(--fs-sm);
    color: var(--bad-text);
  }
  /* A tree under the line: one rail, a short tick into each step. */
  .tree {
    display: flex;
    flex-direction: column;
    margin-left: calc(var(--space) * 2);
    padding-left: calc(var(--space) * 4);
    border-left: 1px solid var(--line);
  }
  .branch {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .branch::before {
    content: "";
    position: absolute;
    top: calc(var(--h) / 2);
    left: calc(var(--space) * -4);
    width: calc(var(--space) * 3);
    border-top: 1px solid var(--line);
  }
  .output {
    font-size: var(--fs-sm);
  }
  .output.solo {
    margin-left: calc(var(--space) * 6);
  }
  .output :global(.codeblock) {
    margin: var(--space) 0 calc(var(--space) * 2);
  }
</style>
