<script lang="ts">
  import Icon from "./Icon.svelte";
  import { mdHtml, richReady } from "./md";

  /** The agent's thinking: one quiet line, its words inside. */
  export let text = "";
  /** It is thinking right now. */
  export let live = false;

  let open = false;
  let drawn = false;
  $: if (open) drawn = true;
</script>

<div class="thinking">
  <button class="head" aria-expanded={open} on:click={() => (open = !open)}>
    <span class="chev" class:open><Icon name="chevDown" size={12} /></span>
    <span class="verb" class:shine={live}>{live ? "Thinking" : "Thought"}</span>
  </button>
  <div class="fold" class:open inert={!open}>
    <div class="fold-in">
      {#if drawn}<div class="words">{@html mdHtml(text, $richReady)}</div>{/if}
    </div>
  </div>
</div>

<style>
  .thinking {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .head {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-height: var(--h);
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-md);
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
  .words {
    max-height: 320px;
    overflow-y: auto;
    margin-left: calc(var(--space) * 2);
    padding: var(--space) 0 var(--space) calc(var(--space) * 4);
    border-left: 1px solid var(--line);
    color: var(--muted);
    font-size: var(--fs-md);
    line-height: 1.6;
  }
  .words :global(p) {
    margin: 0 0 0.6em;
  }
  .words :global(p:last-child) {
    margin-bottom: 0;
  }
</style>
