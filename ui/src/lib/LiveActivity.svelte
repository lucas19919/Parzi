<script lang="ts">
  import { SLIDE } from "./motion";
  import { duration } from "./format";
  import { createEventDispatcher } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import ThinkingFace from "./ThinkingFace.svelte";
  import type { LiveTool } from "./live";

  /** What a running agent is doing, in the thread, without opening a fold:
   *  its latest steps as they run, and the last thing it said or thought,
   *  kept light: two steps, one line, a time only for a slow step.
   *  At the end of the turn it folds into "Worked for…" with the rest. */
  export let tools: LiveTool[] = [];
  export let said = "";
  export let shown = 3;
  /** Subagents this session started that are still working (or waiting
   *  on the person), each with the step it is on. */
  export let helpers: { id: string; title: string; need: boolean; step: string }[] = [];

  const dispatch = createEventDispatcher<{ open: { id: string } }>();

  $: recent = tools.slice(-shown);
</script>

<div class="activity">
  {#if said}<p class="said">{said}</p>{/if}
  {#each recent as t (t.id)}
    <div class="step" class:done={!t.running} class:failed={!t.running && !t.ok} in:fly|local={{ y: 4, duration: SLIDE, easing: cubicOut }}>
      <span class="mark">
        {#if t.running}<span class="spin" aria-label="Running" />{:else if t.ok}<Icon name="check" size={12} />{:else}<Icon name="close" size={12} />{/if}
      </span>
      <span class="label">{t.label}</span>
      {#if !t.running && t.ms >= 5000}<span class="ms">{duration(t.ms)}</span>{/if}
    </div>
  {/each}
  {#if helpers.length}
    <div class="helpers">
      {#each helpers as h (h.id)}
        <button class="helper" on:click={() => dispatch("open", { id: h.id })} in:fly|local={{ y: 4, duration: SLIDE, easing: cubicOut }}>
          <span class="mark">{#if h.need}<i class="need" />{:else}<ThinkingFace size={14} />{/if}</span>
          <span class="who">{h.title}</span>
          <span class="what">{h.need ? "needs you" : h.step || "working"}</span>
          <span class="go"><Icon name="arrowRight" size={12} /></span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  /* The same two columns as the working line below: a 22px mark (the
     face's width) at the thread's edge, then the text. Every row 24px. */
  .activity {
    --mark: 22px;
    --col: calc(var(--space) * 3);
    display: flex;
    flex-direction: column;
  }
  .said {
    margin: 0 0 var(--space) calc(var(--mark) + var(--col));
    color: var(--muted);
    font-size: var(--fs-md);
    line-height: 20px;
    display: -webkit-box;
    -webkit-line-clamp: 1;
    line-clamp: 1;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .step,
  .helper {
    display: flex;
    align-items: center;
    gap: var(--col);
    min-width: 0;
    height: var(--h-sm);
    font-size: var(--fs-md);
    line-height: 20px;
    color: var(--text);
  }
  .step.done {
    color: var(--muted);
  }
  .step.failed .mark {
    color: var(--bad-text);
  }
  .mark {
    flex: none;
    width: var(--mark);
    height: 20px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
  }
  .spin {
    box-sizing: border-box;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 1.5px solid color-mix(in srgb, var(--text) 25%, transparent);
    border-top-color: var(--text);
    animation: spin 0.9s linear infinite;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ms {
    flex: none;
    margin-left: auto;
    font-size: var(--fs-xs);
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  /* A subagent row is a button whose hover reaches a little past the
     columns; its content stays on them. */
  .helper {
    width: calc(100% + var(--space) * 4);
    margin: 0 calc(var(--space) * -2);
    padding: 0 calc(var(--space) * 2);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    font: inherit;
    font-size: var(--fs-md);
    line-height: 20px;
    text-align: left;
    cursor: pointer;
  }
  .helper:hover {
    background: var(--hover);
  }
  .who {
    flex: none;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .what {
    min-width: 0;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .go {
    flex: none;
    margin-left: auto;
    display: inline-flex;
    color: var(--faint);
    opacity: 0;
  }
  .helper:hover .go {
    opacity: 1;
  }
  .need {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--warn);
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
</style>
